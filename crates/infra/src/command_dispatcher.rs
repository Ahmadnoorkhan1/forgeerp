//! Command execution pipeline (application-level orchestration).
//!
//! Flow:
//! Command → Load events → Rehydrate aggregate → Decide → Persist → Publish
//!
//! This module contains no IO itself; it composes infrastructure traits.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use forgeerp_core::{Aggregate, AggregateId, DomainError, ExpectedVersion, TenantId};
use forgeerp_events::{EventBus, EventEnvelope};

use crate::event_store::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};

#[derive(Debug)]
pub enum DispatchError {
    /// Optimistic concurrency failure (e.g. stale aggregate version).
    Concurrency(String),
    /// Tenant isolation violation (cross-tenant or cross-aggregate stream mixing).
    TenantIsolation(String),
    /// Domain validation failure (deterministic).
    Validation(String),
    /// Domain invariant failure (deterministic).
    InvariantViolation(String),
    /// Domain authorization failure.
    Unauthorized,
    /// Domain-level not found.
    NotFound,
    /// Failed to deserialize historical event payloads into the aggregate event type.
    Deserialize(String),
    /// Persisting to the event store failed.
    Store(EventStoreError),
    /// Publication failed after a successful append (at-least-once; retry may duplicate).
    Publish(String),
}

impl From<EventStoreError> for DispatchError {
    fn from(value: EventStoreError) -> Self {
        match &value {
            EventStoreError::Concurrency(msg) => DispatchError::Concurrency(msg.clone()),
            EventStoreError::TenantIsolation(msg) => DispatchError::TenantIsolation(msg.clone()),
            _ => DispatchError::Store(value),
        }
    }
}

impl From<DomainError> for DispatchError {
    fn from(value: DomainError) -> Self {
        match value {
            DomainError::Validation(msg) => DispatchError::Validation(msg),
            DomainError::InvariantViolation(msg) => DispatchError::InvariantViolation(msg),
            DomainError::Conflict(msg) => DispatchError::Concurrency(msg),
            DomainError::Unauthorized => DispatchError::Unauthorized,
            DomainError::NotFound => DispatchError::NotFound,
            DomainError::InvalidId(msg) => DispatchError::Validation(msg),
        }
    }
}

/// Reusable command execution engine.
///
/// Notes:
/// - Aggregates must be deterministic and side-effect free.
/// - Events are appended first; publication happens only after successful append.
/// - Publication failures are surfaced as errors and may be retried (at-least-once).
#[derive(Debug)]
pub struct CommandDispatcher<S, B> {
    store: S,
    bus: B,
}

impl<S, B> CommandDispatcher<S, B> {
    pub fn new(store: S, bus: B) -> Self {
        Self { store, bus }
    }

    pub fn into_parts(self) -> (S, B) {
        (self.store, self.bus)
    }
}

impl<S, B> CommandDispatcher<S, B>
where
    S: EventStore,
    B: EventBus<EventEnvelope<JsonValue>>,
{
    /// Dispatch a command through the full event-sourcing pipeline.
    ///
    /// - `make_aggregate` must create an aggregate instance for the given tenant/id.
    /// - Historical events are deserialized into `A::Event` and applied in order.
    ///
    /// Returns the committed stored events (with sequence numbers).
    pub fn dispatch<A>(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: impl Into<String>,
        command: A::Command,
        make_aggregate: impl FnOnce(TenantId, AggregateId) -> A,
    ) -> Result<Vec<StoredEvent>, DispatchError>
    where
        A: Aggregate<Error = DomainError>,
        A::Event: forgeerp_events::Event + Serialize + DeserializeOwned,
    {
        // 1) Load history (tenant-scoped)
        let history = self.store.load_stream(tenant_id, aggregate_id)?;
        validate_loaded_stream(tenant_id, aggregate_id, &history)?;
        let expected = ExpectedVersion::Exact(stream_version(&history));

        // 2) Rehydrate aggregate
        let mut aggregate = make_aggregate(tenant_id, aggregate_id);
        apply_history::<A>(&mut aggregate, &history)?;

        // 3) Decide events (no mutation)
        let decided = aggregate.handle(&command).map_err(DispatchError::from)?;
        if decided.is_empty() {
            return Ok(vec![]);
        }

        // 4) Persist (append-only, optimistic)
        let aggregate_type = aggregate_type.into();
        let uncommitted = decided
            .iter()
            .map(|ev| {
                UncommittedEvent::from_typed(
                    tenant_id,
                    aggregate_id,
                    aggregate_type.clone(),
                    Uuid::now_v7(),
                    ev,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let committed = self.store.append(uncommitted, expected)?;

        // 5) Publish committed events (after append)
        for stored in &committed {
            self.bus
                .publish(stored.to_envelope())
                .map_err(|e| DispatchError::Publish(format!("{e:?}")))?;
        }

        Ok(committed)
    }
}

fn stream_version(stream: &[StoredEvent]) -> u64 {
    stream.last().map(|e| e.sequence_number).unwrap_or(0)
}

fn validate_loaded_stream(
    tenant_id: TenantId,
    aggregate_id: AggregateId,
    stream: &[StoredEvent],
) -> Result<(), DispatchError> {
    // Enforce tenant isolation even if a buggy backend returns cross-tenant data.
    // Also ensure the stream is monotonically increasing by sequence number.
    let mut last = 0u64;
    for (idx, e) in stream.iter().enumerate() {
        if e.tenant_id != tenant_id {
            return Err(DispatchError::TenantIsolation(format!(
                "loaded stream contains wrong tenant_id at index {idx}"
            )));
        }
        if e.aggregate_id != aggregate_id {
            return Err(DispatchError::TenantIsolation(format!(
                "loaded stream contains wrong aggregate_id at index {idx}"
            )));
        }
        if e.sequence_number == 0 {
            return Err(DispatchError::Store(EventStoreError::InvalidAppend(
                "stored event has sequence_number=0".to_string(),
            )));
        }
        if e.sequence_number <= last {
            return Err(DispatchError::Store(EventStoreError::InvalidAppend(format!(
                "non-monotonic sequence_number in loaded stream (last={last}, found={})",
                e.sequence_number
            ))));
        }
        last = e.sequence_number;
    }
    Ok(())
}

fn apply_history<A>(aggregate: &mut A, history: &[StoredEvent]) -> Result<(), DispatchError>
where
    A: Aggregate,
    A::Event: DeserializeOwned,
{
    // Ensure deterministic ordering.
    let mut sorted = history.to_vec();
    sorted.sort_by_key(|e| e.sequence_number);

    for stored in sorted {
        let ev: A::Event = serde_json::from_value(stored.payload)
            .map_err(|e| DispatchError::Deserialize(e.to_string()))?;
        aggregate.apply(&ev);
    }

    Ok(())
}


