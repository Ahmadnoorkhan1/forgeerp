//! Event query interface for inspection and debugging.
//!
//! This module provides read-only query capabilities for inspecting events
//! in the event store. All queries are tenant-scoped and paginated by default.

use chrono::{DateTime, Utc};
use forgeerp_core::{AggregateId, TenantId};
use serde::{Deserialize, Serialize};

use crate::event_store::{EventStoreError, StoredEvent};

/// Pagination parameters for event queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pagination {
    /// Maximum number of events to return.
    pub limit: u32,
    /// Offset for pagination (0-based).
    pub offset: u32,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            limit: 50,  // Safe default
            offset: 0,
        }
    }
}

impl Pagination {
    pub fn new(limit: Option<u32>, offset: Option<u32>) -> Self {
        Self {
            limit: limit.unwrap_or(50).min(1000), // Cap at 1000 for safety
            offset: offset.unwrap_or(0),
        }
    }
}

/// Filter criteria for event queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Filter by aggregate ID (optional).
    pub aggregate_id: Option<AggregateId>,
    /// Filter by aggregate type (optional, e.g., "inventory.item").
    pub aggregate_type: Option<String>,
    /// Filter by event type (optional, e.g., "inventory.item.created").
    pub event_type: Option<String>,
    /// Filter events that occurred after this time (optional).
    pub occurred_after: Option<DateTime<Utc>>,
    /// Filter events that occurred before this time (optional).
    pub occurred_before: Option<DateTime<Utc>>,
}

impl Default for EventFilter {
    fn default() -> Self {
        Self {
            aggregate_id: None,
            aggregate_type: None,
            event_type: None,
            occurred_after: None,
            occurred_before: None,
        }
    }
}

/// Paginated event query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventQueryResult {
    /// The events matching the query.
    pub events: Vec<StoredEvent>,
    /// Total number of events matching the filter (across all pages).
    pub total: u64,
    /// Pagination parameters used.
    pub pagination: Pagination,
    /// Whether there are more events available.
    pub has_more: bool,
}

/// Async query interface for event inspection.
///
/// This trait provides read-only query capabilities for inspecting events.
/// All queries are tenant-scoped and paginated by default.
#[async_trait::async_trait]
pub trait EventQuery: Send + Sync {
    /// Query events for a tenant with optional filters and pagination.
    ///
    /// Returns events matching the filter criteria, ordered by occurred_at (descending)
    /// and sequence_number (ascending for same timestamp).
    async fn query_events(
        &self,
        tenant_id: TenantId,
        filter: EventFilter,
        pagination: Pagination,
    ) -> Result<EventQueryResult, EventStoreError>;

    /// Get events for a specific aggregate stream.
    ///
    /// This is a convenience method that queries events for a specific aggregate.
    /// Note: This uses `query_events` which orders by occurred_at DESC; for aggregate streams,
    /// you may want sequence_number order (ascending), which `load_stream` provides.
    /// This method is provided for consistency with the query interface.
    async fn get_aggregate_events(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        pagination: Option<Pagination>,
    ) -> Result<EventQueryResult, EventStoreError> {
        let filter = EventFilter {
            aggregate_id: Some(aggregate_id),
            ..Default::default()
        };
        self.query_events(tenant_id, filter, pagination.unwrap_or_default()).await
    }

    /// Get a single event by its ID.
    ///
    /// Returns the event if it exists and belongs to the tenant.
    async fn get_event_by_id(
        &self,
        tenant_id: TenantId,
        event_id: uuid::Uuid,
    ) -> Result<Option<StoredEvent>, EventStoreError>;
}

