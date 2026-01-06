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
use forgeerp_parties::PartyId;
use forgeerp_products::ProductId;
use forgeerp_purchasing::{
    AddLine as AddPurchaseLine, Approve, CreatePurchaseOrder, PurchaseOrder, PurchaseOrderCommand,
    PurchaseOrderId, ReceiveGoods,
};

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new().nest("/orders", orders_router())
}

fn orders_router() -> Router {
    Router::new()
        .route("/", post(create_purchase_order).get(list_purchase_orders))
        .route("/:id", get(get_purchase_order))
        .route("/:id/lines", post(add_purchase_order_line))
        .route("/:id/approve", post(approve_purchase_order))
        .route("/:id/receive", post(receive_purchase_order_goods))
}

pub async fn create_purchase_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::CreatePurchaseOrderRequest>,
) -> axum::response::Response {
    let supplier_agg: AggregateId = match body.supplier_id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid supplier_id"),
    };
    let supplier_id = PartyId::new(supplier_agg);

    let order_agg = AggregateId::new();
    let order_id = PurchaseOrderId::new(order_agg);

    // 1) Create order
    let cmd = PurchaseOrderCommand::CreatePurchaseOrder(CreatePurchaseOrder {
        tenant_id: tenant.tenant_id(),
        order_id,
        supplier_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("purchases.orders.create")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let mut committed_total = 0usize;
    let committed = match services.dispatch::<PurchaseOrder>(
        tenant.tenant_id(),
        order_agg,
        "purchasing.order",
        cmd_auth.inner,
        |_t, aggregate_id| PurchaseOrder::empty(PurchaseOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };
    committed_total += committed.len();

    // 2) Add lines
    for l in body.lines {
        let prod_agg: AggregateId = match l.product_id.parse() {
            Ok(v) => v,
            Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
        };
        let add_cmd = PurchaseOrderCommand::AddLine(AddPurchaseLine {
            tenant_id: tenant.tenant_id(),
            order_id,
            product_id: ProductId::new(prod_agg),
            quantity: l.quantity,
            occurred_at: Utc::now(),
        });
        let add_auth = CmdAuth {
            inner: add_cmd,
            required: vec![Permission::new("purchases.orders.add_line")],
        };
        if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &add_auth) {
            return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
        }
        let committed = match services.dispatch::<PurchaseOrder>(
            tenant.tenant_id(),
            order_agg,
            "purchasing.order",
            add_auth.inner,
            |_t, aggregate_id| PurchaseOrder::empty(PurchaseOrderId::new(aggregate_id)),
        ) {
            Ok(c) => c,
            Err(e) => return errors::dispatch_error_to_response(e),
        };
        committed_total += committed.len();
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": order_agg.to_string(),
            "events_committed": committed_total,
        })),
    )
        .into_response()
}

pub async fn add_purchase_order_line(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::PurchaseOrderLineRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid purchase order id"),
    };
    let order_id = PurchaseOrderId::new(agg);

    let prod_agg: AggregateId = match body.product_id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
    };

    let cmd = PurchaseOrderCommand::AddLine(AddPurchaseLine {
        tenant_id: tenant.tenant_id(),
        order_id,
        product_id: ProductId::new(prod_agg),
        quantity: body.quantity,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("purchases.orders.add_line")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<PurchaseOrder>(
        tenant.tenant_id(),
        agg,
        "purchasing.order",
        cmd_auth.inner,
        |_t, aggregate_id| PurchaseOrder::empty(PurchaseOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };
    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn approve_purchase_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid purchase order id"),
    };
    let order_id = PurchaseOrderId::new(agg);

    let cmd = PurchaseOrderCommand::Approve(Approve {
        tenant_id: tenant.tenant_id(),
        order_id,
        occurred_at: Utc::now(),
    });
    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("purchases.orders.approve")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<PurchaseOrder>(
        tenant.tenant_id(),
        agg,
        "purchasing.order",
        cmd_auth.inner,
        |_t, aggregate_id| PurchaseOrder::empty(PurchaseOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };
    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn receive_purchase_order_goods(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid purchase order id"),
    };
    let order_id = PurchaseOrderId::new(agg);

    let cmd = PurchaseOrderCommand::ReceiveGoods(ReceiveGoods {
        tenant_id: tenant.tenant_id(),
        order_id,
        occurred_at: Utc::now(),
    });
    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("purchases.orders.receive")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<PurchaseOrder>(
        tenant.tenant_id(),
        agg,
        "purchasing.order",
        cmd_auth.inner,
        |_t, aggregate_id| PurchaseOrder::empty(PurchaseOrderId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };
    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn get_purchase_order(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid purchase order id"),
    };
    let order_id = PurchaseOrderId::new(agg);
    match services.purchases_get(tenant.tenant_id(), &order_id) {
        Some(rm) => (StatusCode::OK, Json(dto::purchase_order_to_json(rm))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "purchase order not found"),
    }
}

pub async fn list_purchase_orders(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .purchases_list(tenant.tenant_id())
        .into_iter()
        .map(dto::purchase_order_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}


