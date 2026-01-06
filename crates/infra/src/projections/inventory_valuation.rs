//! Inventory Valuation Projection.
//!
//! Tracks inventory value per item (quantity × unit cost).
//! 
//! NOTE: The current inventory domain model tracks quantities only, not costs.
//! This projection allows setting a unit cost for each item to compute valuation.
//! In a complete implementation, costs would come from:
//! - Purchase order receiving (weighted average cost)
//! - Product catalog (standard cost)
//! - Manual cost entries

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_inventory::{InventoryEvent, InventoryItemId};

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Read model: inventory valuation per item.
///
/// Tracks:
/// - `item_id`: The inventory item
/// - `name`: Item name
/// - `quantity`: Current stock quantity
/// - `unit_cost`: Cost per unit (in smallest currency unit, e.g., cents)
/// - `total_value`: quantity × unit_cost
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryValuation {
    pub item_id: InventoryItemId,
    pub name: String,
    pub quantity: i64,
    /// Unit cost in smallest currency unit (e.g., cents). None if not set.
    pub unit_cost: Option<u64>,
    /// Total value = quantity × unit_cost. None if unit_cost not set.
    pub total_value: Option<u64>,
}

impl InventoryValuation {
    pub fn new(item_id: InventoryItemId, name: String) -> Self {
        Self {
            item_id,
            name,
            quantity: 0,
            unit_cost: None,
            total_value: None,
        }
    }

    fn recalculate_value(&mut self) {
        self.total_value = self.unit_cost.map(|cost| {
            if self.quantity > 0 {
                (self.quantity as u64).saturating_mul(cost)
            } else {
                0
            }
        });
    }
}

/// Summary of total inventory value for a tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryValuationSummary {
    pub total_items: usize,
    pub valued_items: usize,
    pub unvalued_items: usize,
    pub total_value: u64,
    pub total_quantity: i64,
}

/// Tenant+aggregate cursor for idempotent projection.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum InventoryValuationError {
    #[error("failed to deserialize inventory event: {0}")]
    Deserialize(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

/// In-memory cursor store (no persistence).
pub struct InMemoryCursorStore;

impl ProjectionCursorStore for InMemoryCursorStore {
    fn get_cursor(
        &self,
        _tenant_id: TenantId,
        _aggregate_id: AggregateId,
        _projection_name: &str,
    ) -> Option<u64> {
        None
    }

    fn update_cursor(
        &self,
        _tenant_id: TenantId,
        _aggregate_id: AggregateId,
        _projection_name: &str,
        _sequence_number: u64,
    ) {
        // no-op
    }

    fn clear_cursors(&self, _tenant_id: TenantId, _projection_name: &str) {
        // no-op
    }
}

/// Inventory valuation projection: tracks quantity × cost per item.
///
/// Rebuildable from inventory events. Tenant-isolated.
#[derive(Debug)]
pub struct InventoryValuationProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<InventoryItemId, InventoryValuation>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
}

impl<S> InventoryValuationProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryValuation>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "inventory.valuation".to_string(),
        }
    }
}

impl<S> InventoryValuationProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryValuation>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> InventoryValuationProjection<S, C> {
        InventoryValuationProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> InventoryValuationProjection<S, C>
where
    S: TenantStore<InventoryItemId, InventoryValuation>,
    C: ProjectionCursorStore + 'static,
{
    fn get_cursor(&self, tenant_id: TenantId, aggregate_id: AggregateId) -> u64 {
        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store
                .get_cursor(tenant_id, aggregate_id, &self.projection_name)
                .unwrap_or(0)
        } else {
            match self.cursors.read() {
                Ok(cursors) => *cursors
                    .get(&CursorKey { tenant_id, aggregate_id })
                    .unwrap_or(&0),
                Err(_) => 0,
            }
        }
    }

    fn update_cursor(&self, tenant_id: TenantId, aggregate_id: AggregateId, sequence_number: u64) {
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.insert(CursorKey { tenant_id, aggregate_id }, sequence_number);
        }

        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store.update_cursor(
                tenant_id,
                aggregate_id,
                &self.projection_name,
                sequence_number,
            );
        }
    }

    fn clear_cursors(&self, tenant_id: TenantId) {
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.retain(|k, _| k.tenant_id != tenant_id);
        }

        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store.clear_cursors(tenant_id, &self.projection_name);
        }
    }

    /// Get valuation for a specific item.
    pub fn get(&self, tenant_id: TenantId, item_id: &InventoryItemId) -> Option<InventoryValuation> {
        self.store.get(tenant_id, item_id)
    }

    /// List all item valuations for a tenant.
    pub fn list(&self, tenant_id: TenantId) -> Vec<InventoryValuation> {
        self.store.list(tenant_id)
    }

    /// Get summary of total inventory value for a tenant.
    pub fn get_summary(&self, tenant_id: TenantId) -> InventoryValuationSummary {
        let items = self.store.list(tenant_id);
        let total_items = items.len();
        let valued_items = items.iter().filter(|v| v.unit_cost.is_some()).count();
        let unvalued_items = total_items - valued_items;
        let total_value: u64 = items.iter().filter_map(|v| v.total_value).sum();
        let total_quantity: i64 = items.iter().map(|v| v.quantity).sum();

        InventoryValuationSummary {
            total_items,
            valued_items,
            unvalued_items,
            total_value,
            total_quantity,
        }
    }

    /// List items without cost set (for valuation review).
    pub fn list_unvalued(&self, tenant_id: TenantId) -> Vec<InventoryValuation> {
        self.store
            .list(tenant_id)
            .into_iter()
            .filter(|v| v.unit_cost.is_none())
            .collect()
    }

    /// List items with positive value.
    pub fn list_valued(&self, tenant_id: TenantId) -> Vec<InventoryValuation> {
        self.store
            .list(tenant_id)
            .into_iter()
            .filter(|v| v.total_value.unwrap_or(0) > 0)
            .collect()
    }

    /// Set the unit cost for an item.
    /// 
    /// This recalculates the total_value based on current quantity.
    pub fn set_unit_cost(&self, tenant_id: TenantId, item_id: InventoryItemId, unit_cost: u64) {
        if let Some(mut val) = self.store.get(tenant_id, &item_id) {
            val.unit_cost = Some(unit_cost);
            val.recalculate_value();
            self.store.upsert(tenant_id, item_id, val);
        }
    }

    /// Apply envelope into inventory valuation.
    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), InventoryValuationError> {
        if envelope.aggregate_type() != "inventory.item" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);

        if seq == 0 {
            return Err(InventoryValuationError::NonMonotonicSequence { last, found: seq });
        }

        if seq <= last {
            return Ok(());
        }

        if seq != last + 1 && last != 0 {
            return Err(InventoryValuationError::NonMonotonicSequence { last, found: seq });
        }

        let ev: InventoryEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| InventoryValuationError::Deserialize(e.to_string()))?;

        let (event_tenant, item_id) = match &ev {
            InventoryEvent::ItemCreated(e) => (e.tenant_id, e.item_id),
            InventoryEvent::StockAdjusted(e) => (e.tenant_id, e.item_id),
        };

        if event_tenant != tenant_id {
            return Err(InventoryValuationError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }

        if item_id.0 != aggregate_id {
            return Err(InventoryValuationError::TenantIsolation(
                "event item_id does not match envelope aggregate_id".to_string(),
            ));
        }

        match ev {
            InventoryEvent::ItemCreated(e) => {
                let val = InventoryValuation::new(e.item_id, e.name);
                self.store.upsert(tenant_id, e.item_id, val);
            }
            InventoryEvent::StockAdjusted(e) => {
                let mut val = self.store.get(tenant_id, &e.item_id).unwrap_or_else(|| {
                    InventoryValuation::new(e.item_id, String::new())
                });
                val.quantity += e.delta;
                val.recalculate_value();
                self.store.upsert(tenant_id, e.item_id, val);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    /// Rebuild the read model from scratch.
    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), InventoryValuationError> {
        let mut envs: Vec<_> = envelopes.into_iter().collect();

        {
            let mut tenants = envs.iter().map(|e| e.tenant_id()).collect::<Vec<_>>();
            tenants.sort_by_key(|t| *t.as_uuid().as_bytes());
            tenants.dedup();
            for t in tenants {
                self.store.clear_tenant(t);
                self.clear_cursors(t);
            }
        }

        envs.sort_by_key(|e| {
            (
                *e.tenant_id().as_uuid().as_bytes(),
                *e.aggregate_id().as_uuid().as_bytes(),
                e.sequence_number(),
            )
        });

        for env in &envs {
            self.apply_envelope(env)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_model::InMemoryTenantStore;
    use forgeerp_core::AggregateId;
    use forgeerp_inventory::{ItemCreated, StockAdjusted};
    use chrono::Utc;

    fn make_envelope(
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        seq: u64,
        event: InventoryEvent,
    ) -> EventEnvelope<JsonValue> {
        EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            aggregate_id,
            "inventory.item".to_string(),
            seq,
            serde_json::to_value(&event).unwrap(),
        )
    }

    #[test]
    fn tracks_quantity_from_events() {
        let store = Arc::new(InMemoryTenantStore::<InventoryItemId, InventoryValuation>::new());
        let proj = InventoryValuationProjection::new(store.clone());

        let tenant_id = TenantId::new();
        let item_id = InventoryItemId::new(AggregateId::new());

        let created = InventoryEvent::ItemCreated(ItemCreated {
            tenant_id,
            item_id,
            name: "Widget".to_string(),
            occurred_at: Utc::now(),
        });

        proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 1, created)).unwrap();

        let adjusted = InventoryEvent::StockAdjusted(StockAdjusted {
            tenant_id,
            item_id,
            delta: 100,
            occurred_at: Utc::now(),
        });

        proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 2, adjusted)).unwrap();

        let val = proj.get(tenant_id, &item_id).unwrap();
        assert_eq!(val.quantity, 100);
        assert_eq!(val.unit_cost, None);
        assert_eq!(val.total_value, None);
    }

    #[test]
    fn calculates_value_when_cost_set() {
        let store = Arc::new(InMemoryTenantStore::<InventoryItemId, InventoryValuation>::new());
        let proj = InventoryValuationProjection::new(store.clone());

        let tenant_id = TenantId::new();
        let item_id = InventoryItemId::new(AggregateId::new());

        let created = InventoryEvent::ItemCreated(ItemCreated {
            tenant_id,
            item_id,
            name: "Widget".to_string(),
            occurred_at: Utc::now(),
        });
        proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 1, created)).unwrap();

        let adjusted = InventoryEvent::StockAdjusted(StockAdjusted {
            tenant_id,
            item_id,
            delta: 50,
            occurred_at: Utc::now(),
        });
        proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 2, adjusted)).unwrap();

        // Set unit cost to 25 cents
        proj.set_unit_cost(tenant_id, item_id, 25);

        let val = proj.get(tenant_id, &item_id).unwrap();
        assert_eq!(val.quantity, 50);
        assert_eq!(val.unit_cost, Some(25));
        assert_eq!(val.total_value, Some(50 * 25));
    }

    #[test]
    fn summary_aggregates_values() {
        let store = Arc::new(InMemoryTenantStore::<InventoryItemId, InventoryValuation>::new());
        let proj = InventoryValuationProjection::new(store.clone());

        let tenant_id = TenantId::new();

        // Create and stock two items
        for i in 0..2 {
            let item_id = InventoryItemId::new(AggregateId::new());
            let created = InventoryEvent::ItemCreated(ItemCreated {
                tenant_id,
                item_id,
                name: format!("Item {}", i),
                occurred_at: Utc::now(),
            });
            proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 1, created)).unwrap();

            let adjusted = InventoryEvent::StockAdjusted(StockAdjusted {
                tenant_id,
                item_id,
                delta: 100,
                occurred_at: Utc::now(),
            });
            proj.apply_envelope(&make_envelope(tenant_id, item_id.0, 2, adjusted)).unwrap();

            // Set cost for first item only
            if i == 0 {
                proj.set_unit_cost(tenant_id, item_id, 10);
            }
        }

        let summary = proj.get_summary(tenant_id);
        assert_eq!(summary.total_items, 2);
        assert_eq!(summary.valued_items, 1);
        assert_eq!(summary.unvalued_items, 1);
        assert_eq!(summary.total_value, 100 * 10);
        assert_eq!(summary.total_quantity, 200);
    }
}

