//! Tauri commands for frontend integration.

use std::sync::Arc;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_inventory::InventoryItemId;
use tauri::State;

use crate::cache::{LocalCache, InventoryReadModel};
use crate::command_queue::{CommandQueue, QueuedCommand};
use crate::offline::{OfflineMode, ConnectivityState};
use crate::sync::SyncClient;
use crate::sync_manager::{SyncManager, ConflictResolution, SyncReadModelResult, SyncResult};

/// Application state shared across Tauri commands.
#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<LocalCache>,
    pub command_queue: Arc<CommandQueue>,
    pub sync_client: Arc<SyncClient>,
    pub sync_manager: Arc<SyncManager>,
    pub offline_mode: Arc<tokio::sync::Mutex<OfflineMode>>,
}

impl AppState {
    /// Create a new AppState with default configuration.
    ///
    /// Database connections will be initialized lazily on first use.
    pub fn new(api_url: String) -> Self {
        let cache = Arc::new(LocalCache::new());
        let command_queue = Arc::new(CommandQueue::new());
        let sync_client = Arc::new(SyncClient::new(api_url.clone()));
        let sync_manager = Arc::new(SyncManager::new(
            api_url.clone(),
            command_queue.clone(),
            cache.clone(),
        ));
        let offline_mode = Arc::new(tokio::sync::Mutex::new(OfflineMode::new(api_url)));

        Self {
            cache,
            command_queue,
            sync_client,
            sync_manager,
            offline_mode,
        }
    }

    /// Create AppState with authentication token.
    ///
    /// Database connections will be initialized lazily on first use.
    pub fn with_token(api_url: String, token: String) -> Self {
        let cache = Arc::new(LocalCache::new());
        let command_queue = Arc::new(CommandQueue::new());
        let sync_client = Arc::new(SyncClient::with_token(api_url.clone(), token.clone()));
        let sync_manager = Arc::new(SyncManager::with_token(
            api_url.clone(),
            token,
            command_queue.clone(),
            cache.clone(),
        ));
        let offline_mode = Arc::new(tokio::sync::Mutex::new(OfflineMode::new(api_url)));

        Self {
            cache,
            command_queue,
            sync_client,
            sync_manager,
            offline_mode,
        }
    }

}

/// List all cached inventory items for a tenant.
#[tauri::command]
pub async fn list_inventory_items(
    tenant_id: String,
    _state: State<'_, AppState>,
) -> Result<Vec<InventoryReadModel>, String> {
    let tenant_id = tenant_id
        .parse::<TenantId>()
        .map_err(|e| format!("Invalid tenant_id: {}", e))?;

    // For now, we'll need to query all cached inventory items
    // This is a simplified implementation - in production, you'd want
    // a method to list all cached items of a type
    tracing::info!("Listing inventory items for tenant: {}", tenant_id);

    // Since we don't have a list_all method, we'll return an empty vec for now
    // In a full implementation, you'd query the cache for all inventory_item types
    Ok(Vec::new())
}

/// Adjust stock for an inventory item.
///
/// If offline, queues the command. If online, attempts to sync immediately.
#[tauri::command]
pub async fn adjust_stock(
    tenant_id: String,
    item_id: String,
    delta: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let tenant_id = tenant_id
        .parse::<TenantId>()
        .map_err(|e| format!("Invalid tenant_id: {}", e))?;

    let aggregate_id = item_id
        .parse::<AggregateId>()
        .map_err(|e| format!("Invalid item_id: {}", e))?;

    let item_id = InventoryItemId::new(aggregate_id);

    // Check connectivity
    let offline_guard = state.offline_mode.lock().await;
    let is_offline = offline_guard.is_offline();
    drop(offline_guard);

    // Create command payload
    let payload = serde_json::json!({
        "delta": delta,
    });

    // Queue the command
    let queued = state.command_queue.enqueue(
        tenant_id,
        "inventory.adjust_stock".to_string(),
        aggregate_id,
        payload,
    );

    tracing::info!(
        "Queued stock adjustment command: {} for item {} (delta: {})",
        queued.id,
        item_id,
        delta
    );

    // If online, attempt immediate sync
    if !is_offline {
        if let Err(e) = state
            .sync_manager
            .sync_command(&queued)
            .await
        {
            tracing::warn!("Failed to sync command immediately: {}", e);
            // Command is queued, will sync later
        } else {
            state.command_queue.mark_synced(queued.id);
            tracing::info!("Command synced immediately");
        }
    }

    Ok(())
}

/// Trigger a full sync operation for the current tenant.
#[tauri::command]
pub async fn sync_now(
    tenant_id: String,
    state: State<'_, AppState>,
) -> Result<SyncResult, String> {
    let tenant_id = tenant_id
        .parse::<TenantId>()
        .map_err(|e| format!("Invalid tenant_id: {}", e))?;

    tracing::info!("Starting manual sync for tenant: {}", tenant_id);

    // Check connectivity first
    let mut offline_guard = state.offline_mode.lock().await;
    if !state.sync_client.check_connectivity().await {
        offline_guard.set_offline();
        return Err("Client is offline. Cannot sync.".to_string());
    }
    offline_guard.set_online();
    drop(offline_guard);

    // Perform sync
    let result = state
        .sync_manager
        .sync_tenant(tenant_id)
        .await
        .map_err(|e| format!("Sync failed: {}", e))?;

    tracing::info!(
        "Sync complete: {} commands, {} read models, {} conflicts",
        result.synced_commands.len(),
        result.synced_read_models.len(),
        result.conflicts.len()
    );

    Ok(result)
}

/// Get the current connectivity state.
#[tauri::command]
pub async fn get_connectivity_state(
    state: State<'_, AppState>,
) -> Result<ConnectivityState, String> {
    let offline_guard = state.offline_mode.lock().await;
    Ok(offline_guard.state())
}

/// List all pending commands for a tenant.
#[tauri::command]
pub async fn list_pending_commands(
    tenant_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<QueuedCommand>, String> {
    let tenant_id = tenant_id
        .parse::<TenantId>()
        .map_err(|e| format!("Invalid tenant_id: {}", e))?;

    let commands = state.command_queue.list_pending(tenant_id);

    Ok(commands)
}

/// Resolve a conflict by applying the specified resolution strategy.
#[tauri::command]
pub async fn resolve_conflict(
    conflict_aggregate_type: String,
    conflict_aggregate_id: String,
    resolution: ConflictResolution,
    tenant_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let tenant_id = tenant_id
        .parse::<TenantId>()
        .map_err(|e| format!("Invalid tenant_id: {}", e))?;

    let aggregate_id = conflict_aggregate_id
        .parse::<AggregateId>()
        .map_err(|e| format!("Invalid aggregate_id: {}", e))?;

    tracing::info!(
        "Resolving conflict for {} {} with strategy: {:?}",
        conflict_aggregate_type,
        aggregate_id,
        resolution
    );

    match resolution {
        ConflictResolution::UseRemote => {
            // Fetch and cache the remote version
            let result = state
                .sync_manager
                .sync_read_model(tenant_id, &conflict_aggregate_type, &aggregate_id)
                .await
                .map_err(|e| format!("Failed to sync remote version: {}", e))?;

            if let Some(sync_result) = result {
                match sync_result {
                    SyncReadModelResult::Synced(_) => {
                        tracing::info!("Successfully applied UseRemote resolution");
                    }
                    SyncReadModelResult::Conflict(_) => {
                        return Err("Conflict still exists after resolution attempt".to_string());
                    }
                }
            }
        }
        ConflictResolution::UseLocal => {
            // Keep local version - no action needed
            // In a full implementation, would POST local version to API
            tracing::info!("Keeping local version (UseLocal resolution)");
        }
        ConflictResolution::Merge => {
            // Future: implement merge logic
            return Err("Merge resolution not yet implemented".to_string());
        }
    }

    Ok(())
}

