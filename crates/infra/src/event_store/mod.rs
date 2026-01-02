//! Append-only event store boundary.
//!
//! This module defines an infrastructure-facing abstraction for storing and
//! loading tenant-scoped event streams without making any storage assumptions.

pub mod in_memory;
pub mod r#trait;

pub use in_memory::InMemoryEventStore;
pub use r#trait::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};

/// Adapter that publishes committed events to an `EventBus` after a successful append.
///
/// This ensures the ordering invariant: **publish happens only after append succeeds**.
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


