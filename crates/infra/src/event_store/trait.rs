use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;
use uuid::Uuid;

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};
use std::sync::Arc;

/// An event ready to be appended to a stream (not yet assigned a sequence number).
///
/// `UncommittedEvent` represents an event that's ready to be persisted but hasn't been
/// assigned a sequence number yet. The event store assigns sequence numbers during append.
///
/// ## Event Lifecycle
///
/// Events go through this lifecycle:
///
/// 1. **Domain event**: Created by aggregate's `handle()` method
/// 2. **UncommittedEvent**: Wrapped with metadata (tenant_id, aggregate_id, etc.)
/// 3. **StoredEvent**: Persisted with assigned sequence_number
/// 4. **EventEnvelope**: Published to event bus for consumers
///
/// ## Construction
///
/// Use `UncommittedEvent::from_typed()` to create an uncommitted event from a typed
/// domain event. This method:
/// - Serializes the event to JSON (payload)
/// - Extracts event metadata (event_type, version, occurred_at)
/// - Wraps it with stream metadata (tenant_id, aggregate_id, aggregate_type)
///
/// Domain modules can build this from their typed events using serde, while preserving
/// the event metadata needed for deserialization.
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
///
/// `StoredEvent` represents an event that has been **persisted** to the event store and
/// assigned a sequence number. This is what you get back from `EventStore::append()`.
///
/// ## Sequence Numbers
///
/// Sequence numbers are assigned by the event store during append and are:
/// - **Monotonically increasing**: Each event gets the next sequence number (last + 1)
/// - **Stream-scoped**: Sequence numbers are per-stream (tenant_id + aggregate_id)
/// - **Immutable**: Once assigned, sequence numbers never change
///
/// Sequence numbers enable:
/// - **Ordering**: Events are processed in sequence number order
/// - **Optimistic concurrency**: Version checking uses sequence numbers
/// - **Idempotency**: Duplicate events have the same sequence number
///
/// ## Conversion to Envelope
///
/// Use `to_envelope()` to convert a `StoredEvent` to an `EventEnvelope` for publication
/// to the event bus. This creates a copy with the same metadata, suitable for distribution.
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

/// Event store operation error.
///
/// This enum represents errors that can occur when interacting with the event store.
/// These are **infrastructure errors** (storage, concurrency, isolation) as opposed to
/// domain errors (validation, invariants).
///
/// ## Error Categories
///
/// - **Concurrency**: Optimistic concurrency check failed (version mismatch)
/// - **TenantIsolation**: Cross-tenant access attempted (security violation)
/// - **AggregateTypeMismatch**: Event type doesn't match stream's aggregate type
/// - **InvalidAppend**: Invalid event data or stream state
/// - **Publish**: Event publication failed (after successful append)
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
/// The `EventStore` is the **persistence layer** for events. It provides an append-only
/// interface where events are stored in streams (one stream per aggregate instance).
///
/// ## Design Principles
///
/// - **No storage assumptions**: Works with in-memory implementations (tests/dev) and
///   future SQL/NoSQL backends (production)
/// - **Tenant isolation**: Enforced on both read and write operations (security boundary)
/// - **Optimistic locking**: Via `ExpectedVersion` (no pessimistic locks, high performance)
/// - **Append-only**: Events cannot be modified or deleted (immutability guarantee)
///
/// ## Event Streams
///
/// Events are organized into **streams**, where each stream corresponds to one aggregate
/// instance. The stream key is `(tenant_id, aggregate_id)`. Within a stream, events have
/// monotonically increasing sequence numbers (1, 2, 3, ...).
///
/// ## Append Semantics
///
/// `append()`:
/// - Validates tenant isolation (all events must belong to the same tenant)
/// - Validates aggregate scoping (all events must target the same aggregate)
/// - Checks optimistic concurrency (version must match expected)
/// - Assigns sequence numbers (starting at current_version + 1)
/// - Persists events atomically (all or nothing)
///
/// ## Load Semantics
///
/// `load_stream()`:
/// - Returns all events for the specified tenant + aggregate
/// - Events are returned in sequence number order
/// - Returns empty vector if stream doesn't exist (aggregate not yet created)
/// - Enforces tenant isolation (cannot load events from different tenant)
///
/// ## Implementation Requirements
///
/// Implementations must:
/// - Enforce tenant isolation (reject cross-tenant operations)
/// - Enforce optimistic concurrency (check version before append)
/// - Assign sequence numbers monotonically (no gaps, no duplicates)
/// - Ensure atomicity (all events in a batch are persisted or none are)
/// - Handle concurrent appends correctly (optimistic locking)
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


