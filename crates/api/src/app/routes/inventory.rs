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
use forgeerp_inventory::{AdjustStock, CreateItem, InventoryCommand, InventoryItem, InventoryItemId};

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new()
        .route("/anomalies", get(get_inventory_anomalies))
        .route("/:id/insights", get(get_inventory_item_insights))
        .route("/items", post(create_item))
        .route("/items/:id/adjust", post(adjust_stock))
        .route("/items/:id", get(get_item))
}

pub async fn create_item(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::CreateItemRequest>,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let item_id = InventoryItemId::new(agg);

    let cmd = InventoryCommand::CreateItem(CreateItem {
        tenant_id: tenant.tenant_id(),
        item_id,
        name: body.name,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("inventory.items.create")],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<InventoryItem>(
        tenant.tenant_id(),
        agg,
        "inventory.item",
        cmd_auth.inner,
        |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
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

pub async fn adjust_stock(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::AdjustStockRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid item id"),
    };

    let item_id = InventoryItemId::new(agg);

    let cmd = InventoryCommand::AdjustStock(AdjustStock {
        tenant_id: tenant.tenant_id(),
        item_id,
        delta: body.delta,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("inventory.items.adjust")],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<InventoryItem>(
        tenant.tenant_id(),
        agg,
        "inventory.item",
        cmd_auth.inner,
        |_tenant_id, aggregate_id| InventoryItem::empty(InventoryItemId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
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

pub async fn get_item(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid item id"),
    };

    let item_id = InventoryItemId::new(agg);
    let rm = services.inventory_get(tenant.tenant_id(), &item_id);
    match rm {
        Some(rm) => (StatusCode::OK, Json(dto::inventory_to_json(rm))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "item not found"),
    }
}

pub async fn get_inventory_anomalies(
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

pub async fn get_inventory_item_insights(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let tenant_id = tenant.tenant_id();
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid inventory id"),
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


