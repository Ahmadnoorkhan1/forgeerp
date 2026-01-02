use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_core::{AggregateId, TenantId};

/// Envelope for an event, containing multi-tenant + stream metadata.
///
/// This is the unit you persist/append to an event stream.
///
/// Notes:
/// - **Multi-tenancy** is enforced here via `tenant_id`.
/// - **Append-only**: `sequence_number` is intended to be monotonically increasing per stream.
/// - `payload` is the domain-agnostic event payload.
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


