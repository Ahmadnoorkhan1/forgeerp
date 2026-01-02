use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_inventory::{InventoryEvent, InventoryItemId};

use crate::read_model::TenantStore;

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
#[derive(Debug)]
pub struct InventoryStockProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
}

impl<S> InventoryStockProjection<S>
where
    S: TenantStore<InventoryItemId, InventoryReadModel>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
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
        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        // Cursor check (per tenant + aggregate stream).
        if let Ok(mut cursors) = self.cursors.write() {
            let key = CursorKey { tenant_id, aggregate_id };
            let last = *cursors.get(&key).unwrap_or(&0);

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
            cursors.insert(key, seq);
        }

        Ok(())
    }

    /// Rebuild the read model from scratch by replaying envelopes.
    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), InventoryProjectionError> {
        // Reset cursors; read model values are disposable, but store is opaque.
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.clear();
        }

        let mut envs: Vec<_> = envelopes.into_iter().collect();

        // Clear read model per tenant before rebuilding.
        {
            let mut tenants = envs.iter().map(|e| e.tenant_id()).collect::<Vec<_>>();
            tenants.sort_by_key(|t| *t.as_uuid().as_bytes());
            tenants.dedup();
            for t in tenants {
                self.store.clear_tenant(t);
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


