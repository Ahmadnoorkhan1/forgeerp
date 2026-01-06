use std::sync::Arc;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{sse::Event as SseEvent, IntoResponse},
    Json,
};

use crate::app::services::{self, AppServices};

pub async fn health() -> StatusCode {
    StatusCode::OK
}

pub async fn whoami(
    axum::extract::Extension(tenant): axum::extract::Extension<crate::context::TenantContext>,
    axum::extract::Extension(principal): axum::extract::Extension<crate::context::PrincipalContext>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "tenant_id": tenant.tenant_id().to_string(),
        "principal_id": principal.principal_id().to_string(),
        "roles": principal.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>(),
    }))
}

pub async fn stream(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Sse<impl tokio_stream::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    services::tenant_sse_stream(services, tenant.tenant_id())
}


