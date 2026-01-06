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
use forgeerp_products::ProductId;
use forgeerp_sales::{
    AddLine as AddSalesLine, ConfirmOrder, CreateSalesOrder, MarkInvoiced, SalesOrder, SalesOrderCommand,
    SalesOrderId,
};

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new().nest("/orders", orders_router())
}

fn orders_router() -> Router {
    Router::new()
        .route("/", post(create_sales_order).get(list_sales_orders))
        .route("/:id", get(get_sales_order))
        .route("/:id/lines", post(add_sales_order_line))
        .route("/:id/confirm", post(confirm_sales_order))
        .route("/:id/mark-invoiced", post(mark_sales_order_invoiced))
}

pub async fn create_sales_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let order_id = SalesOrderId::new(agg);

    let cmd = SalesOrderCommand::CreateSalesOrder(CreateSalesOrder {
        tenant_id: tenant.tenant_id(),
        order_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("sales.orders.create")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<SalesOrder>(
        tenant.tenant_id(),
        agg,
        "sales.order",
        cmd_auth.inner,
        |_t, aggregate_id| SalesOrder::empty(SalesOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::CREATED, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn add_sales_order_line(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::CreateSalesOrderLineRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid order id"),
    };
    let order_id = SalesOrderId::new(agg);

    let product_agg: AggregateId = match body.product_id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
    };

    let cmd = SalesOrderCommand::AddLine(AddSalesLine {
        tenant_id: tenant.tenant_id(),
        order_id,
        product_id: ProductId::new(product_agg),
        quantity: body.quantity,
        unit_price: body.unit_price,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("sales.orders.add_line")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<SalesOrder>(
        tenant.tenant_id(),
        agg,
        "sales.order",
        cmd_auth.inner,
        |_t, aggregate_id| SalesOrder::empty(SalesOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn confirm_sales_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid order id"),
    };
    let order_id = SalesOrderId::new(agg);

    let cmd = SalesOrderCommand::ConfirmOrder(ConfirmOrder {
        tenant_id: tenant.tenant_id(),
        order_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("sales.orders.confirm")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<SalesOrder>(
        tenant.tenant_id(),
        agg,
        "sales.order",
        cmd_auth.inner,
        |_t, aggregate_id| SalesOrder::empty(SalesOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn mark_sales_order_invoiced(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid order id"),
    };
    let order_id = SalesOrderId::new(agg);

    let cmd = SalesOrderCommand::MarkInvoiced(MarkInvoiced {
        tenant_id: tenant.tenant_id(),
        order_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("sales.orders.mark_invoiced")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<SalesOrder>(
        tenant.tenant_id(),
        agg,
        "sales.order",
        cmd_auth.inner,
        |_t, aggregate_id| SalesOrder::empty(SalesOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn get_sales_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid order id"),
    };
    let order_id = SalesOrderId::new(agg);
    match services.sales_get(tenant.tenant_id(), &order_id) {
        Some(rm) => (StatusCode::OK, Json(dto::sales_order_to_json(rm))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "sales order not found"),
    }
}

pub async fn list_sales_orders(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .sales_list(tenant.tenant_id())
        .into_iter()
        .map(dto::sales_order_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}


