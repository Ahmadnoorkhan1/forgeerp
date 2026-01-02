//! API-side authorization guard for commands.
//!
//! This enforces authorization at the command boundary (before dispatch),
//! while keeping domain aggregates and infra auth-agnostic.

use forgeerp_auth::{AuthzError, CommandAuthorization, Permission, Principal, TenantMembership, authorize};

use crate::context::{PrincipalContext, TenantContext};

/// Check authorization for a command in the current request context.
///
/// This is intended to be called **before** dispatching a command.
pub fn authorize_command<C: CommandAuthorization>(
    tenant: &TenantContext,
    principal: &PrincipalContext,
    command: &C,
) -> Result<(), AuthzError> {
    let membership = TenantMembership {
        tenant_id: tenant.tenant_id(),
        roles: principal.roles().to_vec(),
        permissions: permissions_from_roles(principal.roles()),
    };

    let principal = Principal {
        principal_id: principal.principal_id(),
        active_tenant_id: tenant.tenant_id(),
        membership,
    };

    for perm in command.required_permissions() {
        authorize(&principal, perm)?;
    }

    Ok(())
}

/// Minimal role→permission mapping stub.
///
/// This is intentionally simple until a real policy source exists (e.g. DB-backed).
fn permissions_from_roles(roles: &[forgeerp_auth::Role]) -> Vec<Permission> {
    // Convention: "admin" grants all permissions in the current tenant.
    if roles.iter().any(|r| r.as_str() == "admin") {
        return vec![Permission::new("*")];
    }

    Vec::new()
}


