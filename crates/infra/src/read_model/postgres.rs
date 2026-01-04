//! Postgres-backed tenant store implementation.
//!
//! This module provides a persistent read model storage using PostgreSQL.
//! It implements the `TenantStore` trait for tenant-isolated key/value storage
//! with support for deterministic rebuilds and cursor persistence.

use std::sync::Arc;
use tracing::Span;

use forgeerp_core::TenantId;
use sqlx::{PgPool, Row};

use super::TenantStore;

/// Postgres-backed tenant store for read models.
///
///
/// ## Thread Safety
///
/// Uses SQLx connection pool which is thread-safe (Arc + Send + Sync).
/// All operations use SQL transactions implicitly via connection pool.
///
/// ## Tenant Isolation
///
/// Every query includes `tenant_id` in the WHERE clause or as part of the primary key.
/// This makes cross-tenant access architecturally impossible.
///
/// ## Deterministic Rebuilds
///
/// The `clear_tenant()` method removes all read model data for a tenant, enabling
/// deterministic rebuilds from the event stream.
pub struct PostgresTenantStore<K, V> {
    pool: Arc<PgPool>,
    _key: std::marker::PhantomData<K>,
    _value: std::marker::PhantomData<V>,
}

impl<K, V> PostgresTenantStore<K, V> {
    /// Create a new PostgresTenantStore with the given connection pool.
    ///
    /// Note: The table name is determined by the value type `V`. For `InventoryReadModel`,
    /// this will use the `inventory_stock` table.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(pool),
            _key: std::marker::PhantomData,
            _value: std::marker::PhantomData,
        }
    }
}

impl<K, V> TenantStore<K, V> for PostgresTenantStore<K, V>
where
    K: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, _tenant_id: TenantId, _key: &K) -> Option<V> {
        // This is a generic trait, but we need to map to specific tables.
        // For now, we'll implement this only for InventoryReadModel specifically.
        // A fully generic implementation would require table mapping configuration.
        unimplemented!("PostgresTenantStore is implemented for specific types only. Use PostgresInventoryStore instead.")
    }

    fn upsert(&self, _tenant_id: TenantId, _key: K, _value: V) {
        unimplemented!("PostgresTenantStore is implemented for specific types only. Use PostgresInventoryStore instead.")
    }

    fn list(&self, _tenant_id: TenantId) -> Vec<V> {
        unimplemented!("PostgresTenantStore is implemented for specific types only. Use PostgresInventoryStore instead.")
    }

    fn clear_tenant(&self, tenant_id: TenantId) {
        let handle = tokio::runtime::Handle::try_current();
        if let Ok(handle) = handle {
            let pool = self.pool.clone();
            let _ = handle.block_on(async move {
                sqlx::query("SELECT clear_tenant_read_models($1)")
                    .bind(tenant_id.as_uuid())
                    .execute(&*pool)
                    .await
            });
        }
    }
}

/// Postgres-backed tenant store specifically for InventoryReadModel.
///
/// This is a concrete implementation that maps to the `inventory_stock` table.
pub struct PostgresInventoryStore {
    pool: Arc<PgPool>,
}

impl PostgresInventoryStore {
    /// Create a new PostgresInventoryStore with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }
}

// Import types needed for InventoryReadModel implementation
use crate::projections::inventory_stock::InventoryReadModel;
use forgeerp_inventory::InventoryItemId;

impl TenantStore<InventoryItemId, InventoryReadModel> for PostgresInventoryStore {
    fn get(&self, tenant_id: TenantId, key: &InventoryItemId) -> Option<InventoryReadModel> {
        let handle = tokio::runtime::Handle::try_current().ok()?;
        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();
        let item_id_uuid = key.0.as_uuid();

        handle.block_on(async {
            let span = Span::current();
            span.record("operation", "get_inventory_stock");

            match sqlx::query(
                r#"
                SELECT
                    tenant_id,
                    item_id,
                    name,
                    quantity,
                    updated_at
                FROM inventory_stock
                WHERE tenant_id = $1 AND item_id = $2
                "#,
            )
            .bind(tenant_id_uuid)
            .bind(item_id_uuid)
            .fetch_optional(&*pool)
            .await
            {
                Ok(Some(row)) => {
                    match (row.try_get::<String, _>("name"), row.try_get::<i64, _>("quantity"), row.try_get::<uuid::Uuid, _>("item_id")) {
                        (Ok(name), Ok(quantity), Ok(item_id)) => Some(InventoryReadModel {
                            item_id: InventoryItemId(forgeerp_core::AggregateId::from_uuid(item_id)),
                            name,
                            quantity,
                        }),
                        _ => None,
                    }
                }
                Ok(None) => None,
                Err(_) => None,
            }
        })
    }

    fn upsert(&self, tenant_id: TenantId, key: InventoryItemId, value: InventoryReadModel) {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return,
        };

        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();
        let item_id_uuid = key.0.as_uuid();

        let _ = handle.block_on(async {
            let span = Span::current();
            span.record("operation", "upsert_inventory_stock");

            let _ = sqlx::query(
                r#"
                INSERT INTO inventory_stock (
                    tenant_id,
                    item_id,
                    name,
                    quantity
                )
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (tenant_id, item_id)
                DO UPDATE SET
                    name = EXCLUDED.name,
                    quantity = EXCLUDED.quantity,
                    updated_at = NOW()
                "#,
            )
            .bind(tenant_id_uuid)
            .bind(item_id_uuid)
            .bind(&value.name)
            .bind(value.quantity)
            .execute(&*pool)
            .await;
        });
    }

    fn list(&self, tenant_id: TenantId) -> Vec<InventoryReadModel> {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return vec![],
        };

        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();

        handle.block_on(async {
            let span = Span::current();
            span.record("operation", "list_inventory_stock");

            match sqlx::query(
                r#"
                SELECT
                    tenant_id,
                    item_id,
                    name,
                    quantity,
                    updated_at
                FROM inventory_stock
                WHERE tenant_id = $1
                ORDER BY updated_at DESC
                "#,
            )
            .bind(tenant_id_uuid)
            .fetch_all(&*pool)
            .await
            {
                Ok(rows) => rows.into_iter()
                    .filter_map(|r| {
                        match (r.try_get::<uuid::Uuid, _>("item_id"), r.try_get::<String, _>("name"), r.try_get::<i64, _>("quantity")) {
                            (Ok(item_id), Ok(name), Ok(quantity)) => Some(InventoryReadModel {
                                item_id: InventoryItemId(forgeerp_core::AggregateId::from_uuid(item_id)),
                                name,
                                quantity,
                            }),
                            _ => None,
                        }
                    })
                    .collect(),
                Err(_) => vec![],
            }
        })
    }

    fn clear_tenant(&self, tenant_id: TenantId) {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return,
        };

        let pool = self.pool.clone();
        let tenant_id_uuid = tenant_id.as_uuid();

        let _ = handle.block_on(async {
            let span = Span::current();
            span.record("operation", "clear_tenant_inventory_stock");

            let _ = sqlx::query("SELECT clear_tenant_read_models($1)")
                .bind(tenant_id_uuid)
                .execute(&*pool)
                .await;
        });
    }
}

