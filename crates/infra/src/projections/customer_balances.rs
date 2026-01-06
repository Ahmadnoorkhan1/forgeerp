//! Customer Balances Projection.
//!
//! Tracks outstanding balances per customer derived from invoice events.
//! 
//! NOTE: Currently, invoices are linked to SalesOrders, not directly to customers (PartyId).
//! This projection uses a synthetic customer key derived from invoice data.
//! When customer_id is added to invoices/sales orders, this projection should be updated.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_invoicing::{InvoiceEvent, InvoiceStatus};
use forgeerp_parties::PartyId;

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Read model: per-customer balance for a tenant.
///
/// Tracks:
/// - `total_invoiced`: Total amount invoiced to this customer
/// - `total_paid`: Total amount paid by this customer
/// - `outstanding_balance`: Amount still owed (invoiced - paid)
/// - `invoice_count`: Number of open invoices
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerBalance {
    pub customer_id: PartyId,
    pub customer_name: Option<String>,
    pub total_invoiced: u64,
    pub total_paid: u64,
    pub outstanding_balance: u64,
    pub open_invoice_count: u32,
}

impl CustomerBalance {
    pub fn new(customer_id: PartyId) -> Self {
        Self {
            customer_id,
            customer_name: None,
            total_invoiced: 0,
            total_paid: 0,
            outstanding_balance: 0,
            open_invoice_count: 0,
        }
    }
}

/// Tenant+aggregate cursor for idempotent projection.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum CustomerBalanceProjectionError {
    #[error("failed to deserialize invoice event: {0}")]
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

/// Tracks invoice→customer mapping for balance aggregation.
/// 
/// Since invoices don't currently have customer_id, we track this separately.
/// In production, this would be populated from sales order→customer relationships.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InvoiceCustomerMapping {
    customer_id: PartyId,
    total_amount: u64,
    total_paid: u64,
    status: InvoiceStatus,
}

/// Customer balances projection: aggregates outstanding balances per customer.
///
/// Rebuildable from invoice events. Tenant-isolated.
#[derive(Debug)]
pub struct CustomerBalancesProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<PartyId, CustomerBalance>,
{
    store: S,
    /// Invoice → customer mapping (for payment allocation)
    invoice_mappings: RwLock<HashMap<(TenantId, AggregateId), InvoiceCustomerMapping>>,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
}

impl<S> CustomerBalancesProjection<S>
where
    S: TenantStore<PartyId, CustomerBalance>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            invoice_mappings: RwLock::new(HashMap::new()),
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "customers.balances".to_string(),
        }
    }
}

impl<S> CustomerBalancesProjection<S>
where
    S: TenantStore<PartyId, CustomerBalance>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> CustomerBalancesProjection<S, C> {
        CustomerBalancesProjection {
            store: self.store,
            invoice_mappings: self.invoice_mappings,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> CustomerBalancesProjection<S, C>
where
    S: TenantStore<PartyId, CustomerBalance>,
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

    /// Get balance for a specific customer.
    pub fn get(&self, tenant_id: TenantId, customer_id: &PartyId) -> Option<CustomerBalance> {
        self.store.get(tenant_id, customer_id)
    }

    /// List all customer balances for a tenant.
    pub fn list(&self, tenant_id: TenantId) -> Vec<CustomerBalance> {
        self.store.list(tenant_id)
    }

    /// List customers with outstanding balances (non-zero).
    pub fn list_with_outstanding(&self, tenant_id: TenantId) -> Vec<CustomerBalance> {
        self.store
            .list(tenant_id)
            .into_iter()
            .filter(|b| b.outstanding_balance > 0)
            .collect()
    }

    /// Register a customer_id for an invoice.
    /// 
    /// Call this when processing InvoiceIssued to associate the invoice with a customer.
    /// In a complete implementation, this would be derived from sales order → customer linkage.
    pub fn register_invoice_customer(
        &self,
        tenant_id: TenantId,
        invoice_aggregate_id: AggregateId,
        customer_id: PartyId,
    ) {
        if let Ok(mut mappings) = self.invoice_mappings.write() {
            mappings.insert(
                (tenant_id, invoice_aggregate_id),
                InvoiceCustomerMapping {
                    customer_id,
                    total_amount: 0,
                    total_paid: 0,
                    status: InvoiceStatus::Open,
                },
            );
        }
    }

    /// Apply envelope into customer balances.
    ///
    /// NOTE: This requires `register_invoice_customer` to be called first to associate
    /// invoices with customers, since the current invoice model doesn't include customer_id.
    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), CustomerBalanceProjectionError> {
        if envelope.aggregate_type() != "invoicing.invoice" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);

        if seq == 0 {
            return Err(CustomerBalanceProjectionError::NonMonotonicSequence { last, found: seq });
        }

        if seq <= last {
            return Ok(());
        }

        if seq != last + 1 && last != 0 {
            return Err(CustomerBalanceProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: InvoiceEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| CustomerBalanceProjectionError::Deserialize(e.to_string()))?;

        let event_tenant = match &ev {
            InvoiceEvent::InvoiceIssued(e) => e.tenant_id,
            InvoiceEvent::PaymentRegistered(e) => e.tenant_id,
            InvoiceEvent::InvoiceVoided(e) => e.tenant_id,
        };

        if event_tenant != tenant_id {
            return Err(CustomerBalanceProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }

        // Get customer_id from mapping (if registered)
        let customer_id = {
            let mappings = self.invoice_mappings.read().ok();
            mappings.and_then(|m| m.get(&(tenant_id, aggregate_id)).map(|im| im.customer_id))
        };

        // If no customer mapping exists, use a synthetic ID based on sales_order_id
        // This is a fallback until proper customer associations are implemented
        let customer_id = match (&ev, customer_id) {
            (InvoiceEvent::InvoiceIssued(e), None) => {
                // Use sales_order_id as synthetic customer key
                PartyId::new(e.sales_order_id.0)
            }
            (_, Some(id)) => id,
            (InvoiceEvent::PaymentRegistered(e), None) => {
                // Try to look up from existing mapping, else use invoice_id as fallback
                PartyId::new(e.invoice_id.0)
            }
            (InvoiceEvent::InvoiceVoided(e), None) => {
                PartyId::new(e.invoice_id.0)
            }
        };

        match ev {
            InvoiceEvent::InvoiceIssued(e) => {
                // Update invoice mapping
                if let Ok(mut mappings) = self.invoice_mappings.write() {
                    mappings.insert(
                        (tenant_id, aggregate_id),
                        InvoiceCustomerMapping {
                            customer_id,
                            total_amount: e.total_amount,
                            total_paid: 0,
                            status: InvoiceStatus::Open,
                        },
                    );
                }

                // Update customer balance
                let mut balance = self.store.get(tenant_id, &customer_id)
                    .unwrap_or_else(|| CustomerBalance::new(customer_id));
                balance.total_invoiced += e.total_amount;
                balance.outstanding_balance += e.total_amount;
                balance.open_invoice_count += 1;
                self.store.upsert(tenant_id, customer_id, balance);
            }
            InvoiceEvent::PaymentRegistered(e) => {
                // Get customer from mapping
                let mapping_customer = {
                    let mappings = self.invoice_mappings.read().ok();
                    mappings.and_then(|m| m.get(&(tenant_id, aggregate_id)).cloned())
                };

                if let Some(mapping) = mapping_customer {
                    let cid = mapping.customer_id;
                    
                    // Update mapping
                    if let Ok(mut mappings) = self.invoice_mappings.write() {
                        if let Some(m) = mappings.get_mut(&(tenant_id, aggregate_id)) {
                            m.total_paid = e.new_total_paid;
                            if e.new_total_paid >= m.total_amount {
                                m.status = InvoiceStatus::Paid;
                            }
                        }
                    }

                    // Update customer balance
                    if let Some(mut balance) = self.store.get(tenant_id, &cid) {
                        balance.total_paid += e.amount;
                        balance.outstanding_balance = balance.outstanding_balance.saturating_sub(e.amount);
                        if e.new_total_paid >= mapping.total_amount {
                            balance.open_invoice_count = balance.open_invoice_count.saturating_sub(1);
                        }
                        self.store.upsert(tenant_id, cid, balance);
                    }
                }
            }
            InvoiceEvent::InvoiceVoided(e) => {
                // Get customer from mapping
                let mapping = {
                    let mappings = self.invoice_mappings.read().ok();
                    mappings.and_then(|m| m.get(&(tenant_id, aggregate_id)).cloned())
                };

                if let Some(m) = mapping {
                    // Update mapping
                    if let Ok(mut mappings) = self.invoice_mappings.write() {
                        if let Some(mapping) = mappings.get_mut(&(tenant_id, aggregate_id)) {
                            mapping.status = InvoiceStatus::Void;
                        }
                    }

                    // Reverse the outstanding balance
                    if let Some(mut balance) = self.store.get(tenant_id, &m.customer_id) {
                        let outstanding = m.total_amount.saturating_sub(m.total_paid);
                        balance.outstanding_balance = balance.outstanding_balance.saturating_sub(outstanding);
                        balance.open_invoice_count = balance.open_invoice_count.saturating_sub(1);
                        self.store.upsert(tenant_id, m.customer_id, balance);
                    }
                }
                let _ = e; // silence unused warning
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    /// Rebuild the read model from scratch.
    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), CustomerBalanceProjectionError> {
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

        // Clear invoice mappings
        if let Ok(mut mappings) = self.invoice_mappings.write() {
            mappings.clear();
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
    use forgeerp_invoicing::{InvoiceId, InvoiceIssued, InvoiceLine, PaymentRegistered};
    use forgeerp_sales::SalesOrderId;
    use forgeerp_products::ProductId;
    use chrono::Utc;

    fn make_envelope(
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        seq: u64,
        event: InvoiceEvent,
    ) -> EventEnvelope<JsonValue> {
        EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            aggregate_id,
            "invoicing.invoice".to_string(),
            seq,
            serde_json::to_value(&event).unwrap(),
        )
    }

    #[test]
    fn tracks_customer_balance_from_invoice() {
        let store = Arc::new(InMemoryTenantStore::<PartyId, CustomerBalance>::new());
        let proj = CustomerBalancesProjection::new(store.clone());

        let tenant_id = TenantId::new();
        let invoice_id = InvoiceId::new(AggregateId::new());
        let sales_order_id = SalesOrderId::new(AggregateId::new());
        let customer_id = PartyId::new(AggregateId::new());

        // Register customer mapping
        proj.register_invoice_customer(tenant_id, invoice_id.0, customer_id);

        let issued = InvoiceEvent::InvoiceIssued(InvoiceIssued {
            tenant_id,
            invoice_id,
            sales_order_id,
            lines: vec![InvoiceLine {
                line_no: 1,
                sales_order_id,
                product_id: ProductId::new(AggregateId::new()),
                quantity: 2,
                unit_price: 100,
            }],
            due_date: Utc::now(),
            total_amount: 200,
            occurred_at: Utc::now(),
        });

        let env = make_envelope(tenant_id, invoice_id.0, 1, issued);
        proj.apply_envelope(&env).unwrap();

        let balance = proj.get(tenant_id, &customer_id).unwrap();
        assert_eq!(balance.total_invoiced, 200);
        assert_eq!(balance.outstanding_balance, 200);
        assert_eq!(balance.open_invoice_count, 1);
    }

    #[test]
    fn payment_reduces_outstanding_balance() {
        let store = Arc::new(InMemoryTenantStore::<PartyId, CustomerBalance>::new());
        let proj = CustomerBalancesProjection::new(store.clone());

        let tenant_id = TenantId::new();
        let invoice_id = InvoiceId::new(AggregateId::new());
        let sales_order_id = SalesOrderId::new(AggregateId::new());
        let customer_id = PartyId::new(AggregateId::new());

        proj.register_invoice_customer(tenant_id, invoice_id.0, customer_id);

        let issued = InvoiceEvent::InvoiceIssued(InvoiceIssued {
            tenant_id,
            invoice_id,
            sales_order_id,
            lines: vec![InvoiceLine {
                line_no: 1,
                sales_order_id,
                product_id: ProductId::new(AggregateId::new()),
                quantity: 2,
                unit_price: 100,
            }],
            due_date: Utc::now(),
            total_amount: 200,
            occurred_at: Utc::now(),
        });

        proj.apply_envelope(&make_envelope(tenant_id, invoice_id.0, 1, issued)).unwrap();

        let payment = InvoiceEvent::PaymentRegistered(PaymentRegistered {
            tenant_id,
            invoice_id,
            amount: 50,
            new_total_paid: 50,
            occurred_at: Utc::now(),
        });

        proj.apply_envelope(&make_envelope(tenant_id, invoice_id.0, 2, payment)).unwrap();

        let balance = proj.get(tenant_id, &customer_id).unwrap();
        assert_eq!(balance.total_paid, 50);
        assert_eq!(balance.outstanding_balance, 150);
        assert_eq!(balance.open_invoice_count, 1);
    }
}

