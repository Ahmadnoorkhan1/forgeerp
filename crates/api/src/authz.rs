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

/// Map roles to their granted permissions using the default role-permission mapping.
///
/// This function uses the default role-to-permission mapping from the infra projections.
pub fn permissions_from_roles(roles: &[forgeerp_auth::Role]) -> Vec<Permission> {
    use forgeerp_infra::projections::default_role_permissions;
    use std::collections::HashSet;

    let mut all_permissions: HashSet<String> = HashSet::new();

    for role in roles {
        let role_perms = default_role_permissions(role.as_str());
        for perm in role_perms {
            all_permissions.insert(perm);
        }
    }

    all_permissions.into_iter().map(Permission::new).collect()
}


