//! Saga infrastructure: persistence and command execution.

pub mod sales_ar;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::Saga;
use serde_json::Value as JsonValue;

use crate::event_store::{EventStore, StoredEvent, UncommittedEvent};

/// Repository for persisting saga events via the event store.
pub struct SagaRepository<S: Saga, E: EventStore> {
    event_store: E,
    _phantom: std::marker::PhantomData<S>,
}

impl<S: Saga, E: EventStore> SagaRepository<S, E> {
    pub fn new(event_store: E) -> Self {
        Self {
            event_store,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Load saga event history for a saga instance.
    pub fn load(&self, tenant_id: TenantId, saga_id: AggregateId) -> Result<Vec<StoredEvent>, crate::event_store::EventStoreError> {
        self.event_store.load_stream(tenant_id, saga_id)
    }

    /// Append a saga event (Emit action).
    pub fn append_emit(
        &self,
        tenant_id: TenantId,
        saga_id: AggregateId,
        event_type: &str,
        payload: JsonValue,
    ) -> Result<Vec<StoredEvent>, crate::event_store::EventStoreError> {
        let uncommitted = UncommittedEvent {
            tenant_id,
            aggregate_id: saga_id,
            aggregate_type: S::saga_type().to_string(),
            event_id: uuid::Uuid::now_v7(),
            event_type: event_type.to_string(),
            event_version: 1,
            payload,
            occurred_at: chrono::Utc::now(),
        };
        self.event_store.append(vec![uncommitted], forgeerp_core::ExpectedVersion::Any)
    }
}

/// Command executor trait for saga actions.
pub trait CommandExecutor: Send + Sync {
    type Error: std::fmt::Debug;

    fn execute(
        &self,
        tenant_id: TenantId,
        aggregate_type: &str,
        command_type: &str,
        payload: &JsonValue,
    ) -> Result<(), Self::Error>;
}
