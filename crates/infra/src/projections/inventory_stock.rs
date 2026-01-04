use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_ai::{AiError, InventoryItemSnapshot, InventorySnapshot, ReadModelReader};
use forgeerp_events::EventEnvelope;
use forgeerp_inventory::{InventoryEvent, InventoryItemId};

use crate::read_model::TenantStore;
use crate::projections::cursor_store::ProjectionCursorStore;

/// Queryable inventory read model: current stock per item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryReadModel {
    pub item_id: InventoryItemId,
    pub name: String,
    pub quantity: i64,
}

/// Tenant+aggregate cursor to support at-least-once delivery (idempotent projection).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum InventoryProjectionError {
    #[error("failed to deserialize inventory event: {0}")]
    Deserialize(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

/// Inventory stock projection.
///
/// Consumes published envelopes (JSON payloads) and maintains a tenant-isolated read model.
/// Read models are disposable and rebuildable from the event stream.
///
/// Supports both in-memory and persistent cursor tracking:
/// - If `cursor_store` is `None`, uses in-memory cursors (existing behavior)
/// - If `cursor_store` is `Some`, persists cursors to Postgres (resume after crash)
#[derive(Debug)]
pub struct InventoryStockProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
}

/// In-memory cursor store (default, no persistence).
pub struct InMemoryCursorStore;

impl ProjectionCursorStore for InMemoryCursorStore {
    fn get_cursor(&self, _tenant_id: TenantId, _aggregate_id: AggregateId, _projection_name: &str) -> Option<u64> {
        None // Always returns None, forcing use of in-memory cursors
    }

    fn update_cursor(&self, _tenant_id: TenantId, _aggregate_id: AggregateId, _projection_name: &str, _sequence_number: u64) {
        // No-op for in-memory store
    }

    fn clear_cursors(&self, _tenant_id: TenantId, _projection_name: &str) {
        // No-op for in-memory store
    }
}

impl<S> InventoryStockProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
{
    /// Create a new projection with in-memory cursor tracking.
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "inventory.stock".to_string(),
        }
    }
}

impl<S> InventoryStockProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
{
    /// Create a new projection with persistent cursor tracking.
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> InventoryStockProjection<S, C> {
        InventoryStockProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> InventoryStockProjection<S, C>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
    C: ProjectionCursorStore + 'static,
{
}

impl<S, C> InventoryStockProjection<S, C>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
    C: ProjectionCursorStore + 'static,
{

    /// Load cursor from persistent store if available, otherwise from memory.
    fn get_cursor(&self, tenant_id: TenantId, aggregate_id: AggregateId) -> u64 {
        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store
                .get_cursor(tenant_id, aggregate_id, &self.projection_name)
                .unwrap_or(0)
        } else {
            match self.cursors.read() {
                Ok(cursors) => *cursors.get(&CursorKey { tenant_id, aggregate_id }).unwrap_or(&0),
                Err(_) => 0,
            }
        }
    }

    /// Update cursor in both memory and persistent store (if available).
    fn update_cursor(&self, tenant_id: TenantId, aggregate_id: AggregateId, sequence_number: u64) {
        // Update in-memory cache
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.insert(CursorKey { tenant_id, aggregate_id }, sequence_number);
        }

        // Persist to database if available
        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store.update_cursor(tenant_id, aggregate_id, &self.projection_name, sequence_number);
        }
    }

    /// Clear cursors in both memory and persistent store (if available).
    fn clear_cursors(&self, tenant_id: TenantId) {
        // Clear in-memory cache
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.retain(|k, _| k.tenant_id != tenant_id);
        }

        // Clear persistent store if available
        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store.clear_cursors(tenant_id, &self.projection_name);
        }
    }

    /// Query read model for one tenant/item.
    pub fn get(&self, tenant_id: TenantId, item_id: &InventoryItemId) -> Option<InventoryReadModel> {
        self.store.get(tenant_id, item_id)
    }

    /// List all items for a tenant (disposable read model).
    pub fn list(&self, tenant_id: TenantId) -> Vec<InventoryReadModel> {
        self.store.list(tenant_id)
    }

    /// Apply a published envelope into the projection.
    ///
    /// - Enforces tenant isolation
    /// - Enforces monotonic sequence per (tenant, aggregate) stream
    /// - Idempotent for at-least-once delivery (replays <= cursor are ignored)
    pub fn apply_envelope(&self, envelope: &EventEnvelope<JsonValue>) -> Result<(), InventoryProjectionError> {
        // Ignore non-inventory aggregates (allows sharing a bus across modules).
        if envelope.aggregate_type() != "inventory.item" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        // Cursor check (per tenant + aggregate stream).
        let last = self.get_cursor(tenant_id, aggregate_id);

        if seq == 0 {
            return Err(InventoryProjectionError::NonMonotonicSequence { last, found: seq });
        }

        if seq <= last {
            // Duplicate or replay; safe to ignore.
            return Ok(());
        }

        if seq != last + 1 && last != 0 {
            // We allow first event to be any positive sequence (some stores start at 1),
            // but after that we enforce strict monotonic increments.
            return Err(InventoryProjectionError::NonMonotonicSequence { last, found: seq });
        }

        // Deserialize the inventory event from payload.
        let inv: InventoryEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| InventoryProjectionError::Deserialize(e.to_string()))?;

        // Validate tenant isolation at the event level.
        let (event_tenant, item_id) = match &inv {
            InventoryEvent::ItemCreated(e) => (e.tenant_id, e.item_id),
            InventoryEvent::StockAdjusted(e) => (e.tenant_id, e.item_id),
        };

        if event_tenant != tenant_id {
            return Err(InventoryProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }

        if item_id.0 != aggregate_id {
            return Err(InventoryProjectionError::TenantIsolation(
                "event item_id does not match envelope aggregate_id".to_string(),
            ));
        }

        // Apply update.
        match inv {
            InventoryEvent::ItemCreated(e) => {
                self.store.upsert(
                    tenant_id,
                    e.item_id,
                    InventoryReadModel {
                        item_id: e.item_id,
                        name: e.name,
                        quantity: 0,
                    },
                );
            }
            InventoryEvent::StockAdjusted(e) => {
                let mut rm = self.store.get(tenant_id, &e.item_id).unwrap_or(InventoryReadModel {
                    item_id: e.item_id,
                    name: String::new(),
                    quantity: 0,
                });
                rm.quantity += e.delta;
                self.store.upsert(tenant_id, e.item_id, rm);
            }
        }

        // Advance cursor after successful apply.
        self.update_cursor(tenant_id, aggregate_id, seq);

        Ok(())
    }

    /// Rebuild the read model from scratch by replaying envelopes.
    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), InventoryProjectionError> {
        let mut envs: Vec<_> = envelopes.into_iter().collect();

        // Clear read model per tenant before rebuilding.
        {
            let mut tenants = envs.iter().map(|e| e.tenant_id()).collect::<Vec<_>>();
            tenants.sort_by_key(|t| *t.as_uuid().as_bytes());
            tenants.dedup();
            for t in tenants {
                // Clear read models
                self.store.clear_tenant(t);
                // Clear cursors (both in-memory and persistent)
                self.clear_cursors(t);
            }
        }


        // Deterministic replay order: tenant, aggregate, sequence.
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

impl<S, C> ReadModelReader<InventorySnapshot> for InventoryStockProjection<S, C>
where
    S: TenantStore<InventoryItemId, InventoryReadModel> + Send + Sync + 'static,
    C: ProjectionCursorStore + 'static,
{
    fn get_snapshot(&self, tenant_id: TenantId) -> Result<InventorySnapshot, AiError> {
        let items = self
            .list(tenant_id)
            .into_iter()
            .map(|rm| InventoryItemSnapshot {
                item_id: rm.item_id.to_string(),
                quantity: rm.quantity,
                // Derived trend (minimal): latest quantity only.
                historical_trend: vec![rm.quantity],
            })
            .collect::<Vec<_>>();

        Ok(InventorySnapshot { tenant_id, items })
    }
}


