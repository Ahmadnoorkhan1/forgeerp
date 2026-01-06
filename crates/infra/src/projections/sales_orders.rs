use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_sales::{SalesOrderEvent, SalesOrderId, SalesOrderStatus};
use forgeerp_products::ProductId;

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesOrderLineReadModel {
    pub line_no: u32,
    pub product_id: ProductId,
    pub quantity: i64,
    pub unit_price: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesOrderReadModel {
    pub order_id: SalesOrderId,
    pub status: SalesOrderStatus,
    pub lines: Vec<SalesOrderLineReadModel>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum SalesOrderProjectionError {
    #[error("failed to deserialize sales order event: {0}")]
    Deserialize(String),
    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),
    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

#[derive(Debug)]
pub struct SalesOrdersProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<SalesOrderId, SalesOrderReadModel>,
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

impl<S> SalesOrdersProjection<S>
where
    S: TenantStore<SalesOrderId, SalesOrderReadModel>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "sales.orders".to_string(),
        }
    }
}

impl<S> SalesOrdersProjection<S>
where
    S: TenantStore<SalesOrderId, SalesOrderReadModel>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> SalesOrdersProjection<S, C> {
        SalesOrdersProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> SalesOrdersProjection<S, C>
where
    S: TenantStore<SalesOrderId, SalesOrderReadModel>,
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

    pub fn get(&self, tenant_id: TenantId, order_id: &SalesOrderId) -> Option<SalesOrderReadModel> {
        self.store.get(tenant_id, order_id)
    }

    pub fn list(&self, tenant_id: TenantId) -> Vec<SalesOrderReadModel> {
        self.store.list(tenant_id)
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), SalesOrderProjectionError> {
        if envelope.aggregate_type() != "sales.order" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);
        if seq == 0 {
            return Err(SalesOrderProjectionError::NonMonotonicSequence { last, found: seq });
        }
        if seq <= last {
            return Ok(());
        }
        if seq != last + 1 && last != 0 {
            return Err(SalesOrderProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: SalesOrderEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| SalesOrderProjectionError::Deserialize(e.to_string()))?;

        let (event_tenant, order_id) = match &ev {
            SalesOrderEvent::SalesOrderCreated(e) => (e.tenant_id, e.order_id),
            SalesOrderEvent::LineAdded(e) => (e.tenant_id, e.order_id),
            SalesOrderEvent::OrderConfirmed(e) => (e.tenant_id, e.order_id),
            SalesOrderEvent::OrderInvoiced(e) => (e.tenant_id, e.order_id),
        };

        if event_tenant != tenant_id {
            return Err(SalesOrderProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }
        if order_id.0 != aggregate_id {
            return Err(SalesOrderProjectionError::TenantIsolation(
                "event order_id does not match envelope aggregate_id".to_string(),
            ));
        }

        match ev {
            SalesOrderEvent::SalesOrderCreated(e) => {
                self.store.upsert(
                    tenant_id,
                    e.order_id,
                    SalesOrderReadModel {
                        order_id: e.order_id,
                        status: SalesOrderStatus::Draft,
                        lines: vec![],
                    },
                );
            }
            SalesOrderEvent::LineAdded(e) => {
                let mut rm = self.store.get(tenant_id, &e.order_id).unwrap_or(SalesOrderReadModel {
                    order_id: e.order_id,
                    status: SalesOrderStatus::Draft,
                    lines: vec![],
                });
                rm.lines.push(SalesOrderLineReadModel {
                    line_no: e.line_no,
                    product_id: e.product_id,
                    quantity: e.quantity,
                    unit_price: e.unit_price,
                });
                self.store.upsert(tenant_id, e.order_id, rm);
            }
            SalesOrderEvent::OrderConfirmed(e) => {
                let mut rm = self.store.get(tenant_id, &e.order_id).unwrap_or(SalesOrderReadModel {
                    order_id: e.order_id,
                    status: SalesOrderStatus::Draft,
                    lines: vec![],
                });
                rm.status = SalesOrderStatus::Confirmed;
                self.store.upsert(tenant_id, e.order_id, rm);
            }
            SalesOrderEvent::OrderInvoiced(e) => {
                let mut rm = self.store.get(tenant_id, &e.order_id).unwrap_or(SalesOrderReadModel {
                    order_id: e.order_id,
                    status: SalesOrderStatus::Draft,
                    lines: vec![],
                });
                rm.status = SalesOrderStatus::Invoiced;
                self.store.upsert(tenant_id, e.order_id, rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), SalesOrderProjectionError> {
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


