//! Local read model cache for offline support.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};

use forgeerp_core::TenantId;
use forgeerp_inventory::InventoryItemId;

/// Cache entry with timestamp for staleness tracking.
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    cached_at: DateTime<Utc>,
}

/// In-memory local cache for read models (offline support).
///
/// This is intentionally simple for the foundation. In a full Tauri app,
/// this could be backed by IndexedDB (via tauri-plugin-store) or SQLite.
#[derive(Debug, Clone)]
pub struct LocalCache {
    // For now, just inventory read models. Extensible to other projections.
    inventory: Arc<RwLock<HashMap<(TenantId, InventoryItemId), CacheEntry<InventoryReadModel>>>>,
}

/// Simplified inventory read model (matches API response shape).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InventoryReadModel {
    pub id: String,
    pub name: String,
    pub quantity: i64,
}

impl LocalCache {
    pub fn new() -> Self {
        Self {
            inventory: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Cache an inventory item read model.
    pub fn cache_inventory_item(
        &self,
        tenant_id: TenantId,
        item_id: InventoryItemId,
        model: InventoryReadModel,
    ) {
        let mut cache = self.inventory.write().unwrap();
        cache.insert(
            (tenant_id, item_id),
            CacheEntry {
                data: model,
                cached_at: Utc::now(),
            },
        );
    }

    /// Get a cached inventory item (if available and not stale beyond max_age).
    pub fn get_inventory_item(
        &self,
        tenant_id: TenantId,
        item_id: &InventoryItemId,
        max_age: Option<chrono::Duration>,
    ) -> Option<InventoryReadModel> {
        let cache = self.inventory.read().unwrap();
        let entry = cache.get(&(tenant_id, *item_id))?;

        if let Some(max) = max_age {
            let age = Utc::now().signed_duration_since(entry.cached_at);
            if age > max {
                return None;
            }
        }

        Some(entry.data.clone())
    }

    /// Clear all cached data for a tenant.
    pub fn clear_tenant(&self, tenant_id: TenantId) {
        let mut cache = self.inventory.write().unwrap();
        cache.retain(|(t, _), _| *t != tenant_id);
    }

    /// Clear all cached data.
    pub fn clear_all(&self) {
        let mut cache = self.inventory.write().unwrap();
        cache.clear();
    }
}

impl Default for LocalCache {
    fn default() -> Self {
        Self::new()
    }
}

