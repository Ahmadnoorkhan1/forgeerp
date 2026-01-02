use axum::{
    Router,
    http::StatusCode,
    routing::get,
    response::IntoResponse,
    Json,
};
use tower::ServiceBuilder;
use std::sync::Arc;
use axum::routing::post;
use axum::extract::{Path, Extension};
use chrono::Utc;

use forgeerp_auth::{CommandAuthorization, Permission};
use forgeerp_core::AggregateId;
use forgeerp_events::{EventBus, EventEnvelope, InMemoryEventBus};
use forgeerp_infra::command_dispatcher::{CommandDispatcher, DispatchError};
use forgeerp_infra::event_store::InMemoryEventStore;
use forgeerp_infra::projections::inventory_stock::{InventoryReadModel, InventoryStockProjection};
use forgeerp_infra::read_model::InMemoryTenantStore;
use forgeerp_inventory::{AdjustStock, CreateItem, InventoryCommand, InventoryItem, InventoryItemId};
use serde::Deserialize;

#[tokio::main]
async fn main() {
    forgeerp_observability::init();

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("JWT_SECRET not set; using insecure dev default");
        "dev-secret".to_string()
    });

    let jwt = Arc::new(forgeerp_auth::Hs256JwtValidator::new(jwt_secret.into_bytes()));
    let auth_state = forgeerp_api::middleware::AuthState { jwt };

    // In-memory infra wiring (dev): store + bus + projection.
    let store = Arc::new(InMemoryEventStore::new());
    let bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>> = Arc::new(InMemoryEventBus::new());

    let rm_store: Arc<InMemoryTenantStore<InventoryItemId, InventoryReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let projection: Arc<InventoryStockProjection<_>> =
        Arc::new(InventoryStockProjection::new(rm_store));

    // Background subscriber: bus -> projection
    {
        let sub = bus.subscribe();
        let projection = projection.clone();
        tokio::task::spawn_blocking(move || loop {
            match sub.recv() {
                Ok(env) => {
                    if let Err(e) = projection.apply_envelope(&env) {
                        tracing::warn!("projection apply failed: {e}");
                    }
                }
                Err(_) => break,
            }
        });
    }

    let dispatcher: Arc<CommandDispatcher<_, _>> = Arc::new(CommandDispatcher::new(store, bus));
    let services = Arc::new(AppServices { dispatcher, projection });

    // Protected routes: require auth + tenant context.
    let protected = Router::new()
        .route("/whoami", get(whoami))
        .nest("/inventory", inventory_router())
        .layer(Extension(services))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            forgeerp_api::middleware::auth_middleware,
        ));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(ServiceBuilder::new());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind 0.0.0.0:8080");

    tracing::info!("listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn whoami(
    axum::extract::Extension(tenant): axum::extract::Extension<forgeerp_api::context::TenantContext>,
    axum::extract::Extension(principal): axum::extract::Extension<forgeerp_api::context::PrincipalContext>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "tenant_id": tenant.tenant_id().to_string(),
        "principal_id": principal.principal_id().to_string(),
        "roles": principal.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>(),
    }))
}

#[derive(Clone)]
struct AppServices {
    dispatcher: Arc<CommandDispatcher<Arc<InMemoryEventStore>, Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>>>,
    projection: Arc<InventoryStockProjection<Arc<InMemoryTenantStore<InventoryItemId, InventoryReadModel>>>>,
}

fn inventory_router() -> Router {
    Router::new()
        .route("/items", post(create_item))
        .route("/items/{id}/adjust", post(adjust_stock))
        .route("/items/{id}", get(get_item))
}

#[derive(Debug, Deserialize)]
struct CreateItemRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AdjustStockRequest {
    delta: i64,
}

struct CreateItemCommandAuth {
    inner: InventoryCommand,
    required: Vec<Permission>,
}

impl CommandAuthorization for CreateItemCommandAuth {
    fn required_permissions(&self) -> &[Permission] {
        &self.required
    }
}

struct AdjustStockCommandAuth {
    inner: InventoryCommand,
    required: Vec<Permission>,
}

impl CommandAuthorization for AdjustStockCommandAuth {
    fn required_permissions(&self) -> &[Permission] {
        &self.required
    }
}

async fn create_item(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<forgeerp_api::context::TenantContext>,
    Extension(principal): Extension<forgeerp_api::context::PrincipalContext>,
    Json(body): Json<CreateItemRequest>,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let item_id = InventoryItemId::new(agg);

    let cmd = InventoryCommand::CreateItem(CreateItem {
        tenant_id: tenant.tenant_id(),
        item_id,
        name: body.name,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CreateItemCommandAuth {
        inner: cmd,
        required: vec![Permission::new("inventory.items.create")],
    };

    if let Err(e) = forgeerp_api::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatcher.dispatch::<InventoryItem>(
        tenant.tenant_id(),
        agg,
        "inventory.item",
        cmd_auth.inner,
        |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return dispatch_error_to_response(e),
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": agg.to_string(),
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

async fn adjust_stock(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<forgeerp_api::context::TenantContext>,
    Extension(principal): Extension<forgeerp_api::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<AdjustStockRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid item id"),
    };

    let item_id = InventoryItemId::new(agg);

    let cmd = InventoryCommand::AdjustStock(AdjustStock {
        tenant_id: tenant.tenant_id(),
        item_id,
        delta: body.delta,
        occurred_at: Utc::now(),
    });

    let cmd_auth = AdjustStockCommandAuth {
        inner: cmd,
        required: vec![Permission::new("inventory.items.adjust")],
    };

    if let Err(e) = forgeerp_api::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatcher.dispatch::<InventoryItem>(
        tenant.tenant_id(),
        agg,
        "inventory.item",
        cmd_auth.inner,
        |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": agg.to_string(),
            "events_committed": committed.len(),
            "stream_version": committed.last().map(|e| e.sequence_number).unwrap_or(0),
        })),
    )
        .into_response()
}

async fn get_item(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<forgeerp_api::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid item id"),
    };

    let item_id = InventoryItemId::new(agg);
    match services.projection.get(tenant.tenant_id(), &item_id) {
        Some(rm) => (StatusCode::OK, Json(read_model_to_json(rm))).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "not_found", "item not found"),
    }
}

fn read_model_to_json(rm: InventoryReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.item_id.0.to_string(),
        "name": rm.name,
        "quantity": rm.quantity,
    })
}

fn dispatch_error_to_response(err: DispatchError) -> axum::response::Response {
    match err {
        DispatchError::Concurrency(msg) => json_error(StatusCode::CONFLICT, "conflict", msg),
        DispatchError::Validation(msg) => json_error(StatusCode::BAD_REQUEST, "validation_error", msg),
        DispatchError::InvariantViolation(msg) => {
            json_error(StatusCode::UNPROCESSABLE_ENTITY, "invariant_violation", msg)
        }
        DispatchError::Unauthorized => json_error(StatusCode::FORBIDDEN, "unauthorized", "unauthorized"),
        DispatchError::NotFound => json_error(StatusCode::NOT_FOUND, "not_found", "not found"),
        DispatchError::Deserialize(msg) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "deserialize_error", msg),
        DispatchError::Store(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "store_error", format!("{e:?}")),
        DispatchError::Publish(msg) => json_error(StatusCode::BAD_GATEWAY, "publish_error", msg),
        DispatchError::TenantIsolation(msg) => json_error(StatusCode::FORBIDDEN, "tenant_isolation", msg),
    }
}

fn json_error(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> axum::response::Response {
    (
        status,
        Json(serde_json::json!({
            "error": code,
            "message": message.into(),
        })),
    )
        .into_response()
}


