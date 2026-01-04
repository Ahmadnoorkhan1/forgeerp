//! Append-only event store boundary.
//!
//! This module defines an infrastructure-facing abstraction for storing and
//! loading tenant-scoped event streams without making any storage assumptions.

pub mod in_memory;
pub mod postgres;
pub mod r#trait;

pub use in_memory::InMemoryEventStore;
pub use postgres::{PostgresEventStore, Snapshot};
pub use r#trait::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};

/// Adapter that publishes committed events to an `EventBus` after a successful append.
///
/// `PublishingEventStore` is a composable adapter that wraps an `EventStore` and automatically
/// publishes committed events to an `EventBus`. This ensures the critical ordering invariant:
/// **publish happens only after append succeeds**.
///
/// ## Ordering Guarantee
///
/// Events are published **after** they're successfully persisted to the event store. This ensures:
/// - **Durability**: Events are persisted before distribution (never lose events)
/// - **Consistency**: Consumers only receive events that were successfully stored
/// - **Recovery**: If publication fails, events are still in the store and can be republished
///
/// ## Usage Pattern
///
/// This adapter is useful when you want to combine event storage and publication in one step:
///
/// ```ignore
/// let store = InMemoryEventStore::new();
/// let bus = InMemoryEventBus::new();
/// let publishing_store = PublishingEventStore::new(store, bus);
///
/// // append() both stores and publishes events
/// let committed = publishing_store.append(events, expected_version)?;
/// ```
///
/// ## Error Handling
///
/// If publication fails after a successful append, `append()` returns `EventStoreError::Publish`.
/// The events are already persisted, so:
/// - Retrying `append()` is idempotent (events already stored, will fail on version check)
/// - Caller can retry just the publication step (events are in store)
/// - This gives at-least-once delivery semantics
///
/// ## When to Use
///
/// - **Simple workflows**: When you want store + publish in one operation
/// - **Testing**: Simpler than managing store and bus separately
/// - **Synchronous patterns**: When you want immediate publication after append
///
/// For more control (e.g., separate error handling for store vs bus), use `EventStore` and
/// `EventBus` separately (as `CommandDispatcher` does).
pub struct PublishingEventStore<S, B> {
    store: S,
    bus: B,
}

impl<S, B> PublishingEventStore<S, B> {
    pub fn new(store: S, bus: B) -> Self {
        Self { store, bus }
    }

    pub fn into_parts(self) -> (S, B) {
        (self.store, self.bus)
    }
}

impl<S, B> EventStore for PublishingEventStore<S, B>
where
    S: EventStore,
    B: forgeerp_events::EventBus<forgeerp_events::EventEnvelope<serde_json::Value>>,
{
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: forgeerp_core::ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        // 1) Append (durable step)
        let committed = self.store.append(events, expected_version)?;

        // 2) Publish committed events (best-effort; at-least-once acceptable)
        for e in &committed {
            self.bus
                .publish(e.to_envelope())
                .map_err(|err| EventStoreError::Publish(format!("{err:?}")))?;
        }

        Ok(committed)
    }

    fn load_stream(
        &self,
        tenant_id: forgeerp_core::TenantId,
        aggregate_id: forgeerp_core::AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        self.store.load_stream(tenant_id, aggregate_id)
    }
}


