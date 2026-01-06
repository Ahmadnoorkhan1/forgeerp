use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_parties::PartyId;
use forgeerp_purchasing::{
    LineItem, PurchaseOrderEvent, PurchaseOrderId, PurchaseOrderStatus,
};

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderReadModel {
    pub order_id: PurchaseOrderId,
    pub supplier_id: PartyId,
    pub status: PurchaseOrderStatus,
    pub lines: Vec<LineItem>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum PurchaseOrderProjectionError {
    #[error("failed to deserialize purchase order event: {0}")]
    Deserialize(String),
    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),
    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

#[derive(Debug)]
pub struct PurchaseOrdersProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<PurchaseOrderId, PurchaseOrderReadModel>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
}

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
    }
    fn clear_cursors(&self, _tenant_id: TenantId, _projection_name: &str) {}
}

impl<S> PurchaseOrdersProjection<S>
where
    S: TenantStore<PurchaseOrderId, PurchaseOrderReadModel>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "purchasing.orders".to_string(),
        }
    }
}

impl<S> PurchaseOrdersProjection<S>
where
    S: TenantStore<PurchaseOrderId, PurchaseOrderReadModel>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> PurchaseOrdersProjection<S, C> {
        PurchaseOrdersProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> PurchaseOrdersProjection<S, C>
where
    S: TenantStore<PurchaseOrderId, PurchaseOrderReadModel>,
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

    fn update_cursor(&self, tenant_id: TenantId, aggregate_id: AggregateId, seq: u64) {
        if let Ok(mut cursors) = self.cursors.write() {
            cursors.insert(CursorKey { tenant_id, aggregate_id }, seq);
        }
        if let Some(ref cursor_store) = self.cursor_store {
            cursor_store.update_cursor(tenant_id, aggregate_id, &self.projection_name, seq);
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

    pub fn get(&self, tenant_id: TenantId, order_id: &PurchaseOrderId) -> Option<PurchaseOrderReadModel> {
        self.store.get(tenant_id, order_id)
    }

    pub fn list(&self, tenant_id: TenantId) -> Vec<PurchaseOrderReadModel> {
        self.store.list(tenant_id)
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), PurchaseOrderProjectionError> {
        if envelope.aggregate_type() != "purchasing.order" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);
        if seq == 0 {
            return Err(PurchaseOrderProjectionError::NonMonotonicSequence { last, found: seq });
        }
        if seq <= last {
            return Ok(());
        }
        if seq != last + 1 && last != 0 {
            return Err(PurchaseOrderProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: PurchaseOrderEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| PurchaseOrderProjectionError::Deserialize(e.to_string()))?;

        let (event_tenant, order_id) = match &ev {
            PurchaseOrderEvent::PurchaseOrderCreated(e) => (e.tenant_id, e.order_id),
            PurchaseOrderEvent::PurchaseOrderLineAdded(e) => (e.tenant_id, e.order_id),
            PurchaseOrderEvent::PurchaseOrderApproved(e) => (e.tenant_id, e.order_id),
            PurchaseOrderEvent::GoodsReceived(e) => (e.tenant_id, e.order_id),
        };

        if event_tenant != tenant_id {
            return Err(PurchaseOrderProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }
        if order_id.0 != aggregate_id {
            return Err(PurchaseOrderProjectionError::TenantIsolation(
                "event order_id does not match envelope aggregate_id".to_string(),
            ));
        }

        match ev {
            PurchaseOrderEvent::PurchaseOrderCreated(e) => {
                self.store.upsert(
                    tenant_id,
                    e.order_id,
                    PurchaseOrderReadModel {
                        order_id: e.order_id,
                        supplier_id: e.supplier_id,
                        status: PurchaseOrderStatus::Draft,
                        lines: vec![],
                    },
                );
            }
            PurchaseOrderEvent::PurchaseOrderLineAdded(e) => {
                let mut rm = self
                    .store
                    .get(tenant_id, &e.order_id)
                    .unwrap_or(PurchaseOrderReadModel {
                        order_id: e.order_id,
                        supplier_id: PartyId::new(AggregateId::new()),
                        status: PurchaseOrderStatus::Draft,
                        lines: vec![],
                    });
                rm.lines.push(LineItem {
                    line_no: e.line_no,
                    product_id: e.product_id,
                    quantity: e.quantity,
                });
                self.store.upsert(tenant_id, e.order_id, rm);
            }
            PurchaseOrderEvent::PurchaseOrderApproved(e) => {
                let mut rm = self
                    .store
                    .get(tenant_id, &e.order_id)
                    .unwrap_or(PurchaseOrderReadModel {
                        order_id: e.order_id,
                        supplier_id: PartyId::new(AggregateId::new()),
                        status: PurchaseOrderStatus::Draft,
                        lines: vec![],
                    });
                rm.status = PurchaseOrderStatus::Approved;
                self.store.upsert(tenant_id, e.order_id, rm);
            }
            PurchaseOrderEvent::GoodsReceived(e) => {
                let mut rm = self
                    .store
                    .get(tenant_id, &e.order_id)
                    .unwrap_or(PurchaseOrderReadModel {
                        order_id: e.order_id,
                        supplier_id: e.supplier_id,
                        status: PurchaseOrderStatus::Draft,
                        lines: vec![],
                    });
                rm.status = PurchaseOrderStatus::Received;
                rm.lines = e.lines;
                self.store.upsert(tenant_id, e.order_id, rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), PurchaseOrderProjectionError> {
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


