use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;

use forgeerp_core::TenantId;
use std::sync::Arc;

/// Tenant-isolated key/value store abstraction for disposable read models.
pub trait TenantStore<K, V>: Send + Sync {
    fn get(&self, tenant_id: TenantId, key: &K) -> Option<V>;
    fn upsert(&self, tenant_id: TenantId, key: K, value: V);
    fn list(&self, tenant_id: TenantId) -> Vec<V>;
    /// Clear all read-model records for a tenant (rebuild support).
    fn clear_tenant(&self, tenant_id: TenantId);
}

impl<K, V, S> TenantStore<K, V> for Arc<S>
where
    S: TenantStore<K, V> + ?Sized,
{
    fn get(&self, tenant_id: TenantId, key: &K) -> Option<V> {
        (**self).get(tenant_id, key)
    }

    fn upsert(&self, tenant_id: TenantId, key: K, value: V) {
        (**self).upsert(tenant_id, key, value)
    }

    fn list(&self, tenant_id: TenantId) -> Vec<V> {
        (**self).list(tenant_id)
    }

    fn clear_tenant(&self, tenant_id: TenantId) {
        (**self).clear_tenant(tenant_id)
    }
}

/// In-memory tenant-isolated store for tests/dev.
#[derive(Debug)]
pub struct InMemoryTenantStore<K, V> {
    inner: RwLock<HashMap<(TenantId, K), V>>,
}

impl<K, V> InMemoryTenantStore<K, V> {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<K, V> Default for InMemoryTenantStore<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> TenantStore<K, V> for InMemoryTenantStore<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, tenant_id: TenantId, key: &K) -> Option<V> {
        let map = self.inner.read().ok()?;
        map.get(&(tenant_id, key.clone())).cloned()
    }

    fn upsert(&self, tenant_id: TenantId, key: K, value: V) {
        if let Ok(mut map) = self.inner.write() {
            map.insert((tenant_id, key), value);
        }
    }

    fn list(&self, tenant_id: TenantId) -> Vec<V> {
        let map = match self.inner.read() {
            Ok(m) => m,
            Err(_) => return vec![],
        };

        map.iter()
            .filter_map(|((t, _k), v)| if *t == tenant_id { Some(v.clone()) } else { None })
            .collect()
    }

    fn clear_tenant(&self, tenant_id: TenantId) {
        if let Ok(mut map) = self.inner.write() {
            map.retain(|(t, _k), _v| *t != tenant_id);
        }
    }
}


