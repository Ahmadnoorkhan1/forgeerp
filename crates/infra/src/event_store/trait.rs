use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;
use uuid::Uuid;

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};
use std::sync::Arc;

/// An event ready to be appended to a stream (not yet assigned a sequence number).
///
/// Domain modules can build this from their typed events using serde, while
/// preserving the event metadata needed for deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncommittedEvent {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub aggregate_id: AggregateId,
    pub aggregate_type: String,

    pub event_type: String,
    pub event_version: u32,
    pub occurred_at: DateTime<Utc>,

    pub payload: JsonValue,
}

/// A stored event in an append-only stream (assigned a sequence number).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_id: Uuid,
    pub tenant_id: TenantId,
    pub aggregate_id: AggregateId,
    pub aggregate_type: String,

    /// Monotonically increasing position in the aggregate stream.
    pub sequence_number: u64,

    pub event_type: String,
    pub event_version: u32,
    pub occurred_at: DateTime<Utc>,

    pub payload: JsonValue,
}

impl StoredEvent {
    pub fn stream_version(&self) -> u64 {
        self.sequence_number
    }

    /// Convert a stored event into a tenant-scoped event envelope for publication.
    pub fn to_envelope(&self) -> forgeerp_events::EventEnvelope<JsonValue> {
        forgeerp_events::EventEnvelope::new(
            self.event_id,
            self.tenant_id,
            self.aggregate_id,
            self.aggregate_type.clone(),
            self.sequence_number,
            self.payload.clone(),
        )
    }
}

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("optimistic concurrency check failed: {0}")]
    Concurrency(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("aggregate type mismatch: {0}")]
    AggregateTypeMismatch(String),

    #[error("invalid append: {0}")]
    InvalidAppend(String),

    #[error("event publication failed: {0}")]
    Publish(String),
}

/// Append-only, tenant-scoped event store.
///
/// - **No storage assumptions** (works for in-memory tests/dev and future SQL backends)
/// - **Tenant isolation** enforced on read and write
/// - **Optimistic locking** via `ExpectedVersion`
pub trait EventStore: Send + Sync {
    /// Append events to an aggregate stream (append-only).
    ///
    /// Implementations must:
    /// - enforce tenant isolation
    /// - enforce optimistic concurrency against the current stream version
    /// - assign monotonically increasing `sequence_number`s starting at `current_version + 1`
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError>;

    /// Load the full stream for a tenant + aggregate.
    fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError>;
}

impl<S> EventStore for Arc<S>
where
    S: EventStore + ?Sized,
{
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        (**self).append(events, expected_version)
    }

    fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        (**self).load_stream(tenant_id, aggregate_id)
    }
}

impl UncommittedEvent {
    /// Convenience constructor from a typed envelope payload.
    ///
    /// Keeps infra decoupled from business, while still capturing event metadata
    /// needed for future deserialization.
    pub fn from_typed<E>(
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: impl Into<String>,
        event_id: Uuid,
        event: &E,
    ) -> Result<Self, EventStoreError>
    where
        E: forgeerp_events::Event + Serialize,
    {
        let payload = serde_json::to_value(event)
            .map_err(|e| EventStoreError::InvalidAppend(format!("payload serialization failed: {e}")))?;

        Ok(Self {
            event_id,
            tenant_id,
            aggregate_id,
            aggregate_type: aggregate_type.into(),
            event_type: event.event_type().to_string(),
            event_version: event.version(),
            occurred_at: event.occurred_at(),
            payload,
        })
    }
}


