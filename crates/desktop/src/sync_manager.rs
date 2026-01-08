//! Bi-directional sync manager with conflict detection.
//!
//! This module provides a `SyncManager` that:
//! - Syncs queued commands to the API (POST to appropriate endpoints)
//! - Fetches latest read models from the API
//! - Detects conflicts when local version < remote version
//! - Handles retries with exponential backoff
//! - Preserves command ordering

#[cfg(feature = "tauri")]
use std::sync::Arc;
#[cfg(feature = "tauri")]
use std::time::Duration;

#[cfg(feature = "tauri")]
use chrono::Utc;
#[cfg(feature = "tauri")]
use forgeerp_core::{AggregateId, TenantId};
#[cfg(feature = "tauri")]
use serde_json::Value;

#[cfg(feature = "tauri")]
use crate::cache::LocalCache;
#[cfg(feature = "tauri")]
use crate::command_queue::{CommandQueue, QueuedCommand};

// Re-export from shared types module
pub use crate::types::{Conflict, ConflictResolution, SyncResult};

/// Enhanced sync client with bi-directional sync and conflict detection.
#[cfg(feature = "tauri")]
pub struct SyncManager {
    pub(crate) api_url: String,
    token: Option<String>,
    command_queue: Arc<CommandQueue>,
    cache: Arc<LocalCache>,
}

#[cfg(feature = "tauri")]
impl SyncManager {
    /// Create a new sync manager.
    pub fn new(
        api_url: String,
        command_queue: Arc<CommandQueue>,
        cache: Arc<LocalCache>,
    ) -> Self {
        Self {
            api_url,
            token: None,
            command_queue,
            cache,
        }
    }

    /// Create a sync manager with authentication token.
    pub fn with_token(
        api_url: String,
        token: String,
        command_queue: Arc<CommandQueue>,
        cache: Arc<LocalCache>,
    ) -> Self {
        Self {
            api_url,
            token: Some(token),
            command_queue,
            cache,
        }
    }

    /// Perform a full bi-directional sync for a tenant.
    ///
    /// This method:
    /// 1. Syncs all pending commands to the API (in created_at order)
    /// 2. Fetches latest read models for aggregates referenced by commands
    /// 3. Detects conflicts when local version < remote version
    /// 4. Returns a `SyncResult` with sync status and conflicts
    pub async fn sync_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<SyncResult, SyncError> {
        tracing::info!("Starting sync for tenant: {}", tenant_id);

        let mut result = SyncResult {
            synced_commands: Vec::new(),
            synced_read_models: Vec::new(),
            conflicts: Vec::new(),
        };

        // Step 1: Sync pending commands to API (preserve ordering)
        let pending = self.command_queue.list_pending(tenant_id);
        tracing::info!("Found {} pending commands to sync", pending.len());

        for cmd in pending {
            match self.sync_command(&cmd).await {
                Ok(()) => {
                    self.command_queue.mark_synced(cmd.id);
                    result.synced_commands.push(cmd.id);
                    tracing::info!("Successfully synced command: {}", cmd.id);

                    // After successful command sync, fetch the updated read model
                    let agg_type = self.aggregate_type_from_command_type(&cmd.command_type);
                    if let Ok(Some(read_model_result)) = self
                        .sync_read_model(tenant_id, &agg_type, &cmd.aggregate_id)
                        .await
                    {
                        match read_model_result {
                            SyncReadModelResult::Conflict(conflict) => {
                                result.conflicts.push(conflict);
                            }
                            SyncReadModelResult::Synced((agg_type, agg_id)) => {
                                result.synced_read_models.push((agg_type, agg_id));
                            }
                        }
                    }
                }
                Err(SyncError::Network(_)) | Err(SyncError::Offline) => {
                    // Network error - mark as syncing and will retry later
                    self.command_queue.mark_syncing(cmd.id);
                    tracing::warn!("Network error syncing command {}, will retry", cmd.id);
                }
                Err(e) => {
                    // Other errors - mark as failed
                    self.command_queue.mark_failed(cmd.id, e.to_string());
                    tracing::error!("Failed to sync command {}: {}", cmd.id, e);
                }
            }
        }

        // Step 2: Fetch latest read models for known aggregates (optional full sync)
        // This could be expanded to sync all cached read models

        tracing::info!(
            "Sync complete: {} commands synced, {} read models synced, {} conflicts",
            result.synced_commands.len(),
            result.synced_read_models.len(),
            result.conflicts.len()
        );

        Ok(result)
    }

    /// Sync a single command to the API with exponential backoff retry.
    pub async fn sync_command(&self, cmd: &QueuedCommand) -> Result<(), SyncError> {
        let client = reqwest::Client::new();
        let endpoint = self.command_endpoint(&cmd.command_type, &cmd.aggregate_id);
        let url = format!("{}{}", self.api_url, endpoint);

        let mut req = client.post(&url).json(&cmd.payload);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        // Exponential backoff retry logic
        let max_retries = 3;
        let mut delay = Duration::from_secs(1);

        for attempt in 0..=max_retries {
            match req.try_clone() {
                Some(cloned_req) => {
                    match cloned_req.send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                tracing::debug!(
                                    "Command {} synced successfully on attempt {}",
                                    cmd.id,
                                    attempt + 1
                                );
                                return Ok(());
                            } else if resp.status() == reqwest::StatusCode::CONFLICT {
                                // Optimistic concurrency conflict
                                let error_text = resp.text().await.unwrap_or_default();
                                return Err(SyncError::Conflict(error_text));
                            } else {
                                let status = resp.status();
                                let error_text = resp.text().await.unwrap_or_default();
                                if attempt < max_retries {
                                    tracing::warn!(
                                        "Command {} failed with {} on attempt {}, retrying...",
                                        cmd.id,
                                        status,
                                        attempt + 1
                                    );
                                    tokio::time::sleep(delay).await;
                                    delay *= 2; // Exponential backoff
                                    continue;
                                } else {
                                    return Err(SyncError::Api(
                                        status.as_u16(),
                                        error_text,
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            if attempt < max_retries {
                                tracing::warn!(
                                    "Network error syncing command {} on attempt {}: {}, retrying...",
                                    cmd.id,
                                    attempt + 1,
                                    e
                                );
                                tokio::time::sleep(delay).await;
                                delay *= 2;
                                continue;
                            } else {
                                return Err(SyncError::Network(e.to_string()));
                            }
                        }
                    }
                }
                None => {
                    return Err(SyncError::Network(
                        "Failed to clone request for retry".to_string(),
                    ));
                }
            }
        }

        Err(SyncError::Network("Max retries exceeded".to_string()))
    }

    /// Sync a read model from the API and detect conflicts.
    ///
    /// Returns:
    /// - `Ok(Some(SyncReadModelResult::Conflict))` if a conflict is detected
    /// - `Ok(Some(SyncReadModelResult::Synced))` if successfully synced
    /// - `Ok(None)` if no local version exists (first sync)
    pub async fn sync_read_model(
        &self,
        tenant_id: TenantId,
        aggregate_type: &str,
        aggregate_id: &AggregateId,
    ) -> Result<Option<SyncReadModelResult>, SyncError> {
        // Get local version from cache
        let local_version = self
            .cache
            .get_read_model_version(tenant_id, aggregate_type, aggregate_id)
            .map_err(|e| SyncError::Cache(e.to_string()))?;

        // Fetch remote read model
        let client = reqwest::Client::new();
        let endpoint = self.read_model_endpoint(aggregate_type, aggregate_id);
        let url = format!("{}{}", self.api_url, endpoint);

        let mut req = client.get(&url);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| SyncError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SyncError::Api(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        // Extract headers before consuming response body
        let headers = resp.headers().clone();

        // Read response body once (needed for both version extraction and model caching)
        let body: Value = resp.json().await.map_err(|e| {
            SyncError::Parse(format!("Failed to parse read model: {}", e))
        })?;

        // Extract version from response headers or body
        let remote_version = self.extract_version_from_body(&body, &headers).await?;

        // Check for conflict
        if let Some(local_ver) = local_version {
            if local_ver < remote_version {
                tracing::warn!(
                    "Conflict detected for {} {}: local={}, remote={}",
                    aggregate_type,
                    aggregate_id,
                    local_ver,
                    remote_version
                );

                // For now, default to UseRemote resolution
                // In a full implementation, this would be configurable
                let conflict = Conflict {
                    aggregate_type: aggregate_type.to_string(),
                    aggregate_id: *aggregate_id,
                    local_version: Some(local_ver),
                    remote_version,
                    resolution: ConflictResolution::UseRemote,
                };

                // Apply resolution
                match conflict.resolution {
                    ConflictResolution::UseRemote => {
                        // Cache the remote version
                        self.cache
                            .cache_read_model_with_version(
                                &tenant_id,
                                aggregate_type,
                                &aggregate_id.to_string(),
                                &body,
                                Some(remote_version),
                            )
                            .await
                            .map_err(|e| SyncError::Cache(e.to_string()))?;
                    }
                    ConflictResolution::UseLocal => {
                        // Keep local version, don't update cache
                        // In a full implementation, would POST local version to API
                    }
                    ConflictResolution::Merge => {
                        // Future: implement merge logic
                        tracing::warn!("Merge resolution not yet implemented, using remote");
                        self.cache
                            .cache_read_model_with_version(
                                &tenant_id,
                                aggregate_type,
                                &aggregate_id.to_string(),
                                &body,
                                Some(remote_version),
                            )
                            .await
                            .map_err(|e| SyncError::Cache(e.to_string()))?;
                    }
                }

                return Ok(Some(SyncReadModelResult::Conflict(conflict)));
            }
        }

        // No conflict or first sync - cache the remote version
        self.cache
            .cache_read_model_with_version(
                &tenant_id,
                aggregate_type,
                &aggregate_id.to_string(),
                &body,
                Some(remote_version),
            )
            .await
            .map_err(|e| SyncError::Cache(e.to_string()))?;

        Ok(Some(SyncReadModelResult::Synced((
            aggregate_type.to_string(),
            *aggregate_id,
        ))))
    }

    /// Extract version from API response (headers or body).
    async fn extract_version_from_body(
        &self,
        body: &Value,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<u64, SyncError> {
        // Try to get version from ETag or custom header
        if let Some(etag) = headers.get("ETag") {
            if let Ok(etag_str) = etag.to_str() {
                if let Some(version_str) = etag_str.strip_prefix("v") {
                    if let Ok(version) = version_str.parse::<u64>() {
                        return Ok(version);
                    }
                }
            }
        }

        // Try to get version from X-Stream-Version header
        if let Some(version_header) = headers.get("X-Stream-Version") {
            if let Ok(version_str) = version_header.to_str() {
                if let Ok(version) = version_str.parse::<u64>() {
                    return Ok(version);
                }
            }
        }

        // Try to parse from response body (for endpoints that return stream_version)
        if let Some(version) = body.get("stream_version").and_then(|v| v.as_u64()) {
            return Ok(version);
        }

        // Fallback: use cached_at timestamp as a proxy for version ordering
        // This is not ideal but works when version is not available
        tracing::warn!("Version not found in response, using timestamp fallback");
        // Return a high version number to avoid false conflicts
        Ok(Utc::now().timestamp() as u64)
    }

    /// Map command type to API endpoint.
    fn command_endpoint(&self, command_type: &str, aggregate_id: &AggregateId) -> String {
        match command_type {
            s if s.starts_with("inventory.") => {
                if s.contains("adjust") {
                    format!("/inventory/items/{}/adjust", aggregate_id)
                } else if s.contains("create") {
                    "/inventory/items".to_string()
                } else {
                    format!("/inventory/items/{}", aggregate_id)
                }
            }
            s if s.starts_with("sales.") => {
                if s.contains("create") {
                    "/sales/orders".to_string()
                } else {
                    format!("/sales/orders/{}", aggregate_id)
                }
            }
            s if s.starts_with("products.") => {
                if s.contains("create") {
                    "/products".to_string()
                } else {
                    format!("/products/{}", aggregate_id)
                }
            }
            _ => format!("/commands/{}", aggregate_id),
        }
    }

    /// Map aggregate type to read model endpoint.
    fn read_model_endpoint(&self, aggregate_type: &str, aggregate_id: &AggregateId) -> String {
        match aggregate_type {
            "inventory_item" => format!("/inventory/items/{}", aggregate_id),
            "sales_order" => format!("/sales/orders/{}", aggregate_id),
            "product" => format!("/products/{}", aggregate_id),
            _ => format!("/read-models/{}/{}", aggregate_type, aggregate_id),
        }
    }

    /// Extract aggregate type from command type.
    fn aggregate_type_from_command_type(&self, command_type: &str) -> String {
        if let Some(prefix) = command_type.split('.').next() {
            match prefix {
                "inventory" => "inventory_item".to_string(),
                "sales" => "sales_order".to_string(),
                "products" => "product".to_string(),
                _ => prefix.to_string(),
            }
        } else {
            command_type.to_string()
        }
    }
}

/// Result of syncing a read model.
#[cfg(feature = "tauri")]
pub enum SyncReadModelResult {
    Conflict(Conflict),
    Synced((String, AggregateId)),
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("client is offline")]
    Offline,
    #[error("network error: {0}")]
    Network(String),
    #[error("API error ({0}): {1}")]
    Api(u16, String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("cache error: {0}")]
    Cache(String),
    #[error("conflict: {0}")]
    Conflict(String),
}

