use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;

use forgeerp_auth::Permission;
use forgeerp_core::AggregateId;
use forgeerp_products::{ActivateProduct, ArchiveProduct, CreateProduct, Product, ProductCommand, ProductId};

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new()
        .route("/", post(create_product).get(list_products))
        .route("/:id", get(get_product))
        .route("/:id/activate", post(activate_product))
        .route("/:id/archive", post(archive_product))
}

pub async fn create_product(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::CreateProductRequest>,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let product_id = ProductId::new(agg);

    let cmd = ProductCommand::CreateProduct(CreateProduct {
        tenant_id: tenant.tenant_id(),
        product_id,
        sku: body.sku,
        name: body.name,
        pricing: body.pricing,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("products.create")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Product>(
        tenant.tenant_id(),
        agg,
        "products.product",
        cmd_auth.inner,
        |_t, aggregate_id| Product::empty(ProductId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
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

pub async fn activate_product(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
    };
    let product_id = ProductId::new(agg);

    let cmd = ProductCommand::ActivateProduct(ActivateProduct {
        tenant_id: tenant.tenant_id(),
        product_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("products.activate")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Product>(
        tenant.tenant_id(),
        agg,
        "products.product",
        cmd_auth.inner,
        |_t, aggregate_id| Product::empty(ProductId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn archive_product(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
    };
    let product_id = ProductId::new(agg);

    let cmd = ProductCommand::ArchiveProduct(ArchiveProduct {
        tenant_id: tenant.tenant_id(),
        product_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("products.archive")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Product>(
        tenant.tenant_id(),
        agg,
        "products.product",
        cmd_auth.inner,
        |_t, aggregate_id| Product::empty(ProductId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn get_product(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
    };
    let product_id = ProductId::new(agg);
    match services.products_get(tenant.tenant_id(), &product_id) {
        Some(rm) => (StatusCode::OK, Json(dto::product_to_json(rm))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "product not found"),
    }
}

pub async fn list_products(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .products_list(tenant.tenant_id())
        .into_iter()
        .map(dto::product_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}


