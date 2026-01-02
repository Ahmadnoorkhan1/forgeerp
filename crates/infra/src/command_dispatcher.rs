//! Command execution pipeline (application-level orchestration).
//!
//! This module implements the **command dispatch pattern** for event-sourced aggregates.
//! It orchestrates the full lifecycle: loading history, rehydrating state, handling commands,
//! persisting events, and publishing to the event bus.
//!
//! ## Command Execution Flow
//!
//! The `CommandDispatcher` implements this pipeline:
//!
//! ```text
//! Command
//!   ↓
//! 1. Load events from store (tenant-scoped)
//!   ↓
//! 2. Rehydrate aggregate (apply historical events to rebuild state)
//!   ↓
//! 3. Handle command (pure decision logic, produces events)
//!   ↓
//! 4. Persist events to store (append-only, optimistic concurrency check)
//!   ↓
//! 5. Publish events to bus (for projections, handlers, etc.)
//! ```
//!
//! ## Why This Orchestration?
//!
//! This module exists to:
//!
//! - **Encapsulate complexity**: The command execution pattern is consistent across all aggregates,
//!   so we centralize it here rather than duplicating in every handler
//!
//! - **Enforce invariants**: Tenant isolation, optimistic concurrency, and event ordering are
//!   enforced here, preventing bugs in domain code
//!
//! - **Compose infrastructure**: The dispatcher composes `EventStore` and `EventBus` traits,
//!   making it testable with in-memory implementations and swappable with real backends
//!
//! - **Error handling**: Centralizes error mapping from domain errors, store errors, and bus
//!   errors into a consistent `DispatchError` enum
//!
//! ## Design Principles
//!
//! - **No IO assumptions**: Uses trait objects, works with any `EventStore` and `EventBus`
//! - **Deterministic**: No side effects except through the injected store/bus
//! - **Tenant-aware**: All operations are scoped to a tenant ID
//! - **Failure handling**: Maps domain errors, store errors, and bus errors consistently
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

/// Reusable command execution engine for event-sourced aggregates.
///
/// `CommandDispatcher` orchestrates the full event-sourcing pipeline: loading events,
/// rehydrating aggregates, handling commands, persisting events, and publishing to the bus.
///
/// ## Architecture Role
///
/// The dispatcher sits between the API layer (HTTP handlers) and the infrastructure layer
/// (event store, event bus). It provides a **consistent execution model** for all commands
/// while keeping domain code pure and testable.
///
/// ## Execution Guarantees
///
/// - **Atomicity**: Events are persisted before publication (if append fails, nothing is published)
/// - **Consistency**: Tenant isolation and optimistic concurrency are enforced
/// - **Isolation**: Each command operates on a single aggregate instance
/// - **Durability**: Events are persisted before returning (store's responsibility)
///
/// ## Error Semantics
///
/// - **Domain errors**: Validation failures, invariant violations → `DispatchError::Validation` / `InvariantViolation`
/// - **Concurrency errors**: Version mismatch → `DispatchError::Concurrency`
/// - **Tenant errors**: Cross-tenant access → `DispatchError::TenantIsolation`
/// - **Bus errors**: Publication failures → `DispatchError::Publish` (events are persisted, but publication failed)
///
/// ## At-Least-Once Delivery
///
/// If event publication fails after a successful append, the error is returned to the caller.
/// The events are already persisted, so retrying the command is idempotent (or the caller
/// can retry just the publication step). This gives **at-least-once** delivery semantics.
///
/// ## Generic Parameters
///
/// - `S`: Event store implementation (must implement `EventStore` trait)
/// - `B`: Event bus implementation (must implement `EventBus` trait)
///
/// This design enables:
/// - **Testability**: Use `InMemoryEventStore` and `InMemoryEventBus` in tests
/// - **Swappability**: Replace with Postgres event store, Redis bus, etc. without changing domain code
/// - **Composability**: Mix and match different store/bus implementations
///
/// ## Aggregate Requirements
///
/// Aggregates used with `CommandDispatcher` must be:
/// - **Deterministic**: Same events produce same state (required for replay)
/// - **Side-effect free**: No IO, no external state (pure functions only)
/// - **Version-aware**: Track version in `apply()` for optimistic concurrency
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
    /// This method implements the complete command execution lifecycle:
    ///
    /// 1. **Load**: Retrieves all events for the aggregate from the event store
    /// 2. **Validate**: Checks tenant isolation and event ordering (defense in depth)
    /// 3. **Rehydrate**: Applies historical events to rebuild the aggregate's current state
    /// 4. **Decide**: Calls `aggregate.handle(command)` to produce new events (pure, no mutation)
    /// 5. **Persist**: Appends events to the store with optimistic concurrency check
    /// 6. **Publish**: Publishes committed events to the event bus for downstream consumers
    ///
    /// ## Parameters
    ///
    /// - `tenant_id`: The tenant context (enforced throughout the pipeline)
    /// - `aggregate_id`: The aggregate instance identifier
    /// - `aggregate_type`: Type identifier for the aggregate (e.g., "inventory.item")
    /// - `command`: The domain command to execute
    /// - `make_aggregate`: Factory function to create a fresh aggregate instance
    ///
    /// ## Aggregate Factory Pattern
    ///
    /// The `make_aggregate` closure enables the dispatcher to work with any aggregate type
    /// without needing to know how to construct it. This keeps the dispatcher generic and
    /// allows domain code to control aggregate initialization (e.g., `InventoryItem::empty(id)`).
    ///
    /// ## Return Value
    ///
    /// Returns the committed `StoredEvent`s (with assigned sequence numbers) if successful.
    /// These can be used for:
    /// - Determining the new aggregate version
    /// - Triggering downstream processing
    /// - Idempotency checks (if needed)
    ///
    /// Returns `DispatchError` if any step fails (validation, concurrency, store error, etc.).
    ///
    /// ## Concurrency Safety
    ///
    /// This method uses **optimistic concurrency control**:
    /// - Loads the current version from the event stream
    /// - Expects that version when appending new events
    /// - If version changed (concurrent modification), append fails with `DispatchError::Concurrency`
    ///
    /// Callers should retry by reloading and re-executing the command (or surface a conflict error).
    ///
    /// ## Tenant Isolation
    ///
    /// Tenant ID is validated at multiple points:
    /// - Events are loaded scoped to `tenant_id`
    /// - Loaded events are validated to ensure they belong to the correct tenant
    /// - New events are created with the provided `tenant_id`
    ///
    /// This defense-in-depth approach prevents cross-tenant data leaks even if the store is buggy.
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


