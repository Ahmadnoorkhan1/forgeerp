//! Background worker for periodic command synchronization.

use std::sync::Arc;
use std::time::Duration;

use forgeerp_core::TenantId;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::AppState;
use crate::sync_manager::SyncError;

/// Background sync worker that periodically syncs pending commands.
pub struct SyncWorker {
    app_handle: AppHandle,
    state: Arc<AppState>,
    shutdown: Arc<tokio::sync::Notify>,
    active_tenants: Arc<tokio::sync::RwLock<Vec<TenantId>>>,
}

/// Event payload for sync completion.
#[derive(Debug, Clone, Serialize)]
pub struct SyncCompletedEvent {
    pub synced_commands: usize,
    pub synced_read_models: usize,
    pub conflicts: usize,
}

/// Event payload for sync conflict.
#[derive(Debug, Clone, Serialize)]
pub struct SyncConflictEvent {
    pub conflicts: Vec<crate::sync_manager::Conflict>,
}

/// Event payload for sync failure.
#[derive(Debug, Clone, Serialize)]
pub struct SyncFailedEvent {
    pub error: String,
}

impl SyncWorker {
    /// Create a new sync worker.
    pub fn new(app_handle: AppHandle, state: Arc<AppState>) -> Self {
        Self {
            app_handle,
            state,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            active_tenants: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Add a tenant to the list of active tenants to sync.
    pub async fn add_tenant(&self, tenant_id: TenantId) {
        let mut tenants = self.active_tenants.write().await;
        if !tenants.contains(&tenant_id) {
            tenants.push(tenant_id);
            tracing::info!("Added tenant {} to sync worker", tenant_id);
        }
    }

    /// Remove a tenant from the list of active tenants.
    pub async fn remove_tenant(&self, tenant_id: TenantId) {
        let mut tenants = self.active_tenants.write().await;
        tenants.retain(|&id| id != tenant_id);
        tracing::info!("Removed tenant {} from sync worker", tenant_id);
    }

    /// Start the background sync worker.
    ///
    /// This spawns a background task that:
    /// - Checks connectivity every 30 seconds
    /// - Syncs pending commands when online
    /// - Emits Tauri events for sync status
    /// - Respects graceful shutdown signals
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let shutdown = self.shutdown.clone();
        let app_handle = self.app_handle.clone();
        let state = self.state.clone();
        let active_tenants = self.active_tenants.clone();

        tokio::spawn(async move {
            tracing::info!("Background sync worker started");

            let mut sync_interval = tokio::time::interval(Duration::from_secs(30));
            sync_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            let mut consecutive_failures = 0u32;
            let mut backoff_duration = Duration::from_secs(1);

            loop {
                tokio::select! {
                    _ = shutdown.notified() => {
                        tracing::info!("Background sync worker received shutdown signal");
                        break;
                    }
                    _ = sync_interval.tick() => {
                        // Check if offline mode is active
                        let offline_guard = state.offline_mode.lock().await;
                        let is_offline = offline_guard.is_offline();
                        drop(offline_guard);

                        if is_offline {
                            tracing::debug!("Skipping sync - offline mode is active");
                            continue;
                        }

                        // Check connectivity
                        let is_online = state.sync_client.check_connectivity().await;
                        if !is_online {
                            tracing::debug!("Skipping sync - no connectivity");
                            let mut offline_guard = state.offline_mode.lock().await;
                            offline_guard.set_offline();
                            continue;
                        }

                        // Update offline mode to online
                        {
                            let mut offline_guard = state.offline_mode.lock().await;
                            offline_guard.set_online();
                        }

                        // Get all active tenants
                        let tenants = {
                            let tenants_guard = active_tenants.read().await;
                            tenants_guard.clone()
                        };

                        if tenants.is_empty() {
                            tracing::debug!("No active tenants to sync");
                            continue;
                        }

                        tracing::debug!("Syncing {} active tenant(s)", tenants.len());

                        // Sync each tenant
                        for tenant_id in tenants {
                            // Check for pending commands
                            let pending = state.command_queue.list_pending(tenant_id);
                            if pending.is_empty() {
                                tracing::debug!("No pending commands for tenant {}", tenant_id);
                                continue;
                            }

                            tracing::info!(
                                "Syncing {} pending commands for tenant {}",
                                pending.len(),
                                tenant_id
                            );

                            // Perform sync
                            match Self::sync_tenant_internal(
                                &app_handle,
                                &state,
                                tenant_id,
                            )
                            .await
                            {
                                Ok(()) => {
                                    consecutive_failures = 0;
                                    backoff_duration = Duration::from_secs(1);
                                }
                                Err(e) => {
                                    consecutive_failures += 1;
                                    tracing::warn!(
                                        "Sync failed for tenant {} (failure count: {}): {}",
                                        tenant_id,
                                        consecutive_failures,
                                        e
                                    );

                                    // Apply exponential backoff
                                    if consecutive_failures > 0 {
                                        let backoff = std::cmp::min(
                                            backoff_duration * (1 << consecutive_failures.min(5)),
                                            Duration::from_secs(300), // Cap at 5 minutes
                                        );
                                        tracing::debug!(
                                            "Applying backoff of {:?} before next sync attempt",
                                            backoff
                                        );
                                        backoff_duration = backoff;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            tracing::info!("Background sync worker stopped");
        })
    }

    /// Internal sync method (static to avoid borrowing issues in async closure).
    async fn sync_tenant_internal(
        app_handle: &AppHandle,
        state: &Arc<AppState>,
        tenant_id: TenantId,
    ) -> Result<(), String> {
        tracing::info!("Worker syncing tenant: {}", tenant_id);

        // Check if offline mode is active
        let offline_guard = state.offline_mode.lock().await;
        if offline_guard.is_offline() {
            return Err("Offline mode is active".to_string());
        }
        drop(offline_guard);

        // Check connectivity
        if !state.sync_client.check_connectivity().await {
            let mut offline_guard = state.offline_mode.lock().await;
            offline_guard.set_offline();
            return Err("No connectivity".to_string());
        }

        // Update offline mode to online
        {
            let mut offline_guard = state.offline_mode.lock().await;
            offline_guard.set_online();
        }

        // Perform sync
        match state.sync_manager.sync_tenant(tenant_id).await {
            Ok(result) => {
                // Check for conflicts
                if !result.conflicts.is_empty() {
                    tracing::warn!(
                        "Sync completed with {} conflicts for tenant {}",
                        result.conflicts.len(),
                        tenant_id
                    );

                    // Emit conflict event
                    let _ = app_handle.emit(
                        "sync:conflict",
                        SyncConflictEvent {
                            conflicts: result.conflicts.clone(),
                        },
                    );

                    // Pause syncing for this tenant until conflicts are resolved
                    // In production, you'd track which tenants have unresolved conflicts
                    return Ok(());
                }

                tracing::info!(
                    "Sync completed successfully for tenant {}: {} commands, {} read models",
                    tenant_id,
                    result.synced_commands.len(),
                    result.synced_read_models.len()
                );

                // Emit completion event
                let _ = app_handle.emit(
                    "sync:completed",
                    SyncCompletedEvent {
                        synced_commands: result.synced_commands.len(),
                        synced_read_models: result.synced_read_models.len(),
                        conflicts: result.conflicts.len(),
                    },
                );

                Ok(())
            }
            Err(SyncError::Offline) => {
                let mut offline_guard = state.offline_mode.lock().await;
                offline_guard.set_offline();
                Err("Client is offline".to_string())
            }
            Err(SyncError::Network(e)) => {
                tracing::warn!("Network error during sync: {}", e);
                let mut offline_guard = state.offline_mode.lock().await;
                offline_guard.set_offline();

                // Emit failure event
                let _ = app_handle.emit("sync:failed", SyncFailedEvent { error: e.clone() });

                Err(format!("Network error: {}", e))
            }
            Err(e) => {
                tracing::error!("Sync failed for tenant {}: {}", tenant_id, e);

                // Emit failure event
                let _ = app_handle.emit("sync:failed", SyncFailedEvent {
                    error: e.to_string(),
                });

                Err(format!("Sync failed: {}", e))
            }
        }
    }

    /// Sync pending commands for a specific tenant.
    ///
    /// This is called by the worker or can be called manually.
    pub async fn sync_tenant(&self, tenant_id: TenantId) -> Result<(), String> {
        Self::sync_tenant_internal(&self.app_handle, &self.state, tenant_id).await
    }

    /// Request graceful shutdown of the worker.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }
}

/// Helper to sync a tenant with exponential backoff retry.
pub async fn sync_tenant_with_backoff(
    worker: &SyncWorker,
    tenant_id: TenantId,
    max_retries: u32,
) -> Result<(), String> {
    let mut delay = Duration::from_secs(1);
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match worker.sync_tenant(tenant_id).await {
            Ok(()) => {
                if attempt > 0 {
                    tracing::info!(
                        "Sync succeeded for tenant {} after {} retries",
                        tenant_id,
                        attempt
                    );
                }
                return Ok(());
            }
            Err(e) => {
                last_error = Some(e.clone());
                if attempt < max_retries {
                    tracing::warn!(
                        "Sync attempt {} failed for tenant {}, retrying in {:?}...",
                        attempt + 1,
                        tenant_id,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(60)); // Cap at 60s
                } else {
                    tracing::error!(
                        "Sync failed for tenant {} after {} retries",
                        tenant_id,
                        max_retries
                    );
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "Unknown error".to_string()))
}

