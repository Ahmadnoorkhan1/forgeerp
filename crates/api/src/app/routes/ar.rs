use std::sync::Arc;

use axum::{extract::Extension, http::StatusCode, response::IntoResponse, routing::get, Json, Router};

use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new().route("/aging", get(get_ar_aging))
}

pub async fn get_ar_aging(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .ar_aging_list(tenant.tenant_id())
        .into_iter()
        .map(|rm| serde_json::json!({
            "invoice_id": rm.invoice_id.0.to_string(),
            "status": format!("{:?}", rm.status).to_lowercase(),
            "total_amount": rm.total_amount,
            "outstanding_amount": rm.outstanding_amount,
            "due_date": rm.due_date.map(|d| d.to_rfc3339()),
        }))
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}


