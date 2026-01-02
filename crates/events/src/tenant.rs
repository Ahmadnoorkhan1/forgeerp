use forgeerp_core::TenantId;

use crate::EventEnvelope;

/// Helper trait for tenant-scoped messages.
///
/// Used by infrastructure workers to enforce tenant-safe initialization.
pub trait TenantScoped {
    fn tenant_id(&self) -> TenantId;
}

impl<E> TenantScoped for EventEnvelope<E> {
    fn tenant_id(&self) -> TenantId {
        self.tenant_id()
    }
}


