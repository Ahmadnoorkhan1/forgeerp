//! Shared types for both backend and frontend (WASM-compatible).
//!
//! This module contains type definitions that are used by both the backend
//! and the frontend. These types must not depend on backend-only dependencies
//! like `tokio`, `sqlx`, etc.

use chrono::{DateTime, Utc};
use forgeerp_core::{AggregateId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Simplified inventory read model (matches API response shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReadModel {
    pub id: String,
    pub name: String,
    pub quantity: i64,
}

/// Status of a queued command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandStatus {
    Pending,
    Syncing,
    Synced,
    Failed,
}

impl CommandStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandStatus::Pending => "Pending",
            CommandStatus::Syncing => "Syncing",
            CommandStatus::Synced => "Synced",
            CommandStatus::Failed => "Failed",
        }
    }
}

/// A command queued for offline execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedCommand {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub command_type: String,
    pub aggregate_id: AggregateId,
    pub payload: Value,
    pub status: CommandStatus,
    pub created_at: DateTime<Utc>,
    pub synced_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

/// Connectivity state of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectivityState {
    /// Online and connected to the API.
    Online,
    /// Offline (network unreachable or API unavailable).
    Offline,
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictResolution {
    /// Overwrite remote with local version.
    UseLocal,
    /// Discard local, use remote version.
    UseRemote,
    /// Merge strategies (for future implementation).
    Merge,
}

/// A detected conflict between local and remote versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub aggregate_type: String,
    pub aggregate_id: AggregateId,
    pub local_version: Option<u64>,
    pub remote_version: u64,
    pub resolution: ConflictResolution,
}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub synced_commands: Vec<Uuid>,
    pub synced_read_models: Vec<(String, AggregateId)>,
    pub conflicts: Vec<Conflict>,
}

