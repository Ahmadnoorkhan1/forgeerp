use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value as JsonValue;
use thiserror::Error;

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_products::{ProductEvent, ProductId, ProductStatus};
use forgeerp_products::product::PricingMetadata;

use crate::projections::cursor_store::ProjectionCursorStore;
use crate::read_model::TenantStore;

/// Queryable product read model (catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReadModel {
    pub product_id: ProductId,
    pub sku: String,
    pub name: String,
    pub status: ProductStatus,
    pub pricing: PricingMetadata,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
struct CursorKey {
    tenant_id: TenantId,
    aggregate_id: AggregateId,
}

#[derive(Debug, Error)]
pub enum ProductProjectionError {
    #[error("failed to deserialize product event: {0}")]
    Deserialize(String),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),

    #[error("non-monotonic sequence number (last={last}, found={found})")]
    NonMonotonicSequence { last: u64, found: u64 },
}

#[derive(Debug)]
pub struct ProductCatalogProjection<S, C = InMemoryCursorStore>
where
    S: TenantStore<ProductId, ProductReadModel>,
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

impl<S> ProductCatalogProjection<S>
where
    S: TenantStore<ProductId, ProductReadModel>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: None,
            projection_name: "products.catalog".to_string(),
        }
    }
}

impl<S> ProductCatalogProjection<S>
where
    S: TenantStore<ProductId, ProductReadModel>,
{
    pub fn with_persistent_cursors<C: ProjectionCursorStore + 'static>(
        self,
        cursor_store: Arc<C>,
        projection_name: impl Into<String>,
    ) -> ProductCatalogProjection<S, C> {
        ProductCatalogProjection {
            store: self.store,
            cursors: RwLock::new(HashMap::new()),
            cursor_store: Some(cursor_store),
            projection_name: projection_name.into(),
        }
    }
}

impl<S, C> ProductCatalogProjection<S, C>
where
    S: TenantStore<ProductId, ProductReadModel>,
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
            cursor_store.update_cursor(tenant_id, aggregate_id, &self.projection_name, sequence_number);
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

    pub fn get(&self, tenant_id: TenantId, product_id: &ProductId) -> Option<ProductReadModel> {
        self.store.get(tenant_id, product_id)
    }

    pub fn list(&self, tenant_id: TenantId) -> Vec<ProductReadModel> {
        self.store.list(tenant_id)
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<JsonValue>,
    ) -> Result<(), ProductProjectionError> {
        if envelope.aggregate_type() != "products.product" {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        let last = self.get_cursor(tenant_id, aggregate_id);
        if seq == 0 {
            return Err(ProductProjectionError::NonMonotonicSequence { last, found: seq });
        }
        if seq <= last {
            return Ok(());
        }
        if seq != last + 1 && last != 0 {
            return Err(ProductProjectionError::NonMonotonicSequence { last, found: seq });
        }

        let ev: ProductEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| ProductProjectionError::Deserialize(e.to_string()))?;

        let (event_tenant, product_id) = match &ev {
            ProductEvent::ProductCreated(e) => (e.tenant_id, e.product_id),
            ProductEvent::ProductActivated(e) => (e.tenant_id, e.product_id),
            ProductEvent::ProductArchived(e) => (e.tenant_id, e.product_id),
        };

        if event_tenant != tenant_id {
            return Err(ProductProjectionError::TenantIsolation(
                "event tenant_id does not match envelope tenant_id".to_string(),
            ));
        }
        if product_id.0 != aggregate_id {
            return Err(ProductProjectionError::TenantIsolation(
                "event product_id does not match envelope aggregate_id".to_string(),
            ));
        }

        match ev {
            ProductEvent::ProductCreated(e) => {
                self.store.upsert(
                    tenant_id,
                    e.product_id,
                    ProductReadModel {
                        product_id: e.product_id,
                        sku: e.sku,
                        name: e.name,
                        status: ProductStatus::Draft,
                        pricing: e.pricing,
                    },
                );
            }
            ProductEvent::ProductActivated(e) => {
                let mut rm = self.store.get(tenant_id, &e.product_id).unwrap_or(ProductReadModel {
                    product_id: e.product_id,
                    sku: String::new(),
                    name: String::new(),
                    status: ProductStatus::Draft,
                    pricing: PricingMetadata::default(),
                });
                rm.status = ProductStatus::Active;
                self.store.upsert(tenant_id, e.product_id, rm);
            }
            ProductEvent::ProductArchived(e) => {
                let mut rm = self.store.get(tenant_id, &e.product_id).unwrap_or(ProductReadModel {
                    product_id: e.product_id,
                    sku: String::new(),
                    name: String::new(),
                    status: ProductStatus::Draft,
                    pricing: PricingMetadata::default(),
                });
                rm.status = ProductStatus::Archived;
                self.store.upsert(tenant_id, e.product_id, rm);
            }
        }

        self.update_cursor(tenant_id, aggregate_id, seq);
        Ok(())
    }

    pub fn rebuild_from_scratch(
        &self,
        envelopes: impl IntoIterator<Item = EventEnvelope<JsonValue>>,
    ) -> Result<(), ProductProjectionError> {
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


