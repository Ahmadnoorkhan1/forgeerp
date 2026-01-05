use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{sse::{Event as SseEvent, KeepAlive, Sse}, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use std::{collections::HashMap, convert::Infallible, sync::{Arc, Mutex}, time::Duration};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower::ServiceBuilder;

use forgeerp_auth::{CommandAuthorization, Permission};
use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::{EventBus, EventEnvelope, InMemoryEventBus};
use forgeerp_ai::AiResult;
use forgeerp_infra::{
    ai::{AiInsightSink, InventoryAnomalyRunner, InventoryAnomalyRunnerHandle},
    command_dispatcher::{CommandDispatcher, DispatchError},
    event_store::{InMemoryEventStore, StoredEvent},
    projections::inventory_stock::{InventoryReadModel, InventoryStockProjection},
    read_model::InMemoryTenantStore,
};
#[cfg(feature = "redis")]
use forgeerp_infra::{
    event_bus::RedisStreamsEventBus,
    event_store::PostgresEventStore,
    read_model::PostgresInventoryStore,
};
use forgeerp_inventory::{AdjustStock, CreateItem, InventoryCommand, InventoryItem, InventoryItemId};
use serde::{Deserialize, Serialize};
#[cfg(feature = "redis")]
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize)]
struct RealtimeMessage {
    tenant_id: TenantId,
    topic: String,
    payload: serde_json::Value,
}

/// API-local AI insight sink that stores results and broadcasts "insight available" notifications.
#[derive(Debug)]
struct ApiAiInsightSink {
    inner: Mutex<Vec<(TenantId, AiResult)>>,
    realtime_tx: broadcast::Sender<RealtimeMessage>,
}

impl ApiAiInsightSink {
    fn new(realtime_tx: broadcast::Sender<RealtimeMessage>) -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            realtime_tx,
        }
    }

    fn all(&self) -> Vec<(TenantId, AiResult)> {
        self.inner.lock().unwrap().clone()
    }
}

impl AiInsightSink for ApiAiInsightSink {
    fn emit(&self, tenant_id: TenantId, result: AiResult) {
        self.inner.lock().unwrap().push((tenant_id, result.clone()));

        // Broadcast that new insights are available (lossy; no backpressure on core).
        let _ = self.realtime_tx.send(RealtimeMessage {
            tenant_id,
            topic: "ai.insight_available".to_string(),
            payload: serde_json::json!({
                "kind": "insights",
                "insight_type": "ai.result",
                "metadata": result.metadata,
            }),
        });
    }
}

// Type-erased dispatcher for in-memory implementations
type InMemoryDispatcher = CommandDispatcher<
    Arc<InMemoryEventStore>,
    Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>,
>;

// Type-erased dispatcher for persistent implementations
#[cfg(feature = "redis")]
type PersistentDispatcher = CommandDispatcher<
    Arc<PostgresEventStore>,
    Arc<RedisStreamsEventBus>,
>;

#[derive(Clone)]
enum AppServices {
    InMemory {
        dispatcher: Arc<InMemoryDispatcher>,
        projection: Arc<InventoryStockProjection<Arc<InMemoryTenantStore<InventoryItemId, InventoryReadModel>>>>,
        ai_sink: Arc<ApiAiInsightSink>,
        realtime_tx: broadcast::Sender<RealtimeMessage>,
    },
    #[cfg(feature = "redis")]
    Persistent {
        dispatcher: Arc<PersistentDispatcher>,
        projection: Arc<InventoryStockProjection<Arc<PostgresInventoryStore>>>,
        ai_sink: Arc<ApiAiInsightSink>,
        realtime_tx: broadcast::Sender<RealtimeMessage>,
        bus: Arc<RedisStreamsEventBus>,
    },
}

fn build_in_memory_services() -> AppServices {
    // In-memory infra wiring (dev/test): store + bus + projection.
    let store = Arc::new(InMemoryEventStore::new());
    let bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>> = Arc::new(InMemoryEventBus::new());

    let rm_store: Arc<InMemoryTenantStore<InventoryItemId, InventoryReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let projection: Arc<InventoryStockProjection<_>> =
        Arc::new(InventoryStockProjection::new(rm_store));

    // Realtime channel (SSE): lossy broadcast, tenant-filtered in handlers.
    let (realtime_tx, _realtime_rx) = broadcast::channel::<RealtimeMessage>(256);

    // AI wiring (dev/test): in-memory insights + per-tenant anomaly runners.
    let ai_sink: Arc<ApiAiInsightSink> = Arc::new(ApiAiInsightSink::new(realtime_tx.clone()));
    let ai_runners: Arc<Mutex<HashMap<TenantId, InventoryAnomalyRunnerHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let ai_runner_cfg = InventoryAnomalyRunner::default();

    // Background subscriber: bus -> projection
    {
        let sub = bus.subscribe();
        let projection = projection.clone();
        let ai_sink = ai_sink.clone();
        let ai_runners = ai_runners.clone();
        let realtime_tx = realtime_tx.clone();
        tokio::task::spawn_blocking(move || loop {
            match sub.recv() {
                Ok(env) => {
                    if let Err(e) = projection.apply_envelope(&env) {
                        tracing::warn!("projection apply failed: {e}");
                        continue;
                    }

                    // Broadcast projection update (lossy; no backpressure on core).
                    let _ = realtime_tx.send(RealtimeMessage {
                        tenant_id: env.tenant_id(),
                        topic: "inventory.projection_updated".to_string(),
                        payload: serde_json::json!({
                            "kind": "projection_update",
                            "aggregate_type": env.aggregate_type(),
                            "aggregate_id": env.aggregate_id().to_string(),
                            "sequence_number": env.sequence_number(),
                        }),
                    });

                    // Event-triggered AI execution (after successful projection update).
                    let tenant_id = env.tenant_id();
                    let mut runners = ai_runners.lock().unwrap();
                    let handle = runners.entry(tenant_id).or_insert_with(|| {
                        ai_runner_cfg.spawn_for_tenant(
                            "ai.inventory_anomaly",
                            tenant_id,
                            projection.clone(),
                            ai_sink.clone(),
                        )
                    });
                    handle.trigger();
                }
                Err(_) => break,
            }
        });
    }

    let dispatcher: Arc<InMemoryDispatcher> = Arc::new(CommandDispatcher::new(store, bus));
    AppServices::InMemory {
        dispatcher,
        projection,
        ai_sink,
        realtime_tx,
    }
}

#[cfg(feature = "redis")]
async fn build_persistent_services() -> AppServices {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set when USE_PERSISTENT_STORES=true");
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://localhost:6379".to_string());

    // Create Postgres connection pool
    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    // Create persistent event store
    let store = Arc::new(PostgresEventStore::new(pool.clone()));

    // Create Redis Streams event bus
    let bus = Arc::new(
        RedisStreamsEventBus::new(&redis_url, None, None)
            .expect("Failed to create Redis Streams event bus")
    );

    // Ensure consumer group exists for inventory projection
    bus.ensure_consumer_group("inventory.projection")
        .expect("Failed to create consumer group");

    // Create persistent read model store
    let rm_store = Arc::new(PostgresInventoryStore::new(pool));
    let projection: Arc<InventoryStockProjection<_>> =
        Arc::new(InventoryStockProjection::new(rm_store));

    // Realtime channel (SSE): lossy broadcast, tenant-filtered in handlers.
    let (realtime_tx, _realtime_rx) = broadcast::channel::<RealtimeMessage>(256);

    // AI wiring (dev/test): in-memory insights + per-tenant anomaly runners.
    let ai_sink: Arc<ApiAiInsightSink> = Arc::new(ApiAiInsightSink::new(realtime_tx.clone()));
    let ai_runners: Arc<Mutex<HashMap<TenantId, InventoryAnomalyRunnerHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let ai_runner_cfg = InventoryAnomalyRunner::default();

    // Background subscriber: bus -> projection (Redis Streams with consumer group)
    {
        let bus = bus.clone();
        let projection = projection.clone();
        let ai_sink = ai_sink.clone();
        let ai_runners = ai_runners.clone();
        let realtime_tx = realtime_tx.clone();
        tokio::task::spawn_blocking(move || {
            let sub = bus.subscribe_with_group(
                "inventory.projection",
                &format!("consumer-{}", uuid::Uuid::now_v7()),
                None, // No tenant filter - process all tenants
            );
            loop {
                match sub.recv() {
                    Ok(env) => {
                        if let Err(e) = projection.apply_envelope(&env) {
                            tracing::warn!("projection apply failed: {e}");
                            continue;
                        }

                        // Broadcast projection update (lossy; no backpressure on core).
                        let _ = realtime_tx.send(RealtimeMessage {
                            tenant_id: env.tenant_id(),
                            topic: "inventory.projection_updated".to_string(),
                            payload: serde_json::json!({
                                "kind": "projection_update",
                                "aggregate_type": env.aggregate_type(),
                                "aggregate_id": env.aggregate_id().to_string(),
                                "sequence_number": env.sequence_number(),
                            }),
                        });

                        // Event-triggered AI execution (after successful projection update).
                        let tenant_id = env.tenant_id();
                        let mut runners = ai_runners.lock().unwrap();
                        let handle = runners.entry(tenant_id).or_insert_with(|| {
                            ai_runner_cfg.spawn_for_tenant(
                                "ai.inventory_anomaly",
                                tenant_id,
                                projection.clone(),
                                ai_sink.clone(),
                            )
                        });
                        handle.trigger();
                    }
                    Err(_) => break,
                }
            }
        });
    }

    let dispatcher: Arc<PersistentDispatcher> = Arc::new(CommandDispatcher::new(store, bus.clone()));
    AppServices::Persistent {
        dispatcher,
        projection,
        ai_sink,
        realtime_tx,
        bus,
    }
}

impl AppServices {
    fn realtime_tx(&self) -> &broadcast::Sender<RealtimeMessage> {
        match self {
            AppServices::InMemory { realtime_tx, .. } => realtime_tx,
            #[cfg(feature = "redis")]
            AppServices::Persistent { realtime_tx, .. } => realtime_tx,
        }
    }

    fn ai_sink(&self) -> &Arc<ApiAiInsightSink> {
        match self {
            AppServices::InMemory { ai_sink, .. } => ai_sink,
            #[cfg(feature = "redis")]
            AppServices::Persistent { ai_sink, .. } => ai_sink,
        }
    }

    fn dispatch_in_memory(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: &str,
        command: InventoryCommand,
        empty_fn: impl FnOnce(TenantId, AggregateId) -> InventoryItem,
    ) -> Result<Vec<StoredEvent>, DispatchError> {
        match self {
            AppServices::InMemory { dispatcher, .. } => {
                dispatcher.dispatch::<InventoryItem>(tenant_id, aggregate_id, aggregate_type, command, empty_fn)
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { .. } => {
                unreachable!("dispatch_in_memory called on Persistent services")
            }
        }
    }

    #[cfg(feature = "redis")]
    fn dispatch_persistent(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: &str,
        command: InventoryCommand,
        empty_fn: impl FnOnce(TenantId, AggregateId) -> InventoryItem,
    ) -> Result<Vec<StoredEvent>, DispatchError> {
        match self {
            AppServices::InMemory { .. } => {
                unreachable!("dispatch_persistent called on InMemory services")
            }
            AppServices::Persistent { dispatcher, .. } => {
                dispatcher.dispatch::<InventoryItem>(tenant_id, aggregate_id, aggregate_type, command, empty_fn)
            }
        }
    }

    fn get_item_in_memory(&self, tenant_id: TenantId, item_id: &InventoryItemId) -> Option<InventoryReadModel> {
        match self {
            AppServices::InMemory { projection, .. } => {
                projection.get(tenant_id, item_id)
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { .. } => {
                unreachable!("get_item_in_memory called on Persistent services")
            }
        }
    }

    #[cfg(feature = "redis")]
    fn get_item_persistent(&self, tenant_id: TenantId, item_id: &InventoryItemId) -> Option<InventoryReadModel> {
        match self {
            AppServices::InMemory { .. } => {
                unreachable!("get_item_persistent called on InMemory services")
            }
            AppServices::Persistent { projection, .. } => {
                projection.get(tenant_id, item_id)
            }
        }
    }
}

pub async fn build_app(jwt_secret: String) -> Router {
    let jwt = Arc::new(forgeerp_auth::Hs256JwtValidator::new(jwt_secret.into_bytes()));
    let auth_state = crate::middleware::AuthState { jwt };

    // Check environment variables to determine which implementation to use
    let use_persistent = std::env::var("USE_PERSISTENT_STORES")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    let services = if use_persistent {
        #[cfg(feature = "redis")]
        {
            build_persistent_services().await
        }
        #[cfg(not(feature = "redis"))]
        {
            tracing::warn!("USE_PERSISTENT_STORES=true but redis feature not enabled, falling back to in-memory");
            build_in_memory_services()
        }
    } else {
        build_in_memory_services()
    };

    let services = Arc::new(services);

    // Protected routes: require auth + tenant context.
    let protected = Router::new()
        .route("/whoami", get(whoami))
        .route("/stream", get(stream))
        .nest("/inventory", inventory_router())
        .layer(Extension(services))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            crate::middleware::auth_middleware,
        ));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(ServiceBuilder::new())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn whoami(
    axum::extract::Extension(tenant): axum::extract::Extension<crate::context::TenantContext>,
    axum::extract::Extension(principal): axum::extract::Extension<crate::context::PrincipalContext>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "tenant_id": tenant.tenant_id().to_string(),
        "principal_id": principal.principal_id().to_string(),
        "roles": principal.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>(),
    }))
}

async fn stream(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let tenant_id = tenant.tenant_id();
    let rx = services.realtime_tx().subscribe();

    let stream = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(m) if m.tenant_id == tenant_id => {
            let data = serde_json::to_string(&m.payload).unwrap_or_else(|_| "{}".to_string());
            Some(Ok(SseEvent::default().event(m.topic).data(data)))
        }
        _ => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

fn inventory_router() -> Router {
    Router::new()
        .route("/anomalies", get(get_inventory_anomalies))
        .route("/:id/insights", get(get_inventory_item_insights))
        .route("/items", post(create_item))
        .route("/items/:id/adjust", post(adjust_stock))
        .route("/items/:id", get(get_item))
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
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
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

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match match &*services {
        AppServices::InMemory { .. } => {
            services.dispatch_in_memory(
                tenant.tenant_id(),
                agg,
                "inventory.item",
                cmd_auth.inner,
                |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
            )
        }
        #[cfg(feature = "redis")]
        AppServices::Persistent { .. } => {
            services.dispatch_persistent(
                tenant.tenant_id(),
                agg,
                "inventory.item",
                cmd_auth.inner,
                |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
            )
        }
    } {
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
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
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

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match match &*services {
        AppServices::InMemory { .. } => {
            services.dispatch_in_memory(
                tenant.tenant_id(),
                agg,
                "inventory.item",
                cmd_auth.inner,
                |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
            )
        }
        #[cfg(feature = "redis")]
        AppServices::Persistent { .. } => {
            services.dispatch_persistent(
                tenant.tenant_id(),
                agg,
                "inventory.item",
                cmd_auth.inner,
                |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
            )
        }
    } {
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
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid item id"),
    };

    let item_id = InventoryItemId::new(agg);
    let rm = match &*services {
        AppServices::InMemory { .. } => {
            services.get_item_in_memory(tenant.tenant_id(), &item_id)
        }
        #[cfg(feature = "redis")]
        AppServices::Persistent { .. } => {
            services.get_item_persistent(tenant.tenant_id(), &item_id)
        }
    };
    match rm {
        Some(rm) => (StatusCode::OK, Json(read_model_to_json(rm))).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "not_found", "item not found"),
    }
}

async fn get_inventory_anomalies(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let tenant_id = tenant.tenant_id();
    let all = services.ai_sink().all();

    let mut anomalies: Vec<serde_json::Value> = Vec::new();
    for (t, r) in all {
        if t != tenant_id {
            continue;
        }
        if r.metadata.get("kind").and_then(|v| v.as_str()) != Some("inventory.anomaly_detection") {
            continue;
        }
        if let Some(arr) = r.metadata.get("anomalies").and_then(|v| v.as_array()) {
            anomalies.extend(arr.iter().cloned());
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "kind": "insights",
            "insight_type": "inventory.anomalies",
            "count": anomalies.len(),
            "anomalies": anomalies,
        })),
    )
        .into_response()
}

async fn get_inventory_item_insights(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let tenant_id = tenant.tenant_id();
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid inventory id"),
    };
    let item_id = agg.to_string();

    let all = services.ai_sink().all();
    let mut item_anomalies: Vec<serde_json::Value> = Vec::new();

    for (t, r) in all {
        if t != tenant_id {
            continue;
        }
        if r.metadata.get("kind").and_then(|v| v.as_str()) != Some("inventory.anomaly_detection") {
            continue;
        }
        if let Some(arr) = r.metadata.get("anomalies").and_then(|v| v.as_array()) {
            for a in arr {
                if a.get("item_id").and_then(|v| v.as_str()) == Some(item_id.as_str()) {
                    item_anomalies.push(a.clone());
                }
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "kind": "insights",
            "insight_type": "inventory.item",
            "item_id": item_id,
            "anomalies": item_anomalies,
        })),
    )
        .into_response()
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


