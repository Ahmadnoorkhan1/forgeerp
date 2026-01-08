//! Offline-first command queue persisted in SQLite.
//!
//! This module provides a `CommandQueue` abstraction that stores commands in a
//! durable SQLite table (`command_queue`). Commands are scoped by `TenantId`
//! and can be safely retried when connectivity is restored.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use forgeerp_core::{AggregateId, TenantId};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tokio::runtime::Runtime;
use uuid::Uuid;

// Re-export from shared types module
pub use crate::types::{CommandStatus, QueuedCommand};

// Implement sqlx::Type for CommandStatus (backend-only)
impl sqlx::Type<sqlx::Sqlite> for CommandStatus {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <&str as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for CommandStatus {
    fn decode(
        value: sqlx::sqlite::SqliteValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <&str as sqlx::Decode<'r, sqlx::Sqlite>>::decode(value)?;
        match s {
            "Pending" => Ok(CommandStatus::Pending),
            "Syncing" => Ok(CommandStatus::Syncing),
            "Synced" => Ok(CommandStatus::Synced),
            "Failed" => Ok(CommandStatus::Failed),
            _ => Err(format!("invalid CommandStatus: {}", s).into()),
        }
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for CommandStatus {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let s = self.as_str();
        <&str as sqlx::Encode<'q, sqlx::Sqlite>>::encode_by_ref(&s, buf)
    }
}

/// SQLite-backed command queue.
///
/// This struct is cheap to clone and is safe to share across threads.
#[derive(Debug, Clone)]
pub struct CommandQueue {
    pool: Arc<tokio::sync::Mutex<Option<SqlitePool>>>,
}

impl CommandQueue {
    /// Create a new CommandQueue (lazy initialization).
    ///
    /// The database will be initialized on first use.
    pub fn new() -> Self {
        Self {
            pool: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Initialize the database connection (called lazily on first use).
    async fn ensure_initialized(&self) -> anyhow::Result<()> {
        let mut pool_guard = self.pool.lock().await;
        if pool_guard.is_some() {
            return Ok(());
        }

        let db_path = command_db_path()
            .context("failed to determine command queue DB path - ensure app data directory is accessible")?;
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create command queue directory at {:?}", parent))?;
        }
        
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());

        let pool = SqlitePool::connect(&db_url)
            .await
            .with_context(|| format!("failed to create SQLite pool for CommandQueue at {:?}", db_path))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS command_queue (
                id            TEXT PRIMARY KEY,
                tenant_id     TEXT NOT NULL,
                command_type  TEXT NOT NULL,
                aggregate_id  TEXT NOT NULL,
                payload       TEXT NOT NULL,
                status        TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                synced_at     TEXT NULL,
                error         TEXT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .context("failed to create command_queue table")?;

        *pool_guard = Some(pool);
        Ok(())
    }

    /// Get the pool, initializing if necessary.
    async fn get_pool(&self) -> anyhow::Result<SqlitePool> {
        self.ensure_initialized().await?;
        let pool_guard = self.pool.lock().await;
        Ok(pool_guard.as_ref().unwrap().clone())
    }


    /// Enqueue a new command for the given tenant and aggregate.
    pub fn enqueue(
        &self,
        tenant_id: TenantId,
        command_type: String,
        aggregate_id: AggregateId,
        payload: Value,
    ) -> QueuedCommand {
        let id = Uuid::now_v7();
        let created_at = Utc::now();
        let status = CommandStatus::Pending;

        let cmd = QueuedCommand {
            id,
            tenant_id,
            command_type: command_type.clone(),
            aggregate_id,
            payload: payload.clone(),
            status,
            created_at,
            synced_at: None,
            error: None,
        };

        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for enqueue: {err:?}");
                return cmd;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for enqueue (queue may not be initialized): {err:?}");
                return cmd;
            }
        };

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                INSERT INTO command_queue (
                    id,
                    tenant_id,
                    command_type,
                    aggregate_id,
                    payload,
                    status,
                    created_at,
                    synced_at,
                    error
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL)
                "#,
            )
            .bind(id.to_string())
            .bind(tenant_id.to_string())
            .bind(&command_type)
            .bind(aggregate_id.to_string())
            .bind(payload.to_string())
            .bind(status.as_str())
            .bind(created_at.to_rfc3339())
            .execute(&pool)
            .await
            .context("failed to insert queued command")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to enqueue command: {err:?}");
        }

        cmd
    }

    /// List all pending commands for a tenant (status = Pending or Failed for retry).
    pub fn list_pending(&self, tenant_id: TenantId) -> Vec<QueuedCommand> {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for list_pending: {err:?}");
                return Vec::new();
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for list_pending (queue may not be initialized): {err:?}");
                return Vec::new();
            }
        };

        let key_tenant = tenant_id.to_string();

        let result = rt.block_on(async move {
            let rows = sqlx::query(
                r#"
                SELECT
                    id,
                    tenant_id,
                    command_type,
                    aggregate_id,
                    payload,
                    status,
                    created_at,
                    synced_at,
                    error
                FROM command_queue
                WHERE tenant_id = ?1
                  AND status IN ('Pending', 'Failed')
                ORDER BY created_at ASC
                "#,
            )
            .bind(&key_tenant)
            .fetch_all(&pool)
            .await
            .context("failed to list pending commands")?;

            let mut cmds = Vec::with_capacity(rows.len());
            for row in rows {
                cmds.push(row_to_command(row)?);
            }

            Ok::<Vec<QueuedCommand>, anyhow::Error>(cmds)
        });

        match result {
            Ok(cmds) => cmds,
            Err(err) => {
                tracing::error!("failed to list pending commands: {err:?}");
                Vec::new()
            }
        }
    }

    /// Mark a command as syncing.
    pub fn mark_syncing(&self, id: Uuid) {
        self.update_status(id, CommandStatus::Syncing, None);
    }

    /// Mark a command as successfully synced.
    pub fn mark_synced(&self, id: Uuid) {
        let now = Some(Utc::now());
        self.update_status(id, CommandStatus::Synced, now);
    }

    /// Mark a command as failed with an error message.
    pub fn mark_failed(&self, id: Uuid, error: String) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for mark_failed: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for mark_failed (queue may not be initialized): {err:?}");
                return;
            }
        };

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                UPDATE command_queue
                SET status = 'Failed',
                    error = ?2
                WHERE id = ?1
                "#,
            )
            .bind(id.to_string())
            .bind(error)
            .execute(&pool)
            .await
            .context("failed to mark command as failed")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to mark command as failed: {err:?}");
        }
    }

    /// Retry a failed command by moving it back to Pending and clearing the error.
    pub fn retry_failed(&self, id: Uuid) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for retry_failed: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for retry_failed (queue may not be initialized): {err:?}");
                return;
            }
        };

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                UPDATE command_queue
                SET status = 'Pending',
                    error = NULL
                WHERE id = ?1
                  AND status = 'Failed'
                "#,
            )
            .bind(id.to_string())
            .execute(&pool)
            .await
            .context("failed to retry failed command")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to retry failed command: {err:?}");
        }
    }

    /// Clear successfully synced commands older than 7 days.
    pub fn clear_synced(&self) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for clear_synced: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for clear_synced (queue may not be initialized): {err:?}");
                return;
            }
        };

        let cutoff = (Utc::now() - Duration::days(7)).to_rfc3339();

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                DELETE FROM command_queue
                WHERE status = 'Synced'
                  AND synced_at IS NOT NULL
                  AND synced_at < ?1
                "#,
            )
            .bind(&cutoff)
            .execute(&pool)
            .await
            .context("failed to clear synced commands")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to clear synced commands: {err:?}");
        }
    }

    fn update_status(&self, id: Uuid, status: CommandStatus, synced_at: Option<DateTime<Utc>>) {
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                tracing::error!("failed to create runtime for update_status: {err:?}");
                return;
            }
        };

        let pool = match rt.block_on(async { self.get_pool().await }) {
            Ok(pool) => pool,
            Err(err) => {
                tracing::warn!("failed to get pool for update_status (queue may not be initialized): {err:?}");
                return;
            }
        };

        let synced_at_str = synced_at.map(|dt| dt.to_rfc3339());
        let status_str = status.as_str();

        if let Err(err) = rt.block_on(async move {
            sqlx::query(
                r#"
                UPDATE command_queue
                SET status = ?2,
                    synced_at = ?3
                WHERE id = ?1
                "#,
            )
            .bind(id.to_string())
            .bind(status_str)
            .bind(synced_at_str)
            .execute(&pool)
            .await
            .context("failed to update command status")?;

            Ok::<(), anyhow::Error>(())
        }) {
            tracing::error!("failed to update command status: {err:?}");
        }
    }
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a database row into a `QueuedCommand`.
fn row_to_command(row: sqlx::sqlite::SqliteRow) -> anyhow::Result<QueuedCommand> {
    let id_str: String = row.try_get("id")?;
    let id = Uuid::parse_str(&id_str).context("invalid UUID in command_queue.id")?;

    let tenant_str: String = row.try_get("tenant_id")?;
    let tenant_id = tenant_str
        .parse::<TenantId>()
        .context("invalid tenant_id in command_queue")?;

    let command_type: String = row.try_get("command_type")?;

    let aggregate_str: String = row.try_get("aggregate_id")?;
    let aggregate_id = aggregate_str
        .parse::<AggregateId>()
        .context("invalid aggregate_id in command_queue")?;

    let payload_str: String = row.try_get("payload")?;
    let payload: Value =
        serde_json::from_str(&payload_str).context("invalid JSON payload in command_queue")?;

    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "Pending" => CommandStatus::Pending,
        "Syncing" => CommandStatus::Syncing,
        "Synced" => CommandStatus::Synced,
        "Failed" => CommandStatus::Failed,
        other => {
            return Err(anyhow::anyhow!(
                "unknown command status '{}' in command_queue",
                other
            ))
        }
    };

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .context("invalid created_at in command_queue")?;

    let synced_at_str: Option<String> = row.try_get("synced_at")?;
    let synced_at = if let Some(s) = synced_at_str {
        Some(
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .context("invalid synced_at in command_queue")?,
        )
    } else {
        None
    };

    let error: Option<String> = row.try_get("error")?;

    Ok(QueuedCommand {
        id,
        tenant_id,
        command_type,
        aggregate_id,
        payload,
        status,
        created_at,
        synced_at,
        error,
    })
}

/// Resolve the path to the SQLite database for the command queue.
///
/// We reuse the same physical database file as the local cache:
/// `{app_data_dir}/forgeerp/cache.db`.
fn command_db_path() -> anyhow::Result<PathBuf> {
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
        .with_context(|| format!("failed to create command queue directory at {:?}", dir))?;

    dir.push("cache.db");

    Ok(dir)
}


