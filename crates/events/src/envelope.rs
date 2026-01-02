use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_core::{AggregateId, TenantId};

/// Envelope for an event, containing multi-tenant + stream metadata.
///
/// An `EventEnvelope` wraps a domain event with infrastructure metadata needed for
/// event sourcing in a multi-tenant system. This is the **unit of persistence** - what
/// you actually store in the event store and publish to the event bus.
///
/// ## Why Envelopes?
///
/// Envelopes separate **infrastructure concerns** (tenant isolation, ordering, identity)
/// from **domain concerns** (business events). This enables:
///
/// - **Multi-tenancy**: Tenant ID is enforced at the envelope level, making it impossible
///   to accidentally mix events from different tenants
/// - **Event ordering**: Sequence numbers enable deterministic replay and detect duplicates
/// - **Stream management**: Aggregate type + ID enable efficient querying and partitioning
/// - **Domain purity**: Domain events remain infrastructure-agnostic
///
/// ## Architecture Pattern
///
/// The envelope pattern follows the principle of **separation of concerns**:
///
/// ```text
/// Domain Event (InventoryEvent::StockAdjusted)
///     ↓
/// EventEnvelope { tenant_id, aggregate_id, sequence_number, payload: event }
///     ↓
/// Event Store / Event Bus (infrastructure layer)
/// ```
///
/// The domain defines events; the infrastructure wraps them in envelopes for storage/transport.
///
/// ## Multi-Tenancy Enforcement
///
/// `tenant_id` is the primary isolation boundary. Every envelope includes a tenant ID, and
/// the infrastructure layer validates that:
/// - Events in the same stream belong to the same tenant
/// - Queries filter by tenant ID
/// - Projections process events per-tenant
///
/// This is enforced at multiple layers (event store, command dispatcher, projections) as
/// defense in depth.
///
/// ## Sequence Numbers
///
/// `sequence_number` provides:
/// - **Ordering**: Events are processed in sequence number order
/// - **Idempotency**: Duplicate events (same sequence number) can be detected and skipped
/// - **Optimistic concurrency**: Version checking prevents concurrent modifications
///
/// Sequence numbers are **monotonically increasing** per stream (tenant_id + aggregate_id).
/// They start at 1 (0 is invalid) and increment by 1 for each event.
///
/// ## Generic Payload
///
/// The `E` type parameter allows envelopes to carry different payload types:
/// - `EventEnvelope<serde_json::Value>`: For storage/transport (JSON serialization)
/// - `EventEnvelope<InventoryEvent>`: For typed domain events (if you need type safety)
///
/// Infrastructure typically uses JSON for flexibility (schema evolution), while domain code
/// works with strongly-typed event enums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<E> {
    event_id: Uuid,
    tenant_id: TenantId,

    aggregate_id: AggregateId,
    aggregate_type: String,

    /// Monotonically increasing position in the aggregate stream.
    sequence_number: u64,

    payload: E,
}

impl<E> EventEnvelope<E> {
    pub fn new(
        event_id: Uuid,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: impl Into<String>,
        sequence_number: u64,
        payload: E,
    ) -> Self {
        Self {
            event_id,
            tenant_id,
            aggregate_id,
            aggregate_type: aggregate_type.into(),
            sequence_number,
            payload,
        }
    }

    pub fn event_id(&self) -> Uuid {
        self.event_id
    }

    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    pub fn aggregate_id(&self) -> AggregateId {
        self.aggregate_id
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    pub fn payload(&self) -> &E {
        &self.payload
    }

    pub fn into_payload(self) -> E {
        self.payload
    }
}


