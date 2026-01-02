use std::collections::HashMap;
use std::sync::RwLock;

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};

use super::r#trait::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct StreamKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

/// In-memory append-only event store.
///
/// Intended for tests/dev. Not optimized for performance.
#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    streams: RwLock<HashMap<StreamKey, Vec<StoredEvent>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_version(stream: &[StoredEvent]) -> u64 {
        stream.last().map(|e| e.sequence_number).unwrap_or(0)
    }
}

impl EventStore for InMemoryEventStore {
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        // All events must target the same tenant + aggregate stream.
        let tenant_id = events[0].tenant_id;
        let aggregate_id = events[0].aggregate_id;
        let aggregate_type = events[0].aggregate_type.clone();

        for (idx, e) in events.iter().enumerate() {
            if e.tenant_id != tenant_id {
                return Err(EventStoreError::TenantIsolation(format!(
                    "batch contains multiple tenant_ids (index {idx})"
                )));
            }
            if e.aggregate_id != aggregate_id {
                return Err(EventStoreError::InvalidAppend(format!(
                    "batch contains multiple aggregate_ids (index {idx})"
                )));
            }
            if e.aggregate_type != aggregate_type {
                return Err(EventStoreError::AggregateTypeMismatch(format!(
                    "batch contains multiple aggregate_types (index {idx})"
                )));
            }
        }

        let key = StreamKey {
            tenant_id,
            aggregate_id,
        };

        let mut streams = self
            .streams
            .write()
            .map_err(|_| EventStoreError::InvalidAppend("lock poisoned".to_string()))?;

        let stream = streams.entry(key).or_default();
        let current = Self::current_version(stream);

        if !expected_version.matches(current) {
            return Err(EventStoreError::Concurrency(format!(
                "expected {expected_version:?}, found {current}"
            )));
        }

        // Enforce aggregate type stability across the stream.
        if let Some(existing) = stream.first() {
            if existing.aggregate_type != aggregate_type {
                return Err(EventStoreError::AggregateTypeMismatch(format!(
                    "stream aggregate_type is '{}', attempted append with '{}'",
                    existing.aggregate_type, aggregate_type
                )));
            }
        }

        // Assign sequence numbers and append (append-only).
        let mut next = current + 1;
        let mut committed = Vec::with_capacity(events.len());
        for e in events {
            let stored = StoredEvent {
                event_id: e.event_id,
                tenant_id: e.tenant_id,
                aggregate_id: e.aggregate_id,
                aggregate_type: e.aggregate_type,
                sequence_number: next,
                event_type: e.event_type,
                event_version: e.event_version,
                occurred_at: e.occurred_at,
                payload: e.payload,
            };
            next += 1;
            stream.push(stored.clone());
            committed.push(stored);
        }

        Ok(committed)
    }

    fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        let key = StreamKey {
            tenant_id,
            aggregate_id,
        };

        let streams = self
            .streams
            .read()
            .map_err(|_| EventStoreError::InvalidAppend("lock poisoned".to_string()))?;

        Ok(streams.get(&key).cloned().unwrap_or_default())
    }
}


