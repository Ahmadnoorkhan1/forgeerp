//! Local read model cache for offline support.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use anyhow::Context;
use chrono::{DateTime, Utc};
use forgeerp_core::{AggregateId, TenantId};
use forgeerp_inventory::InventoryItemId;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use tokio::runtime::Runtime;

/// SQLite-backed local cache for read models (offline support).
#[derive(Debug, Clone)]
pub struct LocalCache {
    /// Shared SQLite connection pool.
    ///
    /// `SqlitePool` is already `Send + Sync`, but we still wrap it in `Arc<Mutex<_>>`
    /// to satisfy the explicit threading requirement and to allow cheap cloning
    /// of the `LocalCache` handle across threads.
    pool: Arc<tokio::sync::Mutex<Option<SqlitePool>>>,
}

// Re-export from shared types module
pub use crate::types::InventoryReadModel;

impl LocalCache {
    /// Create a new LocalCache (lazy initialization).
    ///
    /// The database will be initialized on first use.
    pub fn new() -> Self {
        Self {
            pool: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize the database connection (called lazily on first use).
    async fn ensure_initialized(&self) -> anyhow::Result<()> {
        let mut pool_guard = self.pool.lock().await;
        if pool_guard.is_some() {
            return Ok(());
        }

        let db_path = cache_db_path()
            .context("failed to determine cache DB path - ensure app data directory is accessible")?;
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create cache directory at {:?}", parent))?;
        }
        
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());

        let pool = SqlitePool::connect(&db_url)
            .await
            .with_context(|| format!("failed to create SQLite pool for LocalCache at {:?}", db_path))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS read_models (
                tenant_id      TEXT NOT NULL,
                aggregate_type TEXT NOT NULL,
                aggregate_id   TEXT NOT NULL,
                data           TEXT NOT NULL,
                cached_at      TEXT NOT NULL,
                version        INTEGER NULL,
                PRIMARY KEY (tenant_id, aggregate_type, aggregate_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .context("failed to create read_models table")?;

        *pool_guard = Some(pool);
        Ok(())
    }

    /// Get the pool, initializing if necessary.
    async fn get_pool(&self) -> anyhow::Result<sqlx::sqlite::SqlitePool> {
        self.ensure_initialized().await?;
        let pool_guard = self.pool.lock().await;
        Ok(pool_guard.as_ref().unwrap().clone())
    }


    /// Cache an inventory item read model.
    ///
    /// Preserves the existing public API and delegates to the generic helper.
    pub fn cache_inventory_item(
        &self,
        tenant_id: TenantId,
        item_id: InventoryItemId,
        model: InventoryReadModel,
    ) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for cache_inventory_item: {err:?}");
                return;
            }
        };

        if let Err(err) = rt.block_on(async {
            self.cache_read_model(
                &tenant_id,
                "inventory_item",
                &item_id.to_string(),
                &model,
            ).await
        }) {
            tracing::error!("failed to cache inventory item: {err:?}");
        }
    }

    /// Get a cached inventory item (if available and not stale beyond max_age).
    ///
    /// Preserves the existing public API while enforcing staleness based on the
    /// `cached_at` column in the SQLite table.
    pub fn get_inventory_item(
        &self,
        tenant_id: TenantId,
        item_id: &InventoryItemId,
        max_age: Option<chrono::Duration>,
    ) -> Option<InventoryReadModel> {
        // Use a one-off runtime for this synchronous API.
        let rt = Runtime::new().ok()?;
        let pool = rt.block_on(async {
            self.get_pool().await.ok()
        })?;

        let key_tenant = tenant_id.to_string();
        let key_agg_type = "inventory_item".to_string();
        let key_agg_id = item_id.to_string();

        let result: anyhow::Result<Option<InventoryReadModel>> = rt.block_on(async move {
            let row = sqlx::query(
                r#"
                SELECT data, cached_at
                FROM read_models
                WHERE tenant_id = ?1
                  AND aggregate_type = ?2
                  AND aggregate_id = ?3
                "#,
            )
            .bind(&key_tenant)
            .bind(&key_agg_type)
            .bind(&key_agg_id)
            .fetch_optional(&pool)
            .await
            .context("failed to fetch inventory item from cache")?;

            let row = match row {
                Some(row) => row,
                None => return Ok(None),
            };

            let data: String = row.try_get("data")?;
            let cached_at_str: String = row.try_get("cached_at")?;
            let cached_at = DateTime::parse_from_rfc3339(&cached_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .context("invalid cached_at timestamp in cache")?;

        if let Some(max) = max_age {
                let age = Utc::now().signed_duration_since(cached_at);
            if age > max {
                    return Ok(None);
            }
        }

            let model: InventoryReadModel =
                serde_json::from_str(&data).context("failed to deserialize inventory read model")?;

            Ok(Some(model))
        });

        match result {
            Ok(model) => model,
            Err(err) => {
                tracing::error!("failed to get inventory item from cache: {err:?}");
                None
            }
        }
    }

    /// Clear all cached data for a tenant.
    pub fn clear_tenant(&self, tenant_id: TenantId) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for clear_tenant: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for clear_tenant (cache may not be initialized): {err:?}");
                return;
            }
        };

        let key_tenant = tenant_id.to_string();

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                DELETE FROM read_models
                WHERE tenant_id = ?1
                "#,
            )
            .bind(&key_tenant)
            .execute(&pool)
            .await
            .context("failed to clear tenant cache")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to clear tenant cache: {err:?}");
        }
    }

    /// Clear all cached data.
    pub fn clear_all(&self) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for clear_all: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for clear_all (cache may not be initialized): {err:?}");
                return;
            }
        };

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                DELETE FROM read_models
                "#,
            )
            .execute(&pool)
            .await
            .context("failed to clear all cache")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to clear all cache: {err:?}");
        }
    }

    /// Generic helper: cache an arbitrary read model.
    ///
    /// This is kept internal; higher-level APIs (such as inventory) should wrap it.
    async fn cache_read_model<T>(
        &self,
        tenant_id: &TenantId,
        aggregate_type: &str,
        aggregate_id: &str,
        model: &T,
    ) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        self.cache_read_model_with_version(tenant_id, aggregate_type, aggregate_id, model, None).await
    }

    /// Cache a read model with optional version metadata.
    pub(crate) async fn cache_read_model_with_version<T>(
        &self,
        tenant_id: &TenantId,
        aggregate_type: &str,
        aggregate_id: &str,
        model: &T,
        version: Option<u64>,
    ) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        let pool = self.get_pool().await?;

        let key_tenant = tenant_id.to_string();
        let payload =
            serde_json::to_string(model).context("failed to serialize read model for cache")?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO read_models (
                tenant_id,
                aggregate_type,
                aggregate_id,
                data,
                cached_at,
                version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(tenant_id, aggregate_type, aggregate_id)
            DO UPDATE SET
                data = excluded.data,
                cached_at = excluded.cached_at,
                version = excluded.version
            "#,
        )
        .bind(&key_tenant)
        .bind(aggregate_type)
        .bind(aggregate_id)
        .bind(&payload)
        .bind(&now)
        .bind(version.map(|v| v as i64))
        .execute(&pool)
        .await
        .context("failed to upsert read model in cache")?;

        Ok(())
    }

    /// Get the version of a cached read model.
    pub(crate) fn get_read_model_version(
        &self,
        tenant_id: TenantId,
        aggregate_type: &str,
        aggregate_id: &AggregateId,
    ) -> anyhow::Result<Option<u64>> {
        let rt = Runtime::new().context("failed to create runtime for get_read_model_version")?;
        let pool = rt.block_on(async { self.get_pool().await })?;

        let key_tenant = tenant_id.to_string();
        let key_agg_id = aggregate_id.to_string();

        rt.block_on(async move {
            let row = sqlx::query(
                r#"
                SELECT version
                FROM read_models
                WHERE tenant_id = ?1
                  AND aggregate_type = ?2
                  AND aggregate_id = ?3
                "#,
            )
            .bind(&key_tenant)
            .bind(aggregate_type)
            .bind(&key_agg_id)
            .fetch_optional(&pool)
            .await
            .context("failed to fetch read model version from cache")?;

            let row = match row {
                Some(row) => row,
                None => return Ok(None),
            };

            let version: Option<i64> = row.try_get("version")?;
            Ok(version.map(|v| v as u64))
        })
    }

    /// Generic helper: get an arbitrary read model (ignores staleness).
    fn get_read_model<T>(
        &self,
        tenant_id: &TenantId,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> anyhow::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let rt = Runtime::new().context("failed to create runtime for get_read_model")?;
        let pool = rt.block_on(async { self.get_pool().await })?;

        let key_tenant = tenant_id.to_string();

        let row = rt.block_on(async move {
            let row = sqlx::query(
                r#"
                SELECT data
                FROM read_models
                WHERE tenant_id = ?1
                  AND aggregate_type = ?2
                  AND aggregate_id = ?3
                "#,
            )
            .bind(&key_tenant)
            .bind(aggregate_type)
            .bind(aggregate_id)
            .fetch_optional(&pool)
            .await
            .context("failed to fetch read model from cache")?;

            Ok::<_, anyhow::Error>(row)
        })?;

        let row = match row {
            Some(row) => row,
            None => return Ok(None),
        };

        let data: String = row.try_get("data")?;
        let model =
            serde_json::from_str(&data).context("failed to deserialize cached read model")?;

        Ok(Some(model))
    }
}

impl Default for LocalCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve the path to the SQLite cache database:
/// `{app_data_dir}/forgeerp/cache.db`.
fn cache_db_path() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|mut h| {
            h.push(".local");
            h.push("share");
            h
        }))
        .context("failed to resolve OS app data directory - tried data_dir() and home_dir()/.local/share")?;

    let mut dir = base;
    dir.push("forgeerp");

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create cache directory at {:?}", dir))?;

    dir.push("cache.db");

    Ok(dir)
}


