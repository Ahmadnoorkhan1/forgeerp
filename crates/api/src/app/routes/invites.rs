//! Admin invite endpoint — allows tenant admins to invite new users.
//!
//! POST /admin/invites
//!   → Returns a short-lived token the invitee uses during registration.

use std::sync::Arc;

use axum::{extract::Extension, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::services::{AppServices, AuthStores, StoredInvite};
use crate::app::errors;
use crate::context::{PrincipalContext, TenantContext};

#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    /// Invite expires after this many hours (default 48).
    pub expires_hours: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct InviteResponse {
    pub token: String,
    pub tenant_id: String,
    pub tenant_name: String,
    pub expires_at: String,
}

pub fn router() -> Router {
    Router::new().route("/invites", post(create_invite))
}

/// POST /admin/invites — admin only.
pub async fn create_invite(
    Extension(_services): Extension<Arc<AppServices>>,
    Extension(auth_stores): Extension<Arc<AuthStores>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Json(body): Json<Option<CreateInviteRequest>>,
) -> impl IntoResponse {
    // Only admins may create invites
    let is_admin = principal.roles().iter().any(|r| r.as_str() == "admin");
    if !is_admin {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", "only admins can create invites");
    }

    let hours = body.as_ref().and_then(|b| b.expires_hours).unwrap_or(48).clamp(1, 168);
    let token = Uuid::new_v4().to_string().replace('-', "");
    let expires_at = Utc::now() + Duration::hours(hours);

    let tenant_name = auth_stores.tenants
        .get_name(tenant.tenant_id())
        .unwrap_or_else(|| "Workspace".to_string());

    auth_stores.invites.insert(StoredInvite {
        token: token.clone(),
        tenant_id: tenant.tenant_id(),
        tenant_name: tenant_name.clone(),
        expires_at,
    });

    (StatusCode::CREATED, Json(InviteResponse {
        token,
        tenant_id: tenant.tenant_id().to_string(),
        tenant_name,
        expires_at: expires_at.to_rfc3339(),
    })).into_response()
}
