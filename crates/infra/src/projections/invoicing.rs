use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_invoicing::{InvoiceEvent, InvoiceId, InvoiceStatus};

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Read model for Accounts Receivable (AR) aging.
///
/// This model stores enough information to compute aging buckets at query time:
/// - original invoice amount
/// - outstanding amount
/// - due date
/// - current status
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceAgingReadModel {
    pub invoice_id: InvoiceId,
    pub total_amount: u64,
    pub outstanding_amount: u64,
    pub due_date: Option<chrono::DateTime<chrono::Utc>>,
    pub status: InvoiceStatus,
}

/// Tenant+aggregate cursor to support at-least-once delivery (idempotent projection).
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

/// Invoicing projection: maintains AR aging read model.
#[derive(Debug)]
pub struct InvoiceAgingProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<InvoiceId, InvoiceAgingReadModel>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
}

/// In-memory cursor store (default, no persistence).
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

impl<S> InvoiceAgingProjection<S>
where
    S: TenantStore<InvoiceId, InvoiceAgingReadModel>,
{
    /// Create a new projection with in-memory cursor tracking.
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "invoicing.aging".to_string(),
        }
    }
}

impl<S> InvoiceAgingProjection<S>
where
    S: TenantStore<InvoiceId, InvoiceAgingReadModel>,
{
    /// Create a new projection with persistent cursor tracking.
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> InvoiceAgingProjection<S, C> {
        InvoiceAgingProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> InvoiceAgingProjection<S, C>
where
    S: TenantStore<InvoiceId, InvoiceAgingReadModel>,
    C: ProjectionCursorStore + 'static,
{
}

impl<S, C> InvoiceAgingProjection<S, C>
where
    S: TenantStore<InvoiceId, InvoiceAgingReadModel>,
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

    /// Lookup invoice aging info for a single invoice.
    pub fn get(&self, tenant_id: TenantId, invoice_id: &InvoiceId) -> Option<InvoiceAgingReadModel> {
        self.store.get(tenant_id, invoice_id)
    }

    /// List all invoices for a tenant (to compute AR aging buckets externally).
    pub fn list(&self, tenant_id: TenantId) -> Vec<InvoiceAgingReadModel> {
        self.store.list(tenant_id)
    }

    /// Apply envelope into AR aging read model.
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
                    InvoiceAgingReadModel {
                        invoice_id: e.invoice_id,
                        total_amount: e.total_amount,
                        outstanding_amount: e.total_amount,
                        due_date: Some(e.due_date),
                        status: InvoiceStatus::Open,
                    },
                );
            }
            InvoiceEvent::PaymentRegistered(e) => {
                let mut rm = self
                    .store
                    .get(tenant_id, &e.invoice_id)
                    .unwrap_or(InvoiceAgingReadModel {
                        invoice_id: e.invoice_id,
                        total_amount: 0,
                        outstanding_amount: 0,
                        due_date: None,
                        status: InvoiceStatus::Open,
                    });
                let new_outstanding =
                    rm.outstanding_amount.saturating_sub(e.amount.min(rm.outstanding_amount));
                rm.outstanding_amount = new_outstanding;
                if new_outstanding == 0 {
                    rm.status = InvoiceStatus::Paid;
                }
                self.store.upsert(tenant_id, e.invoice_id, rm);
            }
            InvoiceEvent::InvoiceVoided(e) => {
                let mut rm = self
                    .store
                    .get(tenant_id, &e.invoice_id)
                    .unwrap_or(InvoiceAgingReadModel {
                        invoice_id: e.invoice_id,
                        total_amount: 0,
                        outstanding_amount: 0,
                        due_date: None,
                        status: InvoiceStatus::Open,
                    });
                rm.status = InvoiceStatus::Void;
                self.store.upsert(tenant_id, e.invoice_id, rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    /// Rebuild AR aging read model from scratch by replaying envelopes.
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


