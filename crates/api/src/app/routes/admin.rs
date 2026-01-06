//! Admin routes for identity management.
//!
//! These endpoints provide tenant-scoped user administration with strict
//! privilege escalation prevention.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use forgeerp_auth::{
    admin, ActivateUser, AssignRole, CreateUser, Permission, RevokeRole, Role, SuspendUser,
    User, UserCommand, UserId,
};
use forgeerp_core::AggregateId;
use forgeerp_infra::projections::{default_role_permissions, UserReadModel};

use crate::app::{errors, services::AppServices};
use crate::app::routes::common::CmdAuth;
use crate::context::{PrincipalContext, TenantContext};

// ─────────────────────────────────────────────────────────────────────────────
// Request DTOs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub display_name: String,
    pub initial_roles: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct RevokeRoleRequest {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct SuspendUserRequest {
    pub reason: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new()
        .route("/users", post(create_user).get(list_users))
        .route("/users/:id", get(get_user))
        .route("/users/:id/roles", post(assign_role))
        .route("/users/:id/roles/:role", axum::routing::delete(revoke_role))
        .route("/users/:id/suspend", post(suspend_user))
        .route("/users/:id/activate", post(activate_user))
        .route("/users/:id/permissions", get(inspect_permissions))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// POST /admin/users - Create a new user
pub async fn create_user(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Json(body): Json<CreateUserRequest>,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let user_id = UserId::from(agg);

    let initial_roles: Vec<Role> = body
        .initial_roles
        .unwrap_or_default()
        .into_iter()
        .map(Role::new)
        .collect();

    // Privilege escalation check: actor cannot create users with roles they don't have
    // (unless actor is admin)
    let actor_is_admin = principal.roles().iter().any(|r| r.as_str() == "admin");
    if !actor_is_admin {
        for role in &initial_roles {
            let actor_has_role = principal.roles().iter().any(|r| r.as_str() == role.as_str());
            if !actor_has_role && role.as_str() != "user" {
                return errors::json_error(
                    StatusCode::FORBIDDEN,
                    "privilege_escalation",
                    format!("cannot assign role '{}' that you don't have", role.as_str()),
                );
            }
        }
    }

    let cmd = UserCommand::Create(CreateUser {
        tenant_id: tenant.tenant_id(),
        user_id,
        email: body.email,
        display_name: body.display_name,
        initial_roles,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![admin::USER_CREATE.clone()],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<User>(
        tenant.tenant_id(),
        agg,
        "auth.user",
        cmd_auth.inner,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
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

/// GET /admin/users - List all users in the tenant
pub async fn list_users(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_LIST.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let users = services.users_list(tenant.tenant_id());
    let items: Vec<serde_json::Value> = users.into_iter().map(user_to_json).collect();

    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}

/// GET /admin/users/:id - Get a specific user
pub async fn get_user(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };

    match services.users_get(tenant.tenant_id(), &user_id) {
        Some(user) => (StatusCode::OK, Json(user_to_json(user))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "user not found"),
    }
}

/// POST /admin/users/:id/roles - Assign a role to a user
pub async fn assign_role(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<AssignRoleRequest>,
) -> axum::response::Response {
    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };
    let agg = AggregateId::from_uuid(*user_id.as_uuid());

    let actor_roles: Vec<Role> = principal.roles().to_vec();

    let cmd = UserCommand::AssignRole(AssignRole {
        tenant_id: tenant.tenant_id(),
        user_id,
        role: Role::new(body.role),
        actor_roles, // Pass actor's roles for privilege escalation check
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![admin::USER_ASSIGN_ROLE.clone()],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<User>(
        tenant.tenant_id(),
        agg,
        "auth.user",
        cmd_auth.inner,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

/// DELETE /admin/users/:id/roles/:role - Revoke a role from a user
pub async fn revoke_role(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path((id, role)): Path<(String, String)>,
) -> axum::response::Response {
    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };
    let agg = AggregateId::from_uuid(*user_id.as_uuid());

    let cmd = UserCommand::RevokeRole(RevokeRole {
        tenant_id: tenant.tenant_id(),
        user_id,
        role: Role::new(role),
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![admin::USER_REVOKE_ROLE.clone()],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<User>(
        tenant.tenant_id(),
        agg,
        "auth.user",
        cmd_auth.inner,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

/// POST /admin/users/:id/suspend - Suspend a user
pub async fn suspend_user(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<SuspendUserRequest>,
) -> axum::response::Response {
    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };
    let agg = AggregateId::from_uuid(*user_id.as_uuid());

    let cmd = UserCommand::Suspend(SuspendUser {
        tenant_id: tenant.tenant_id(),
        user_id,
        reason: body.reason.unwrap_or_else(|| "No reason provided".to_string()),
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![admin::USER_SUSPEND.clone()],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<User>(
        tenant.tenant_id(),
        agg,
        "auth.user",
        cmd_auth.inner,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

/// POST /admin/users/:id/activate - Activate a suspended user
pub async fn activate_user(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };
    let agg = AggregateId::from_uuid(*user_id.as_uuid());

    let cmd = UserCommand::Activate(ActivateUser {
        tenant_id: tenant.tenant_id(),
        user_id,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![admin::USER_ACTIVATE.clone()],
    };

    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<User>(
        tenant.tenant_id(),
        agg,
        "auth.user",
        cmd_auth.inner,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

/// GET /admin/users/:id/permissions - Inspect effective permissions for a user
pub async fn inspect_permissions(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    // Check permission (same as USER_READ for now)
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let user_id: UserId = match id.parse::<uuid::Uuid>() {
        Ok(uuid) => UserId::from_uuid(uuid),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid user id"),
    };

    match services.users_effective_permissions(tenant.tenant_id(), &user_id, default_role_permissions) {
        Some(effective) => {
            let mut permissions: Vec<&str> = effective.permissions.iter().map(String::as_str).collect();
            permissions.sort();

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": effective.user_id.to_string(),
                    "tenant_id": effective.tenant_id.to_string(),
                    "roles": effective.roles,
                    "permissions": permissions,
                })),
            )
                .into_response()
        }
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "user not found"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn user_to_json(user: UserReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": user.user_id.to_string(),
        "tenant_id": user.tenant_id.to_string(),
        "email": user.email,
        "display_name": user.display_name,
        "roles": user.roles,
        "status": user.status,
        "created_at": user.created_at.to_rfc3339(),
        "updated_at": user.updated_at.to_rfc3339(),
    })
}

