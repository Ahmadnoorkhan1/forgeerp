use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_accounting::{AccountKind, LedgerEvent};

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Read model: per-account balance for a tenant.
///
/// Balances are signed (debit-positive convention).
#[derive(Debug, Clone, PartialEq)]
pub struct AccountBalance {
    pub account_code: String,
    pub account_name: String,
    pub kind: AccountKind,
    pub balance: i128,
}

/// Tenant+aggregate cursor for idempotent projection.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum AccountingProjectionError {
    #[error("failed to deserialize accounting event: {0}")]
    Deserialize(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

/// Projection: ledger → account balances per tenant.
#[derive(Debug)]
pub struct AccountBalancesProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<String, AccountBalance>,
{
    store: S,
    cursors: RwLock<HashMap<CursorKey, u64>>,
    cursor_store: Option<Arc<C>>,
    projection_name: String,
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

impl<S> AccountBalancesProjection<S>
where
    S: TenantStore<String, AccountBalance>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "accounting.balances".to_string(),
        }
    }
}

impl<S> AccountBalancesProjection<S>
where
    S: TenantStore<String, AccountBalance>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> AccountBalancesProjection<S, C> {
        AccountBalancesProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> AccountBalancesProjection<S, C>
where
    S: TenantStore<String, AccountBalance>,
    C: ProjectionCursorStore + 'static,
{
}

impl<S, C> AccountBalancesProjection<S, C>
where
    S: TenantStore<String, AccountBalance>,
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

    /// Get balance for a specific account code.
    pub fn get(&self, tenant_id: TenantId, code: &str) -> Option<AccountBalance> {
        self.store.get(tenant_id, &code.to_string())
    }

    /// List all balances for a tenant.
    pub fn list(&self, tenant_id: TenantId) -> Vec<AccountBalance> {
        self.store.list(tenant_id)
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), AccountingProjectionError> {
        if envelope.aggregate_type() != "accounting.ledger" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);

        if seq == 0 {
            return Err(AccountingProjectionError::NonMonotonicSequence { last, found: seq });
        }

        if seq <= last {
            return Ok(());
        }

        if seq != last + 1 && last != 0 {
            return Err(AccountingProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: LedgerEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| AccountingProjectionError::Deserialize(e.to_string()))?;

        let event_tenant = match &ev {
            LedgerEvent::JournalEntryPosted(e) => e.tenant_id,
        };

        if event_tenant != tenant_id {
            return Err(AccountingProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }

        if let LedgerEvent::JournalEntryPosted(e) = ev {
            for line in &e.lines {
                let code = line.account.code.clone();
                let mut rm = self
                    .store
                    .get(tenant_id, &code)
                    .unwrap_or(AccountBalance {
                        account_code: code.clone(),
                        account_name: line.account.name.clone(),
                        kind: line.account.kind,
                        balance: 0,
                    });

                // Debit positive, credit negative.
                let delta: i128 = if line.is_debit {
                    line.amount as i128
                } else {
                    -(line.amount as i128)
                };
                rm.balance += delta;
                self.store.upsert(tenant_id, code.clone(), rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), AccountingProjectionError> {
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


