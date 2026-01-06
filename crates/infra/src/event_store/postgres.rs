//! Postgres-backed event store implementation.
//!
//! This module provides a persistent event store using PostgreSQL as the backing storage.
//! It enforces tenant isolation, optimistic concurrency control, and append-only semantics
//! at the database level.
//!
//! ## Error Mapping
//!
//! SQLx errors are mapped to `EventStoreError` as follows:
//!
//! | SQLx Error | PostgreSQL Error Code | EventStoreError | Scenario |
//! |------------|----------------------|-----------------|----------|
//! | Database (unique violation) | `23505` | `Concurrency` | Concurrent append detected (unique constraint on sequence_number) |
//! | Database (foreign key violation) | `23503` | `InvalidAppend` | Referential integrity violation (should not occur in our schema) |
//! | Database (check constraint violation) | `23514` | `InvalidAppend` | Invalid data (e.g., sequence_number <= 0) |
//! | Database (other) | Any other | `InvalidAppend` | Other database errors |
//! | PoolClosed | N/A | `InvalidAppend` | Connection pool was closed |
//! | RowNotFound | N/A | `InvalidAppend` | Unexpected row not found (should not occur) |
//! | Other | N/A | `InvalidAppend` | Network errors, connection failures, etc. |
//!
//! ## Thread Safety
//!
//! `PostgresEventStore` is `Send + Sync` and can be shared across threads.
//! All operations use the SQLx connection pool which handles thread-safe connection management.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};
use std::sync::Arc;
use tracing::{instrument, Span};

use forgeerp_core::{AggregateId, ExpectedVersion, TenantId};

use super::query::{EventFilter, EventQuery, EventQueryResult, Pagination};
use super::r#trait::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};

/// Postgres-backed append-only event store.
///
/// This implementation uses PostgreSQL to persist events in an append-only fashion,
/// with tenant isolation and optimistic concurrency control enforced at the database level.
///
/// ## Thread Safety
///
/// Uses SQLx connection pool which is thread-safe (Arc + Send + Sync).
/// All database operations use transactions to ensure atomicity.
///
/// ## Tenant Isolation
///
/// Every query includes `tenant_id` in the WHERE clause. This makes it impossible
/// to accidentally load or modify events from a different tenant. The database schema
/// also supports Row-Level Security (RLS) as an additional layer of defense.
///
/// ## Optimistic Concurrency
///
/// The `append()` method uses a transaction to:
/// 1. Check the current stream version (MAX(sequence_number))
/// 2. Validate it matches `expected_version`
/// 3. Insert new events atomically
///
/// If another transaction commits between steps 1 and 3, the unique constraint
/// on `(tenant_id, aggregate_id, sequence_number)` will cause the insert to fail,
/// resulting in a concurrency error.
#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: Arc<PgPool>,
}

impl PostgresEventStore {
    /// Create a new PostgresEventStore with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    /// Load all events for a tenant + aggregate stream.
    ///
    /// Events are returned in sequence number order (ascending).
    /// Returns an empty vector if the stream doesn't exist.
    #[instrument(
        skip(self),
        fields(
            tenant_id = %tenant_id.as_uuid(),
            aggregate_id = %aggregate_id.as_uuid()
        ),
        err
    )]
    pub async fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        let span = Span::current();
        span.record("operation", "load_stream");

        let rows = sqlx::query(
            r#"
            SELECT
                event_id,
                tenant_id,
                aggregate_id,
                aggregate_type,
                sequence_number,
                event_type,
                event_version,
                occurred_at,
                payload,
                created_at
            FROM events
            WHERE tenant_id = $1 AND aggregate_id = $2
            ORDER BY sequence_number ASC
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("load_stream", e))?;

        let mut stored_events = Vec::with_capacity(rows.len());
        for row in rows {
            let stored = StoredEventRow::from_row(&row)
                .map_err(|e| EventStoreError::InvalidAppend(format!("failed to deserialize event row: {}", e)))?;
            stored_events.push(stored.into());
        }

        span.record("event_count", stored_events.len());
        Ok(stored_events)
    }

    /// Append events to a stream with optimistic concurrency control.
    ///
    /// This method:
    /// 1. Starts a transaction
    /// 2. Checks the current stream version
    /// 3. Validates it matches `expected_version`
    /// 4. Inserts new events atomically
    /// 5. Commits the transaction
    ///
    /// If the version check fails or if another transaction inserts events concurrently,
    /// returns `EventStoreError::Concurrency`.
    #[instrument(
        skip(self, events),
        fields(
            tenant_id = %tenant_id.as_uuid(),
            aggregate_id = %aggregate_id.as_uuid(),
            event_count = events.len(),
            expected_version = ?expected_version
        ),
        err
    )]
    pub async fn append_events(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        let span = Span::current();
        span.record("operation", "append_events");

        // Validate all events target the same tenant + aggregate
        let first_event = &events[0];
        if first_event.tenant_id != tenant_id {
            return Err(EventStoreError::TenantIsolation(format!(
                "event tenant_id mismatch: expected {}, got {}",
                tenant_id.as_uuid(),
                first_event.tenant_id.as_uuid()
            )));
        }
        if first_event.aggregate_id != aggregate_id {
            return Err(EventStoreError::InvalidAppend(format!(
                "event aggregate_id mismatch: expected {}, got {}",
                aggregate_id.as_uuid(),
                first_event.aggregate_id.as_uuid()
            )));
        }

        // Validate all events have the same tenant/aggregate
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
        }

        let aggregate_type = events[0].aggregate_type.clone();

        // Use a transaction for atomicity
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| map_sqlx_error("begin_transaction", e))?;

        // Check current version and aggregate type
        let (current_version, existing_aggregate_type) = check_stream_version(
            &mut tx,
            tenant_id,
            aggregate_id,
        )
        .await?;

        // Validate aggregate type consistency
        if let Some(ref existing_type) = existing_aggregate_type {
            if existing_type != &aggregate_type {
                tx.rollback()
                    .await
                    .map_err(|e| map_sqlx_error("rollback", e))?;
                return Err(EventStoreError::AggregateTypeMismatch(format!(
                    "stream aggregate_type is '{}', attempted append with '{}'",
                    existing_type, aggregate_type
                )));
            }
        }

        // Validate expected version
        if !expected_version.matches(current_version) {
            tx.rollback()
                .await
                .map_err(|e| map_sqlx_error("rollback", e))?;
            return Err(EventStoreError::Concurrency(format!(
                "optimistic concurrency check failed: expected {:?}, found {}",
                expected_version, current_version
            )));
        }

        // Insert events with sequence numbers starting at current_version + 1
        let mut stored_events = Vec::with_capacity(events.len());
        let mut next_sequence = current_version + 1;

        for event in events {
            // Verify tenant/aggregate consistency one more time
            if event.tenant_id != tenant_id || event.aggregate_id != aggregate_id {
                tx.rollback()
                    .await
                    .map_err(|e| map_sqlx_error("rollback", e))?;
                return Err(EventStoreError::TenantIsolation(
                    "event tenant/aggregate mismatch in batch".to_string(),
                ));
            }

        sqlx::query(
            r#"
            INSERT INTO events (
                event_id,
                tenant_id,
                aggregate_id,
                aggregate_type,
                sequence_number,
                event_type,
                event_version,
                occurred_at,
                payload
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(event.event_id)
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .bind(&aggregate_type)
        .bind(next_sequence as i64)
        .bind(&event.event_type)
        .bind(event.event_version as i32)
        .bind(event.occurred_at)
        .bind(&event.payload)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            // Map unique constraint violations to concurrency errors
            // (happens when another transaction inserts concurrently)
            if is_unique_violation(&e) {
                EventStoreError::Concurrency(format!(
                    "concurrent append detected: sequence_number {} already exists",
                    next_sequence
                ))
            } else {
                map_sqlx_error("insert_event", e)
            }
        })?;

            let stored = StoredEvent {
                event_id: event.event_id,
                tenant_id: event.tenant_id,
                aggregate_id: event.aggregate_id,
                aggregate_type: event.aggregate_type,
                sequence_number: next_sequence,
                event_type: event.event_type,
                event_version: event.event_version,
                occurred_at: event.occurred_at,
                payload: event.payload,
            };
            stored_events.push(stored);
            next_sequence += 1;
        }

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| map_sqlx_error("commit_transaction", e))?;

        span.record("committed_events", stored_events.len());
        Ok(stored_events)
    }

    /// Load the latest snapshot for a tenant + aggregate.
    ///
    /// Returns `None` if no snapshot exists for this aggregate.
    #[instrument(
        skip(self),
        fields(
            tenant_id = %tenant_id.as_uuid(),
            aggregate_id = %aggregate_id.as_uuid()
        ),
        err
    )]
    pub async fn load_snapshot(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Option<Snapshot>, EventStoreError> {
        let span = Span::current();
        span.record("operation", "load_snapshot");

        let row = sqlx::query(
            r#"
            SELECT
                tenant_id,
                aggregate_id,
                aggregate_type,
                version,
                state,
                created_at
            FROM snapshots
            WHERE tenant_id = $1 AND aggregate_id = $2
            ORDER BY version DESC
            LIMIT 1
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("load_snapshot", e))?;

        if let Some(row) = row {
            let snapshot_row = SnapshotRow::from_row(&row)
                .map_err(|e| EventStoreError::InvalidAppend(format!("failed to deserialize snapshot row: {}", e)))?;
            span.record("snapshot_found", true);
            span.record("snapshot_version", snapshot_row.version);
            Ok(Some(snapshot_row.into()))
        } else {
            span.record("snapshot_found", false);
            Ok(None)
        }
    }

    /// Store a snapshot for a tenant + aggregate at a specific version.
    ///
    /// If a snapshot already exists at this version, it will be overwritten.
    /// Multiple snapshots per aggregate are allowed (for versioning/history).
    #[instrument(
        skip(self),
        fields(
            tenant_id = %tenant_id.as_uuid(),
            aggregate_id = %aggregate_id.as_uuid(),
            version = snapshot.version
        ),
        err
    )]
    pub async fn store_snapshot(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        snapshot: &Snapshot,
    ) -> Result<(), EventStoreError> {
        // Validate tenant/aggregate match
        if snapshot.tenant_id != tenant_id {
            return Err(EventStoreError::TenantIsolation(format!(
                "snapshot tenant_id mismatch: expected {}, got {}",
                tenant_id.as_uuid(),
                snapshot.tenant_id.as_uuid()
            )));
        }
        if snapshot.aggregate_id != aggregate_id {
            return Err(EventStoreError::InvalidAppend(format!(
                "snapshot aggregate_id mismatch: expected {}, got {}",
                aggregate_id.as_uuid(),
                snapshot.aggregate_id.as_uuid()
            )));
        }

        let span = Span::current();
        span.record("operation", "store_snapshot");

        sqlx::query(
            r#"
            INSERT INTO snapshots (
                tenant_id,
                aggregate_id,
                aggregate_type,
                version,
                state
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (tenant_id, aggregate_id, version)
            DO UPDATE SET
                state = EXCLUDED.state,
                created_at = NOW()
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .bind(&snapshot.aggregate_type)
        .bind(snapshot.version as i64)
        .bind(&snapshot.state)
        .execute(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("store_snapshot", e))?;

        Ok(())
    }
}

/// Aggregate snapshot for fast rehydration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub tenant_id: TenantId,
    pub aggregate_id: AggregateId,
    pub aggregate_type: String,
    pub version: u64,
    pub state: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Check the current version of a stream.
///
/// Returns `(current_version, aggregate_type)` where `current_version` is 0 if the stream
/// doesn't exist, and `aggregate_type` is `None` if the stream doesn't exist.
async fn check_stream_version(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: TenantId,
    aggregate_id: AggregateId,
) -> Result<(u64, Option<String>), EventStoreError> {
    let row = sqlx::query(
        r#"
        SELECT
            COALESCE(MAX(sequence_number), 0) as current_version,
            MAX(aggregate_type) as aggregate_type
        FROM events
        WHERE tenant_id = $1 AND aggregate_id = $2
        "#,
    )
    .bind(tenant_id.as_uuid())
    .bind(aggregate_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| map_sqlx_error("check_stream_version", e))?;

    let current_version: Option<i64> = row.try_get("current_version")
        .map_err(|e| EventStoreError::InvalidAppend(format!("failed to read current_version: {}", e)))?;
    let aggregate_type: Option<String> = row.try_get("aggregate_type")
        .map_err(|e| EventStoreError::InvalidAppend(format!("failed to read aggregate_type: {}", e)))?;

    Ok((
        current_version.unwrap_or(0) as u64,
        aggregate_type,
    ))
}

/// Map SQLx errors to EventStoreError.
fn map_sqlx_error(operation: &str, err: sqlx::Error) -> EventStoreError {
    match err {
        sqlx::Error::Database(db_err) => {
            let msg = format!("database error in {}: {}", operation, db_err.message());
            
            // Check for specific error codes
            if let Some(code) = db_err.code() {
                match code.as_ref() {
                    "23505" => {
                        // Unique violation
                        EventStoreError::Concurrency(msg)
                    }
                    "23503" => {
                        // Foreign key violation (shouldn't happen in our schema)
                        EventStoreError::InvalidAppend(msg)
                    }
                    "23514" => {
                        // Check constraint violation
                        EventStoreError::InvalidAppend(msg)
                    }
                    _ => EventStoreError::InvalidAppend(msg),
                }
            } else {
                EventStoreError::InvalidAppend(msg)
            }
        }
        sqlx::Error::PoolClosed => {
            EventStoreError::InvalidAppend(format!("connection pool closed in {}", operation))
        }
        sqlx::Error::RowNotFound => {
            // This should not happen for our queries (we use fetch_optional/fetch_all)
            EventStoreError::InvalidAppend(format!("unexpected row not found in {}", operation))
        }
        _ => EventStoreError::InvalidAppend(format!(
            "sqlx error in {}: {}",
            operation, err
        )),
    }
}

/// Check if an error is a unique constraint violation.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        if let Some(code) = db_err.code() {
            return code.as_ref() == "23505";
        }
    }
    false
}

// SQLx row types

#[derive(Debug)]
struct StoredEventRow {
    event_id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    aggregate_id: uuid::Uuid,
    aggregate_type: String,
    sequence_number: i64,
    event_type: String,
    event_version: i32,
    occurred_at: DateTime<Utc>,
    payload: serde_json::Value,
    #[allow(dead_code)] // Not used in StoredEvent, but kept for potential future use (e.g., monitoring)
    created_at: DateTime<Utc>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for StoredEventRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(StoredEventRow {
            event_id: row.try_get("event_id")?,
            tenant_id: row.try_get("tenant_id")?,
            aggregate_id: row.try_get("aggregate_id")?,
            aggregate_type: row.try_get("aggregate_type")?,
            sequence_number: row.try_get("sequence_number")?,
            event_type: row.try_get("event_type")?,
            event_version: row.try_get("event_version")?,
            occurred_at: row.try_get("occurred_at")?,
            payload: row.try_get("payload")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl From<StoredEventRow> for StoredEvent {
    fn from(row: StoredEventRow) -> Self {
        StoredEvent {
            event_id: row.event_id,
            tenant_id: TenantId::from_uuid(row.tenant_id),
            aggregate_id: AggregateId::from_uuid(row.aggregate_id),
            aggregate_type: row.aggregate_type,
            sequence_number: row.sequence_number as u64,
            event_type: row.event_type,
            event_version: row.event_version as u32,
            occurred_at: row.occurred_at,
            payload: row.payload,
        }
    }
}

#[derive(Debug)]
struct SnapshotRow {
    tenant_id: uuid::Uuid,
    aggregate_id: uuid::Uuid,
    aggregate_type: String,
    version: i64,
    state: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for SnapshotRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(SnapshotRow {
            tenant_id: row.try_get("tenant_id")?,
            aggregate_id: row.try_get("aggregate_id")?,
            aggregate_type: row.try_get("aggregate_type")?,
            version: row.try_get("version")?,
            state: row.try_get("state")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl From<SnapshotRow> for Snapshot {
    fn from(row: SnapshotRow) -> Self {
        Snapshot {
            tenant_id: TenantId::from_uuid(row.tenant_id),
            aggregate_id: AggregateId::from_uuid(row.aggregate_id),
            aggregate_type: row.aggregate_type,
            version: row.version as u64,
            state: row.state,
            created_at: row.created_at,
        }
    }
}

// Implement EventStore trait

impl EventStore for PostgresEventStore {
    fn append(
        &self,
        events: Vec<UncommittedEvent>,
        expected_version: ExpectedVersion,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        // The EventStore trait is synchronous, but Postgres operations require async.
        // We use tokio::runtime::Handle to run async code in a sync context.
        // This works when called from within a tokio runtime (e.g., from axum handlers).
        
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| EventStoreError::InvalidAppend(
                "PostgresEventStore requires async runtime (tokio). Ensure you're calling from within a tokio runtime context.".to_string()
            ))?;

        if events.is_empty() {
            return Ok(vec![]);
        }

        let tenant_id = events[0].tenant_id;
        let aggregate_id = events[0].aggregate_id;

        // Use block_on to run the async append operation
        handle.block_on(
            self.append_events(tenant_id, aggregate_id, events, expected_version)
        )
    }

    fn load_stream(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
    ) -> Result<Vec<StoredEvent>, EventStoreError> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| EventStoreError::InvalidAppend(
                "PostgresEventStore requires async runtime (tokio). Ensure you're calling from within a tokio runtime context.".to_string()
            ))?;

        handle.block_on(self.load_stream(tenant_id, aggregate_id))
    }
}

#[async_trait::async_trait]
impl EventQuery for PostgresEventStore {
    async fn query_events(
        &self,
        tenant_id: TenantId,
        filter: EventFilter,
        pagination: Pagination,
    ) -> Result<EventQueryResult, EventStoreError> {
        // Build WHERE conditions using COALESCE for optional filters
        // This allows us to use a single parameterized query
        let agg_id_param: Option<uuid::Uuid> = filter.aggregate_id.map(|id| *id.as_uuid());
        let agg_type_param: Option<&str> = filter.aggregate_type.as_deref();
        let evt_type_param: Option<&str> = filter.event_type.as_deref();

        // Count query with filters
        let count_row = sqlx::query(
            r#"
            SELECT COUNT(*) as total
            FROM events
            WHERE tenant_id = $1
                AND ($2::uuid IS NULL OR aggregate_id = $2)
                AND ($3::text IS NULL OR aggregate_type = $3)
                AND ($4::text IS NULL OR event_type = $4)
                AND ($5::timestamp IS NULL OR occurred_at >= $5)
                AND ($6::timestamp IS NULL OR occurred_at <= $6)
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(agg_id_param)
        .bind(agg_type_param)
        .bind(evt_type_param)
        .bind(filter.occurred_after)
        .bind(filter.occurred_before)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("count_events", e))?;

        let total: i64 = count_row
            .try_get("total")
            .map_err(|e| EventStoreError::InvalidAppend(format!("failed to read count: {}", e)))?;

        // Events query with filters and pagination
        let rows = sqlx::query(
            r#"
            SELECT
                event_id,
                tenant_id,
                aggregate_id,
                aggregate_type,
                sequence_number,
                event_type,
                event_version,
                occurred_at,
                payload,
                created_at
            FROM events
            WHERE tenant_id = $1
                AND ($2::uuid IS NULL OR aggregate_id = $2)
                AND ($3::text IS NULL OR aggregate_type = $3)
                AND ($4::text IS NULL OR event_type = $4)
                AND ($5::timestamp IS NULL OR occurred_at >= $5)
                AND ($6::timestamp IS NULL OR occurred_at <= $6)
            ORDER BY occurred_at DESC, sequence_number ASC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(agg_id_param)
        .bind(agg_type_param)
        .bind(evt_type_param)
        .bind(filter.occurred_after)
        .bind(filter.occurred_before)
        .bind(pagination.limit as i64)
        .bind(pagination.offset as i64)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("query_events", e))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let stored = StoredEventRow::from_row(&row)
                .map_err(|e| EventStoreError::InvalidAppend(format!("failed to deserialize event row: {}", e)))?;
            events.push(stored.into());
        }

        let has_more = total > (pagination.offset + pagination.limit) as i64;

        Ok(EventQueryResult {
            events,
            total: total as u64,
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
        // For aggregate streams, order by sequence_number (ascending) for chronological replay
        let pagination = pagination.unwrap_or_default();

        // Count total events for this aggregate
        let count_row = sqlx::query(
            "SELECT COUNT(*) as total FROM events WHERE tenant_id = $1 AND aggregate_id = $2"
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("count_aggregate_events", e))?;

        let total: i64 = count_row
            .try_get("total")
            .map_err(|e| EventStoreError::InvalidAppend(format!("failed to read count: {}", e)))?;

        // Query events ordered by sequence_number (ascending)
        let rows = sqlx::query(
            r#"
            SELECT
                event_id,
                tenant_id,
                aggregate_id,
                aggregate_type,
                sequence_number,
                event_type,
                event_version,
                occurred_at,
                payload,
                created_at
            FROM events
            WHERE tenant_id = $1 AND aggregate_id = $2
            ORDER BY sequence_number ASC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .bind(pagination.limit as i64)
        .bind(pagination.offset as i64)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("get_aggregate_events", e))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let stored = StoredEventRow::from_row(&row)
                .map_err(|e| EventStoreError::InvalidAppend(format!("failed to deserialize event row: {}", e)))?;
            events.push(stored.into());
        }

        let has_more = total > (pagination.offset + pagination.limit) as i64;

        Ok(EventQueryResult {
            events,
            total: total as u64,
            pagination,
            has_more,
        })
    }

    async fn get_event_by_id(
        &self,
        tenant_id: TenantId,
        event_id: uuid::Uuid,
    ) -> Result<Option<StoredEvent>, EventStoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                event_id,
                tenant_id,
                aggregate_id,
                aggregate_type,
                sequence_number,
                event_type,
                event_version,
                occurred_at,
                payload,
                created_at
            FROM events
            WHERE tenant_id = $1 AND event_id = $2
            LIMIT 1
            "#,
        )
        .bind(tenant_id.as_uuid())
        .bind(event_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| map_sqlx_error("get_event_by_id", e))?;

        if let Some(row) = row {
            let stored = StoredEventRow::from_row(&row)
                .map_err(|e| EventStoreError::InvalidAppend(format!("failed to deserialize event row: {}", e)))?;
            Ok(Some(stored.into()))
        } else {
            Ok(None)
        }
    }
}

