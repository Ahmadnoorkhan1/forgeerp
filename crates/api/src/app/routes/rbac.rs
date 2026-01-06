//! RBAC audit endpoints for transparent authorization debugging.
//!
//! These endpoints provide visibility into authorization decisions, roles,
//! and permissions to help debug "Why was this request denied?" questions.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use forgeerp_auth::{
    admin, explain_authorization, Permission, Principal, RbacRegistry, TenantMembership,
};
use forgeerp_infra::projections::{default_role_permissions, UserReadModel};

use crate::app::{errors, services::AppServices};
use crate::app::routes::common::CmdAuth;
use crate::authz;
use crate::context::{PrincipalContext, TenantContext};

// ─────────────────────────────────────────────────────────────────────────────
// Query Parameters
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExplainAuthzQuery {
    pub permission: String,
    pub user_id: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new()
        .route("/roles", get(list_roles))
        .route("/roles/:name", get(get_role))
        .route("/permissions", get(list_permissions))
        .route("/permissions/:name", get(get_permission))
        .route("/explain", get(explain_authorization_decision))
        .route("/explain/:user_id", get(explain_user_authorization))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /admin/rbac/roles - List all available roles and their permissions
pub async fn list_roles(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
) -> axum::response::Response {
    // Check permission (admin users can inspect roles)
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let registry = RbacRegistry::from_role_mapping(default_role_permissions);
    let roles: Vec<_> = registry.roles.values().cloned().collect();

    (StatusCode::OK, Json(serde_json::json!({ "roles": roles }))).into_response()
}

/// GET /admin/rbac/roles/:name - Get details about a specific role
pub async fn get_role(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(name): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let registry = RbacRegistry::from_role_mapping(default_role_permissions);
    match registry.roles.get(&name) {
        Some(role) => (StatusCode::OK, Json(serde_json::json!({ "role": role }))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "role not found"),
    }
}

/// GET /admin/rbac/permissions - List all available permissions
pub async fn list_permissions(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let registry = RbacRegistry::from_role_mapping(default_role_permissions);
    let permissions: Vec<_> = registry.permissions.values().cloned().collect();

    (StatusCode::OK, Json(serde_json::json!({ "permissions": permissions }))).into_response()
}

/// GET /admin/rbac/permissions/:name - Get details about a specific permission
pub async fn get_permission(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(name): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let registry = RbacRegistry::from_role_mapping(default_role_permissions);
    match registry.permissions.get(&name) {
        Some(perm) => {
            (StatusCode::OK, Json(serde_json::json!({ "permission": perm }))).into_response()
        }
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "permission not found"),
    }
}

/// GET /admin/rbac/explain?permission=X - Explain why the current user can/cannot access a permission
pub async fn explain_authorization_decision(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Query(query): Query<ExplainAuthzQuery>,
) -> axum::response::Response {
    // Allow any authenticated user to check their own permissions
    let required_perm = Permission::new(query.permission);

    // Build principal from current request context
    let membership = TenantMembership {
        tenant_id: tenant.tenant_id(),
        roles: principal.roles().to_vec(),
        permissions: authz::permissions_from_roles(principal.roles()),
    };

    let principal_obj = Principal {
        principal_id: principal.principal_id(),
        active_tenant_id: tenant.tenant_id(),
        membership,
    };

    let explanation = explain_authorization(&principal_obj, &required_perm, default_role_permissions);

    (StatusCode::OK, Json(serde_json::json!({ "explanation": explanation }))).into_response()
}

/// GET /admin/rbac/explain/:user_id?permission=X - Explain why a specific user can/cannot access a permission
pub async fn explain_user_authorization(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(user_id_str): Path<String>,
    Query(query): Query<ExplainAuthzQuery>,
) -> axum::response::Response {
    // Check permission (admin only)
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    // Parse user ID
    let user_id = match user_id_str.parse::<uuid::Uuid>() {
        Ok(uuid) => forgeerp_auth::UserId::from_uuid(uuid),
        Err(_) => {
            return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id");
        }
    };

    // Get user read model
    let user = match services.users_get(tenant.tenant_id(), &user_id) {
        Some(u) => u,
        None => return errors::json_error(StatusCode::NOT_FOUND, "not_found", "user not found"),
    };

    // Get effective permissions for the user
    let effective = match services.users_effective_permissions(
        tenant.tenant_id(),
        &user_id,
        default_role_permissions,
    ) {
        Some(e) => e,
        None => return errors::json_error(StatusCode::NOT_FOUND, "not_found", "user not found"),
    };

    // Build principal from user's effective permissions
    let membership = TenantMembership {
        tenant_id: tenant.tenant_id(),
        roles: effective.roles.iter().map(|r| forgeerp_auth::Role::new(r.clone())).collect(),
        permissions: effective.permissions.iter().map(|p| Permission::new(p.clone())).collect(),
    };

    let principal_obj = Principal {
        principal_id: forgeerp_auth::PrincipalId::from_uuid(*user.user_id.as_uuid()),
        active_tenant_id: tenant.tenant_id(),
        membership,
    };

    let required_perm = Permission::new(query.permission);
    let explanation = explain_authorization(&principal_obj, &required_perm, default_role_permissions);

    (StatusCode::OK, Json(serde_json::json!({
        "user_id": user_id.to_string(),
        "user_email": user.email,
        "explanation": explanation,
    }))).into_response()
}

