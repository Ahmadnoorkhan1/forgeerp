use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_parties::{PartyEvent, PartyId, PartyKind, PartyStatus};

use crate::read_model::TenantStore;
use crate::projections::cursor_store::ProjectionCursorStore;

/// Queryable party read model: basic directory for customers and suppliers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyReadModel {
    pub party_id: PartyId,
    pub kind: PartyKind,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub status: PartyStatus,
}

/// Tenant+aggregate cursor to support at-least-once delivery (idempotent projection).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum PartyProjectionError {
    #[error("failed to deserialize party event: {0}")]
    Deserialize(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

/// Party directory projection.
///
/// Consumes published envelopes (JSON payloads) and maintains a tenant-isolated read model
/// for parties (customers and suppliers), suitable for lookup and basic search.
#[derive(Debug)]
pub struct PartyDirectoryProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<PartyId, PartyReadModel>,
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
        None // Always returns None, forcing use of in-memory cursors
    }

    fn update_cursor(
        &self,
        _tenant_id: TenantId,
        _aggregate_id: AggregateId,
        _projection_name: &str,
        _sequence_number: u64,
    ) {
        // No-op for in-memory store
    }

    fn clear_cursors(&self, _tenant_id: TenantId, _projection_name: &str) {
        // No-op for in-memory store
    }
}

impl<S> PartyDirectoryProjection<S>
where
    S: TenantStore<PartyId, PartyReadModel>,
{
    /// Create a new projection with in-memory cursor tracking.
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "parties.directory".to_string(),
        }
    }
}

impl<S> PartyDirectoryProjection<S>
where
    S: TenantStore<PartyId, PartyReadModel>,
{
    /// Create a new projection with persistent cursor tracking.
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> PartyDirectoryProjection<S, C> {
        PartyDirectoryProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> PartyDirectoryProjection<S, C>
where
    S: TenantStore<PartyId, PartyReadModel>,
    C: ProjectionCursorStore + 'static,
{
}

impl<S, C> PartyDirectoryProjection<S, C>
where
    S: TenantStore<PartyId, PartyReadModel>,
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
                Ok(cursors) => *cursors
                    .get(&CursorKey { tenant_id, aggregate_id })
                    .unwrap_or(&0),
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
            cursor_store.update_cursor(
                tenant_id,
                aggregate_id,
                &self.projection_name,
                sequence_number,
            );
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

    /// Query read model for one tenant/party.
    pub fn get(&self, tenant_id: TenantId, party_id: &PartyId) -> Option<PartyReadModel> {
        self.store.get(tenant_id, party_id)
    }

    /// List all parties for a tenant (disposable read model).
    pub fn list(&self, tenant_id: TenantId) -> Vec<PartyReadModel> {
        self.store.list(tenant_id)
    }

    /// Simple in-memory search by name substring (case-insensitive) for a tenant.
    pub fn search_by_name(&self, tenant_id: TenantId, query: &str) -> Vec<PartyReadModel> {
        let q = query.to_lowercase();
        self.list(tenant_id)
            .into_iter()
            .filter(|rm| rm.name.to_lowercase().contains(&q))
            .collect()
    }

    /// Apply a published envelope into the projection.
    ///
    /// - Ignores non-party aggregates (allows sharing a bus across modules).
    /// - Enforces tenant isolation
    /// - Enforces monotonic sequence per (tenant, aggregate) stream
    /// - Idempotent for at-least-once delivery (replays <= cursor are ignored)
    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), PartyProjectionError> {
        // Ignore non-party aggregates (allows sharing a bus across modules).
        if envelope.aggregate_type() != "parties.party" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        // Cursor check (per tenant + aggregate stream).
        let last = self.get_cursor(tenant_id, aggregate_id);

        if seq == 0 {
            return Err(PartyProjectionError::NonMonotonicSequence { last, found: seq });
        }

        if seq <= last {
            // Duplicate or replay; safe to ignore.
            return Ok(());
        }

        if seq != last + 1 && last != 0 {
            // We allow first event to be any positive sequence (some stores start at 1),
            // but after that we enforce strict monotonic increments.
            return Err(PartyProjectionError::NonMonotonicSequence { last, found: seq });
        }

        // Deserialize the party event from payload.
        let party_event: PartyEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| PartyProjectionError::Deserialize(e.to_string()))?;

        // Validate tenant isolation at the event level.
        let (event_tenant, party_id) = match &party_event {
            PartyEvent::PartyRegistered(e) => (e.tenant_id, e.party_id),
            PartyEvent::PartyUpdated(e) => (e.tenant_id, e.party_id),
            PartyEvent::PartySuspended(e) => (e.tenant_id, e.party_id),
        };

        if event_tenant != tenant_id {
            return Err(PartyProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }

        if party_id.0 != aggregate_id {
            return Err(PartyProjectionError::TenantIsolation(
                "event party_id does not match envelope aggregate_id".to_string(),
            ));
        }

        // Apply update.
        match party_event {
            PartyEvent::PartyRegistered(e) => {
                self.store.upsert(
                    tenant_id,
                    e.party_id,
                    PartyReadModel {
                        party_id: e.party_id,
                        kind: e.kind,
                        name: e.name,
                        email: e.contact.email,
                        phone: e.contact.phone,
                        status: PartyStatus::Active,
                    },
                );
            }
            PartyEvent::PartyUpdated(e) => {
                let mut rm = self.store.get(tenant_id, &e.party_id).unwrap_or(PartyReadModel {
                    party_id: e.party_id,
                    kind: PartyKind::Customer,
                    name: String::new(),
                    email: None,
                    phone: None,
                    status: PartyStatus::Active,
                });
                rm.name = e.name;
                rm.email = e.contact.email;
                rm.phone = e.contact.phone;
                self.store.upsert(tenant_id, e.party_id, rm);
            }
            PartyEvent::PartySuspended(e) => {
                let mut rm = self.store.get(tenant_id, &e.party_id).unwrap_or(PartyReadModel {
                    party_id: e.party_id,
                    kind: PartyKind::Customer,
                    name: String::new(),
                    email: None,
                    phone: None,
                    status: PartyStatus::Active,
                });
                rm.status = PartyStatus::Suspended;
                self.store.upsert(tenant_id, e.party_id, rm);
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
    ) -> Result<(), PartyProjectionError> {
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


