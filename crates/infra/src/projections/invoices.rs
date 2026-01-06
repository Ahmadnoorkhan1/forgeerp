use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_invoicing::{InvoiceEvent, InvoiceId, InvoiceLine, InvoiceStatus};
use forgeerp_sales::SalesOrderId;

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Queryable invoice read model (header + lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceReadModel {
    pub invoice_id: InvoiceId,
    pub sales_order_id: SalesOrderId,
    pub due_date: Option<chrono::DateTime<chrono::Utc>>,
    pub status: InvoiceStatus,
    pub total_amount: u64,
    pub total_paid: u64,
    pub lines: Vec<InvoiceLine>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum InvoiceProjectionError {
    #[error("failed to deserialize invoice event: {0}")]
    Deserialize(String),
    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),
    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

#[derive(Debug)]
pub struct InvoicesProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<InvoiceId, InvoiceReadModel>,
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

impl<S> InvoicesProjection<S>
where
    S: TenantStore<InvoiceId, InvoiceReadModel>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "invoicing.invoices".to_string(),
        }
    }
}

impl<S> InvoicesProjection<S>
where
    S: TenantStore<InvoiceId, InvoiceReadModel>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> InvoicesProjection<S, C> {
        InvoicesProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> InvoicesProjection<S, C>
where
    S: TenantStore<InvoiceId, InvoiceReadModel>,
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

    pub fn get(&self, tenant_id: TenantId, invoice_id: &InvoiceId) -> Option<InvoiceReadModel> {
        self.store.get(tenant_id, invoice_id)
    }

    pub fn list(&self, tenant_id: TenantId) -> Vec<InvoiceReadModel> {
        self.store.list(tenant_id)
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), InvoiceProjectionError> {
        if envelope.aggregate_type() != "invoicing.invoice" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);
        if seq == 0 {
            return Err(InvoiceProjectionError::NonMonotonicSequence { last, found: seq });
        }
        if seq <= last {
            return Ok(());
        }
        if seq != last + 1 && last != 0 {
            return Err(InvoiceProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: InvoiceEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| InvoiceProjectionError::Deserialize(e.to_string()))?;

        let (event_tenant, invoice_id) = match &ev {
            InvoiceEvent::InvoiceIssued(e) => (e.tenant_id, e.invoice_id),
            InvoiceEvent::PaymentRegistered(e) => (e.tenant_id, e.invoice_id),
            InvoiceEvent::InvoiceVoided(e) => (e.tenant_id, e.invoice_id),
        };

        if event_tenant != tenant_id {
            return Err(InvoiceProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }
        if invoice_id.0 != aggregate_id {
            return Err(InvoiceProjectionError::TenantIsolation(
                "event invoice_id does not match envelope aggregate_id".to_string(),
            ));
        }

        match ev {
            InvoiceEvent::InvoiceIssued(e) => {
                self.store.upsert(
                    tenant_id,
                    e.invoice_id,
                    InvoiceReadModel {
                        invoice_id: e.invoice_id,
                        sales_order_id: e.sales_order_id,
                        due_date: Some(e.due_date),
                        status: InvoiceStatus::Open,
                        total_amount: e.total_amount,
                        total_paid: 0,
                        lines: e.lines,
                    },
                );
            }
            InvoiceEvent::PaymentRegistered(e) => {
                let mut rm = self.store.get(tenant_id, &e.invoice_id).unwrap_or(InvoiceReadModel {
                    invoice_id: e.invoice_id,
                    sales_order_id: SalesOrderId::new(AggregateId::new()),
                    due_date: None,
                    status: InvoiceStatus::Open,
                    total_amount: 0,
                    total_paid: 0,
                    lines: vec![],
                });
                rm.total_paid = e.new_total_paid;
                if rm.total_paid >= rm.total_amount {
                    rm.status = InvoiceStatus::Paid;
                }
                self.store.upsert(tenant_id, e.invoice_id, rm);
            }
            InvoiceEvent::InvoiceVoided(e) => {
                let mut rm = self.store.get(tenant_id, &e.invoice_id).unwrap_or(InvoiceReadModel {
                    invoice_id: e.invoice_id,
                    sales_order_id: SalesOrderId::new(AggregateId::new()),
                    due_date: None,
                    status: InvoiceStatus::Open,
                    total_amount: 0,
                    total_paid: 0,
                    lines: vec![],
                });
                rm.status = InvoiceStatus::Void;
                self.store.upsert(tenant_id, e.invoice_id, rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), InvoiceProjectionError> {
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


