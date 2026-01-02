use forgeerp_core::TenantId;

use crate::EventEnvelope;

/// Helper trait for tenant-scoped messages.
///
/// This trait marks types that have an associated tenant ID, enabling tenant-aware
/// processing in infrastructure components (workers, handlers, etc.).
///
/// ## Use Cases
///
/// - **Worker initialization**: Workers can be pinned to a specific tenant to ensure
///   they only process events for that tenant (defense in depth)
/// - **Message filtering**: Filter messages by tenant in subscription loops
/// - **Tenant validation**: Ensure messages belong to the expected tenant
///
/// ## Implementation
///
/// `EventEnvelope` implements this trait, allowing envelopes to be used with tenant-aware
/// infrastructure. Other message types can implement this trait if they need tenant scoping.
///
/// ## Example Usage
///
/// ```ignore
/// let worker = ProjectionWorker::new_for_tenant(
///     tenant_id,
///     projection,
///     event_bus.subscribe(),
/// );
/// // Worker will reject events from other tenants
/// ```
pub trait TenantScoped {
    fn tenant_id(&self) -> TenantId;
}

impl<E> TenantScoped for EventEnvelope<E> {
    fn tenant_id(&self) -> TenantId {
        self.tenant_id()
    }
}


