use forgeerp_auth::{PrincipalId, Role};
use forgeerp_core::TenantId;

/// Tenant context for a request.
///
/// This is immutable and must be present for all domain routes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TenantContext {
    tenant_id: TenantId,
}

impl TenantContext {
    pub fn new(tenant_id: TenantId) -> Self {
        Self { tenant_id }
    }

    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
}

/// Principal context for a request (authenticated identity + roles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalContext {
    principal_id: PrincipalId,
    roles: Vec<Role>,
}

impl PrincipalContext {
    pub fn new(principal_id: PrincipalId, roles: Vec<Role>) -> Self {
        Self { principal_id, roles }
    }

    pub fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }

    pub fn roles(&self) -> &[Role] {
        &self.roles
    }
}


