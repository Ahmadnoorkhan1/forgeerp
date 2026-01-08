//! Explicit sync/reconnect with the API.

#[cfg(feature = "tauri")]
use anyhow::Result;

#[cfg(feature = "tauri")]
use crate::cache::{LocalCache, InventoryReadModel};
#[cfg(feature = "tauri")]
use crate::offline::OfflineMode;
#[cfg(feature = "tauri")]
use forgeerp_core::TenantId;
#[cfg(feature = "tauri")]
use forgeerp_inventory::InventoryItemId;
#[cfg(feature = "tauri")]

/// Client for syncing read models from the API.
///
/// This is intentionally minimal for the foundation. In a full Tauri app,
/// this would use `reqwest` or similar to make HTTP calls.
#[cfg(feature = "tauri")]
pub struct SyncClient {
    api_url: String,
    token: Option<String>,
}

#[cfg(feature = "tauri")]
impl SyncClient {
    pub fn new(api_url: String) -> Self {
        Self {
            api_url,
            token: None,
        }
    }

    pub fn with_token(api_url: String, token: String) -> Self {
        Self {
            api_url,
            token: Some(token),
        }
    }

    /// Check connectivity by hitting the health endpoint.
    pub async fn check_connectivity(&self) -> bool {
        let client = reqwest::Client::new();
        let url = format!("{}/health", self.api_url);
        client.get(&url).send().await.is_ok()
    }

    /// Sync a specific inventory item from the API and update local cache.
    pub async fn sync_inventory_item(
        &self,
        cache: &LocalCache,
        tenant_id: TenantId,
        item_id: &InventoryItemId,
    ) -> Result<(), SyncError> {
        let client = reqwest::Client::new();
        let url = format!("{}/inventory/items/{}", self.api_url, item_id.0);
        let mut req = client.get(&url);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(|e| SyncError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(SyncError::Api(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        let model: InventoryReadModel = resp.json().await.map_err(|e| SyncError::Parse(e.to_string()))?;
        cache.cache_inventory_item(tenant_id, *item_id, model);

        Ok(())
    }

    /// Full sync: reconnect, check connectivity, and sync all known items.
    ///
    /// This is a placeholder for the foundation. A full implementation would:
    /// 1. List all items for the tenant
    /// 2. Sync each item
    /// 3. Handle pagination/batching
    pub async fn full_sync(
        &self,
        _cache: &LocalCache,
        _tenant_id: TenantId,
        offline: &mut OfflineMode,
    ) -> Result<(), SyncError> {
        // Check connectivity first.
        if !self.check_connectivity().await {
            offline.set_offline();
            return Err(SyncError::Offline);
        }

        offline.set_online();

        // For now, this is a no-op (foundation only).
        // In a full implementation, this would enumerate and sync all items.
        Ok(())
    }
}

#[cfg(not(feature = "tauri"))]
pub struct SyncClient;

#[cfg(not(feature = "tauri"))]
impl SyncClient {
    pub fn new(_api_url: String) -> Self {
        Self
    }

    pub fn with_token(_api_url: String, _token: String) -> Self {
        Self
    }
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
}

