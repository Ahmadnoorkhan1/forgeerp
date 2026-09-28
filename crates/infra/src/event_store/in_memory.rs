//! In-memory, tenant-scoped event store for testing and local development.
//!
//! `InMemoryEventStore` stores events in-memory using `Arc<RwLock<>>` for thread-safe access.
//! It's designed for integration tests and local development where durability isn't required.
//!
//! ## Design
//!
//! - **Tenant isolation**: Enforced by organizing events by `(tenant_id, aggregate_id)`
//! - **Optimistic locking**: Version checks are performed before appending
//! - **Sequence numbers**: Assigned monotonically per stream (tenant_id + aggregate_id)
//! - **Atomicity**: All events in a batch are appended or none (no partial appends)
//! - **Thread-safe**: Uses `Arc<RwLock<>>` for concurrent read/write access
//!
//! ## Limitations
//!
//! - No persistence across restarts (data lost when application exits)
//! - Not suitable for production use (data loss risk)
//! - Memory grows unbounded (no cleanup/archival mechanisms)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};

use crate::event_store::{
    EventFilter, EventQuery, EventQueryResult, EventStoreError, Pagination, StoredEvent,
    UncommittedEvent,
};

/// In-memory event store implementation for testing and development.
///
/// Stores events in-memory organized by `(tenant_id, aggregate_id)` stream.
/// Each stream maintains events with monotonically increasing sequence numbers.
#[derive(Debug, Clone)]
pub struct InMemoryEventStore {
    /// Stores events organized by (tenant_id, aggregate_id) streams.
    ///
    /// Structure:
    /// - Outer key: TenantId (tenant isolation)
    /// - Middle key: AggregateId (per-aggregate streams)
    /// - Inner Vec: StoredEvent entries (ordered by sequence_number)
    inner: Arc<RwLock<HashMap<(TenantId, AggregateId), Vec<StoredEvent>>>>,
}

impl InMemoryEventStore {
    /// Create a new empty in-memory event store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the current version (sequence_number) of a stream.
    ///
    /// Returns 0 if the stream doesn't exist (first event will be sequence 1).
    fn get_stream_version(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<u64, EventStoreError> {
        let store = self.inner.read().map_err(|e| {
            EventStoreError::InvalidAppend(format!("lock acquisition failed: {}", e))
        })?;

        let current_version = store
            .get(&(tenant_id, aggregate_id))
            .map(|events| {
                events
                    .last()
                    .map(|e| e.sequence_number)
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        Ok(current_version)
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::event_store::EventStore for InMemoryEventStore {
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        if events.is_empty() {
            return Ok(Vec::new());
        }

        // Validate all events are for the same tenant and aggregate
        let first_tenant = events[0].tenant_id;
        let first_aggregate = events[0].aggregate_id;

        for event in &events {
            if event.tenant_id != first_tenant {
                return Err(EventStoreError::TenantIsolation(
                    "all events must belong to same tenant".to_string(),
                ));
            }
            if event.aggregate_id != first_aggregate {
                return Err(EventStoreError::InvalidAppend(
                    "all events must target same aggregate".to_string(),
                ));
            }
        }

        let mut store = self.inner.write().map_err(|e| {
            EventStoreError::InvalidAppend(format!("lock acquisition failed: {}", e))
        })?;

        let stream_key = (first_tenant, first_aggregate);

        // Get current version and validate optimistic concurrency
        let current_version = store
            .get(&stream_key)
            .map(|s| s.last().map(|e| e.sequence_number).unwrap_or(0))
            .unwrap_or(0);

        // Check optimistic concurrency
        match expected_version {
            ExpectedVersion::Any => {
                // No version check needed
            }
            ExpectedVersion::Exact(expected) => {
                if current_version != expected {
                    return Err(EventStoreError::Concurrency(format!(
                        "version mismatch: expected {}, got {}",
                        expected, current_version
                    )));
                }
            }
        }

        // Assign sequence numbers and create stored events
        let mut stored_events = Vec::with_capacity(events.len());
        for (i, event) in events.into_iter().enumerate() {
            let sequence_number = current_version + (i as u64) + 1;
            stored_events.push(StoredEvent {
                event_id: event.event_id,
                tenant_id: event.tenant_id,
                aggregate_id: event.aggregate_id,
                aggregate_type: event.aggregate_type,
                sequence_number,
                event_type: event.event_type,
                event_version: event.event_version,
                occurred_at: event.occurred_at,
                payload: event.payload,
            });
        }

        // Append to stream atomically
        store
            .entry(stream_key)
            .or_insert_with(Vec::new)
            .extend(stored_events.clone());

        Ok(stored_events)
    }

    fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        let store = self.inner.read().map_err(|e| {
            EventStoreError::InvalidAppend(format!("lock acquisition failed: {}", e))
        })?;

        let events = store
            .get(&(tenant_id, aggregate_id))
            .cloned()
            .unwrap_or_default();

        Ok(events)
    }
}

impl InMemoryEventStore {
    /// List all tenant IDs that have events in the store.
    pub fn list_tenants(&self) -> Vec<TenantId> {
        let store = match self.inner.read() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut tenants: Vec<TenantId> = store
            .keys()
            .map(|(tenant_id, _)| *tenant_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tenants.sort_by(|a, b| a.as_uuid().cmp(b.as_uuid()));
        tenants
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
        let store = self.inner.read().map_err(|e| {
            EventStoreError::InvalidAppend(format!("lock acquisition failed: {}", e))
        })?;

        // Collect all matching events for this tenant
        let mut matching_events: Vec<StoredEvent> = store
            .iter()
            .filter(|(key, _)| key.0 == tenant_id) // Tenant isolation
            .flat_map(|(_, events)| events.clone())
            .collect();

        // Apply filters
        if let Some(aggregate_id) = filter.aggregate_id {
            matching_events.retain(|e| e.aggregate_id == aggregate_id);
        }

        if let Some(aggregate_type) = filter.aggregate_type {
            matching_events.retain(|e| e.aggregate_type == aggregate_type);
        }

        if let Some(event_type) = filter.event_type {
            matching_events.retain(|e| e.event_type == event_type);
        }

        if let Some(occurred_after) = filter.occurred_after {
            matching_events.retain(|e| e.occurred_at > occurred_after);
        }

        if let Some(occurred_before) = filter.occurred_before {
            matching_events.retain(|e| e.occurred_at < occurred_before);
        }

        // Sort by occurred_at DESC (newest first), then sequence_number ASC
        matching_events.sort_by(|a, b| {
            match b.occurred_at.cmp(&a.occurred_at) {
                std::cmp::Ordering::Equal => a.sequence_number.cmp(&b.sequence_number),
                other => other,
            }
        });

        let total = matching_events.len() as u64;

        // Apply pagination
        let start = pagination.offset as usize;
        let end = (start + pagination.limit as usize).min(matching_events.len());
        let events = matching_events[start..end].to_vec();

        let has_more = end < matching_events.len();

        Ok(EventQueryResult {
            events,
            total,
            pagination,
            has_more,
        })
    }

    async fn get_event_by_id(
        &self,
        tenant_id: TenantId,
        event_id: Uuid,
    ) -> Result<Option<StoredEvent>, EventStoreError> {
        let store = self.inner.read().map_err(|e| {
            EventStoreError::InvalidAppend(format!("lock acquisition failed: {}", e))
        })?;

        let event = store
            .iter()
            .filter(|(key, _)| key.0 == tenant_id) // Tenant isolation
            .flat_map(|(_, events)| events)
            .find(|e| e.event_id == event_id)
            .cloned();

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_empty_store() {
        let store = InMemoryEventStore::new();
        let inner = store.inner.read().unwrap();
        assert!(inner.is_empty());
    }

    #[test]
    fn test_append_events() {
        let store = InMemoryEventStore::new();
        let tenant_id = TenantId::new();
        let aggregate_id = AggregateId::new();

        let event = UncommittedEvent {
            event_id: Uuid::new_v7(),
            tenant_id,
            aggregate_id,
            aggregate_type: "test.item".to_string(),
            event_type: "test.created".to_string(),
            event_version: 1,
            occurred_at: chrono::Utc::now(),
            payload: serde_json::json!({"name": "test"}),
        };

        let result = store.append(vec![event], ExpectedVersion::Any);
        assert!(result.is_ok());

        let stored = result.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].sequence_number, 1);
    }

    #[test]
    fn test_version_check() {
        let store = InMemoryEventStore::new();
        let tenant_id = TenantId::new();
        let aggregate_id = AggregateId::new();

        // First append should succeed
        let event1 = UncommittedEvent {
            event_id: Uuid::new_v7(),
            tenant_id,
            aggregate_id,
            aggregate_type: "test.item".to_string(),
            event_type: "test.created".to_string(),
            event_version: 1,
            occurred_at: chrono::Utc::now(),
            payload: serde_json::json!({}),
        };

        let result1 = store.append(vec![event1], ExpectedVersion::Any);
        assert!(result1.is_ok());

        // Second append with wrong version should fail
        let event2 = UncommittedEvent {
            event_id: Uuid::new_v7(),
            tenant_id,
            aggregate_id,
            aggregate_type: "test.item".to_string(),
            event_type: "test.updated".to_string(),
            event_version: 1,
            occurred_at: chrono::Utc::now(),
            payload: serde_json::json!({}),
        };

        let result2 = store.append(vec![event2], ExpectedVersion::Exact(999));
        assert!(result2.is_err());
    }
}
