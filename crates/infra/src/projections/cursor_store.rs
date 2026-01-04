//! Projection cursor/offset persistence.
//!
//! This module provides persistence for projection cursors (checkpoints) that track
//! the last processed sequence_number per (tenant, aggregate) stream. This enables:
//! - Idempotent projections (replays <= cursor are ignored)
//! - Resume after crash (projections can continue from last offset)
//! - Deterministic rebuilds (clear offsets and replay from scratch)

use std::sync::Arc;

use forgeerp_core::{AggregateId, TenantId};
use sqlx::{PgPool, Row};

/// Projection cursor store for persisting offsets.
pub trait ProjectionCursorStore: Send + Sync {
    /// Get the last processed sequence_number for a (tenant, aggregate, projection) stream.
    fn get_cursor(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        projection_name: &str,
    ) -> Option<u64>;

    /// Update the cursor to a new sequence_number.
    fn update_cursor(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        projection_name: &str,
        sequence_number: u64,
    );

    /// Clear all cursors for a tenant + projection (for rebuilds).
    fn clear_cursors(&self, tenant_id: TenantId, projection_name: &str);
}

/// Postgres-backed projection cursor store.
pub struct PostgresCursorStore {
    pool: Arc<PgPool>,
}

impl PostgresCursorStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }
}

impl ProjectionCursorStore for PostgresCursorStore {
    fn get_cursor(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        projection_name: &str,
    ) -> Option<u64> {
        let handle = tokio::runtime::Handle::try_current().ok()?;
        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();
        let aggregate_id_uuid = aggregate_id.as_uuid();
        let projection_name = projection_name.to_string();

        handle.block_on(async {
            match sqlx::query(
                r#"
                SELECT last_sequence_number
                FROM projection_offsets
                WHERE tenant_id = $1 AND aggregate_id = $2 AND projection_name = $3
                "#,
            )
            .bind(tenant_id_uuid)
            .bind(aggregate_id_uuid)
            .bind(&projection_name)
            .fetch_optional(&*pool)
            .await
            {
                Ok(Some(row)) => {
                    match row.try_get::<i64, _>("last_sequence_number") {
                        Ok(seq) => Some(seq as u64),
                        Err(_) => None,
                    }
                }
                Ok(None) => None,
                Err(_) => None,
            }
        })
    }

    fn update_cursor(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        projection_name: &str,
        sequence_number: u64,
    ) {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return,
        };

        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();
        let aggregate_id_uuid = aggregate_id.as_uuid();
        let projection_name = projection_name.to_string();

        let _ = handle.block_on(async {
            let _ = sqlx::query(
                r#"
                INSERT INTO projection_offsets (
                    tenant_id,
                    aggregate_id,
                    projection_name,
                    last_sequence_number
                )
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (tenant_id, aggregate_id, projection_name)
                DO UPDATE SET
                    last_sequence_number = EXCLUDED.last_sequence_number,
                    updated_at = NOW()
                "#,
            )
            .bind(tenant_id_uuid)
            .bind(aggregate_id_uuid)
            .bind(&projection_name)
            .bind(sequence_number as i64)
            .execute(&*pool)
            .await;
        });
    }

    fn clear_cursors(&self, tenant_id: TenantId, projection_name: &str) {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return,
        };

        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();
        let projection_name = projection_name.to_string();

        let _ = handle.block_on(async {
            let _ = sqlx::query(
                r#"
                SELECT clear_tenant_offsets($1, $2)
                "#,
            )
            .bind(tenant_id_uuid)
            .bind(&projection_name)
            .execute(&*pool)
            .await;
        });
    }
}

