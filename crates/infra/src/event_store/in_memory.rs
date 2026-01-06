use std::collections::HashMap;
use std::sync::RwLock;

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};

use super::query::{EventFilter, EventQuery, EventQueryResult, Pagination};
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

#[async_trait::async_trait]
impl EventQuery for InMemoryEventStore {
    async fn query_events(
        &self,
        tenant_id: TenantId,
        filter: EventFilter,
        pagination: Pagination,
    ) -> Result<EventQueryResult, EventStoreError> {
        // Use spawn_blocking since we're accessing RwLock (blocking operation)
        let streams = {
            let guard = self
                .streams
                .read()
                .map_err(|_| EventStoreError::InvalidAppend("lock poisoned".to_string()))?;
            guard.clone()
        };

        // Collect all events for the tenant
        let mut all_events: Vec<StoredEvent> = Vec::new();
        for (key, stream) in streams.iter() {
            if key.tenant_id == tenant_id {
                all_events.extend(stream.iter().cloned());
            }
        }

        // Apply filters
        let mut filtered: Vec<StoredEvent> = all_events
            .into_iter()
            .filter(|e| {
                if let Some(agg_id) = filter.aggregate_id {
                    if e.aggregate_id != agg_id {
                        return false;
                    }
                }
                if let Some(ref agg_type) = filter.aggregate_type {
                    if e.aggregate_type != *agg_type {
                        return false;
                    }
                }
                if let Some(ref evt_type) = filter.event_type {
                    if e.event_type != *evt_type {
                        return false;
                    }
                }
                if let Some(after) = filter.occurred_after {
                    if e.occurred_at < after {
                        return false;
                    }
                }
                if let Some(before) = filter.occurred_before {
                    if e.occurred_at > before {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Sort by occurred_at (descending), then sequence_number (ascending)
        filtered.sort_by(|a, b| {
            match b.occurred_at.cmp(&a.occurred_at) {
                std::cmp::Ordering::Equal => a.sequence_number.cmp(&b.sequence_number),
                other => other,
            }
        });

        let total = filtered.len() as u64;

        // Apply pagination
        let start = pagination.offset as usize;
        let paginated = filtered.into_iter().skip(start).take(pagination.limit as usize).collect();

        let has_more = total > (pagination.offset + pagination.limit) as u64;

        Ok(EventQueryResult {
            events: paginated,
            total,
            pagination,
            has_more,
        })
    }

    async fn get_aggregate_events(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        pagination: Option<Pagination>,
    ) -> Result<EventQueryResult, EventStoreError> {
        // For aggregate streams, use load_stream (sequence order) instead of query_events (time order)
        let all_events = self.load_stream(tenant_id, aggregate_id)?;

        let total = all_events.len() as u64;
        let pagination = pagination.unwrap_or_default();

        let start = pagination.offset as usize;
        let paginated: Vec<StoredEvent> = all_events
            .into_iter()
            .skip(start)
            .take(pagination.limit as usize)
            .collect();

        let has_more = total > (pagination.offset + pagination.limit) as u64;

        Ok(EventQueryResult {
            events: paginated,
            total,
            pagination,
            has_more,
        })
    }

    async fn get_event_by_id(
        &self,
        tenant_id: TenantId,
        event_id: uuid::Uuid,
    ) -> Result<Option<StoredEvent>, EventStoreError> {
        let streams = {
            let guard = self
                .streams
                .read()
                .map_err(|_| EventStoreError::InvalidAppend("lock poisoned".to_string()))?;
            guard.clone()
        };
        
        for (key, stream) in streams.iter() {
            if key.tenant_id == tenant_id {
                if let Some(event) = stream.iter().find(|e| e.event_id == event_id) {
                    return Ok(Some(event.clone()));
                }
            }
        }
        
        Ok(None)
    }
}


