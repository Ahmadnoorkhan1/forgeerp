use std::collections::HashSet;

use thiserror::Error;

use forgeerp_core::TenantId;

use crate::{Permission, PrincipalId, TenantMembership};

/// A fully resolved principal for authorization decisions.
///
/// Construction of this object is intentionally decoupled from storage and
/// transport: API/workers can derive memberships from claims and a policy source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub principal_id: PrincipalId,
    pub active_tenant_id: TenantId,
    pub membership: TenantMembership,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthzError {
    #[error("tenant mismatch")]
    TenantMismatch,

    #[error("forbidden: missing permission '{0}'")]
    Forbidden(String),
}

/// Authorize a principal within its active tenant context.
///
/// - No IO
/// - No panics
/// - No business logic (pure policy check)
pub fn authorize(principal: &Principal, required: &Permission) -> Result<(), AuthzError> {
    if principal.active_tenant_id != principal.membership.tenant_id {
        return Err(AuthzError::TenantMismatch);
    }

    let perms: HashSet<&str> = principal
        .membership
        .permissions
        .iter()
        .map(|p| p.as_str())
        .collect();

    if perms.contains(required.as_str()) {
        Ok(())
    } else {
        Err(AuthzError::Forbidden(required.as_str().to_string()))
    }
}


