use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use forgeerp_ai::AiResult;
use forgeerp_core::{AggregateId, DomainError, TenantId};
use forgeerp_events::{EventBus, EventEnvelope, InMemoryEventBus};
use forgeerp_auth::UserId;
use forgeerp_infra::{
    ai::{AiInsightSink, InventoryAnomalyRunner, InventoryAnomalyRunnerHandle},
    command_dispatcher::{CommandDispatcher, DispatchError},
    event_store::{EventFilter, EventQuery, EventQueryResult, InMemoryEventStore, Pagination, StoredEvent},
    projections::{
        accounting::{AccountBalance, AccountBalancesProjection},
        invoices::{InvoiceReadModel, InvoicesProjection},
        inventory_stock::{InventoryReadModel, InventoryStockProjection},
        invoicing::{InvoiceAgingProjection, InvoiceAgingReadModel},
        parties::{PartyDirectoryProjection, PartyReadModel},
        products::{ProductCatalogProjection, ProductReadModel},
        purchasing::{PurchaseOrderReadModel, PurchaseOrdersProjection},
        sales_orders::{SalesOrderReadModel, SalesOrdersProjection},
        users::{EffectivePermissions, UserReadModel, UsersProjection},
    },
    read_model::InMemoryTenantStore,
};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

#[cfg(feature = "redis")]
use forgeerp_infra::{
    event_bus::RedisStreamsEventBus,
    event_store::{EventFilter, EventQuery, EventQueryResult, Pagination, PostgresEventStore},
    read_model::PostgresInventoryStore,
};
#[cfg(feature = "redis")]
use sqlx::PgPool;

/// Realtime message broadcasted via SSE.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RealtimeMessage {
    pub tenant_id: TenantId,
    pub topic: String,
    pub payload: serde_json::Value,
}

/// API-local AI insight sink that stores results and broadcasts "insight available" notifications.
#[derive(Debug)]
pub struct ApiAiInsightSink {
    inner: Mutex<Vec<(TenantId, AiResult)>>,
    realtime_tx: broadcast::Sender<RealtimeMessage>,
}

impl ApiAiInsightSink {
    pub fn new(realtime_tx: broadcast::Sender<RealtimeMessage>) -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            realtime_tx,
        }
    }

    pub fn all(&self) -> Vec<(TenantId, AiResult)> {
        self.inner.lock().unwrap().clone()
    }
}

impl AiInsightSink for ApiAiInsightSink {
    fn emit(&self, tenant_id: TenantId, result: AiResult) {
        self.inner.lock().unwrap().push((tenant_id, result.clone()));

        // Broadcast that new insights are available (lossy; no backpressure on core).
        let _ = self.realtime_tx.send(RealtimeMessage {
            tenant_id,
            topic: "ai.insight_available".to_string(),
            payload: serde_json::json!({
                "kind": "insights",
                "insight_type": "ai.result",
                "metadata": result.metadata,
            }),
        });
    }
}

// Type-erased dispatcher for in-memory implementations
type InMemoryDispatcher = CommandDispatcher<
    Arc<InMemoryEventStore>,
    Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>,
>;

// Type-erased dispatcher for persistent implementations
#[cfg(feature = "redis")]
type PersistentDispatcher = CommandDispatcher<Arc<PostgresEventStore>, Arc<RedisStreamsEventBus>>;

#[derive(Clone)]
pub enum AppServices {
    InMemory {
        dispatcher: Arc<InMemoryDispatcher>,
        event_store: Arc<InMemoryEventStore>,
        event_bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>,
        inventory_projection: Arc<
            InventoryStockProjection<Arc<InMemoryTenantStore<forgeerp_inventory::InventoryItemId, InventoryReadModel>>>,
        >,
        parties_projection: Arc<
            PartyDirectoryProjection<Arc<InMemoryTenantStore<forgeerp_parties::PartyId, PartyReadModel>>>,
        >,
        products_projection: Arc<
            ProductCatalogProjection<Arc<InMemoryTenantStore<forgeerp_products::ProductId, ProductReadModel>>>,
        >,
        sales_projection: Arc<
            SalesOrdersProjection<Arc<InMemoryTenantStore<forgeerp_sales::SalesOrderId, SalesOrderReadModel>>>,
        >,
        invoices_projection: Arc<
            InvoicesProjection<Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceReadModel>>>,
        >,
        ar_aging_projection: Arc<
            InvoiceAgingProjection<Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceAgingReadModel>>>,
        >,
        purchases_projection: Arc<
            PurchaseOrdersProjection<Arc<InMemoryTenantStore<forgeerp_purchasing::PurchaseOrderId, PurchaseOrderReadModel>>>,
        >,
        ledger_projection: Arc<AccountBalancesProjection<Arc<InMemoryTenantStore<String, AccountBalance>>>>,
        users_projection: Arc<UsersProjection<Arc<InMemoryTenantStore<UserId, UserReadModel>>>>,
        default_ledger_id: AggregateId,
        ai_sink: Arc<ApiAiInsightSink>,
        realtime_tx: broadcast::Sender<RealtimeMessage>,
    },
    #[cfg(feature = "redis")]
    Persistent {
        dispatcher: Arc<PersistentDispatcher>,
        event_store: Arc<PostgresEventStore>,
        inventory_projection: Arc<InventoryStockProjection<Arc<PostgresInventoryStore>>>,
        parties_projection: Arc<
            PartyDirectoryProjection<Arc<InMemoryTenantStore<forgeerp_parties::PartyId, PartyReadModel>>>,
        >,
        products_projection: Arc<
            ProductCatalogProjection<Arc<InMemoryTenantStore<forgeerp_products::ProductId, ProductReadModel>>>,
        >,
        sales_projection: Arc<
            SalesOrdersProjection<Arc<InMemoryTenantStore<forgeerp_sales::SalesOrderId, SalesOrderReadModel>>>,
        >,
        invoices_projection: Arc<
            InvoicesProjection<Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceReadModel>>>,
        >,
        ar_aging_projection: Arc<
            InvoiceAgingProjection<Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceAgingReadModel>>>,
        >,
        purchases_projection: Arc<
            PurchaseOrdersProjection<Arc<InMemoryTenantStore<forgeerp_purchasing::PurchaseOrderId, PurchaseOrderReadModel>>>,
        >,
        ledger_projection: Arc<AccountBalancesProjection<Arc<InMemoryTenantStore<String, AccountBalance>>>>,
        users_projection: Arc<UsersProjection<Arc<InMemoryTenantStore<UserId, UserReadModel>>>>,
        default_ledger_id: AggregateId,
        ai_sink: Arc<ApiAiInsightSink>,
        realtime_tx: broadcast::Sender<RealtimeMessage>,
        bus: Arc<RedisStreamsEventBus>,
    },
}

pub async fn build_services() -> AppServices {
    let use_persistent = std::env::var("USE_PERSISTENT_STORES")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    if use_persistent {
        #[cfg(feature = "redis")]
        {
            return build_persistent_services().await;
        }
        #[cfg(not(feature = "redis"))]
        {
            tracing::warn!(
                "USE_PERSISTENT_STORES=true but redis feature not enabled, falling back to in-memory"
            );
            return build_in_memory_services();
        }
    }

    build_in_memory_services()
}

fn build_in_memory_services() -> AppServices {
    // In-memory infra wiring (dev/test): store + bus + projection.
    let store = Arc::new(InMemoryEventStore::new());
    let bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>> = Arc::new(InMemoryEventBus::new());

    let rm_store: Arc<InMemoryTenantStore<forgeerp_inventory::InventoryItemId, InventoryReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let inventory_projection: Arc<InventoryStockProjection<_>> =
        Arc::new(InventoryStockProjection::new(rm_store));

    let parties_store: Arc<InMemoryTenantStore<forgeerp_parties::PartyId, PartyReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let parties_projection: Arc<PartyDirectoryProjection<_>> =
        Arc::new(PartyDirectoryProjection::new(parties_store));

    let products_store: Arc<InMemoryTenantStore<forgeerp_products::ProductId, ProductReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let products_projection: Arc<ProductCatalogProjection<_>> =
        Arc::new(ProductCatalogProjection::new(products_store));

    let sales_store: Arc<InMemoryTenantStore<forgeerp_sales::SalesOrderId, SalesOrderReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let sales_projection: Arc<SalesOrdersProjection<_>> =
        Arc::new(SalesOrdersProjection::new(sales_store));

    let invoices_store: Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let invoices_projection: Arc<InvoicesProjection<_>> =
        Arc::new(InvoicesProjection::new(invoices_store));

    let ar_aging_store: Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceAgingReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let ar_aging_projection: Arc<InvoiceAgingProjection<_>> =
        Arc::new(InvoiceAgingProjection::new(ar_aging_store));

    let purchases_store: Arc<
        InMemoryTenantStore<forgeerp_purchasing::PurchaseOrderId, PurchaseOrderReadModel>,
    > = Arc::new(InMemoryTenantStore::new());
    let purchases_projection: Arc<PurchaseOrdersProjection<_>> =
        Arc::new(PurchaseOrdersProjection::new(purchases_store));

    let ledger_store: Arc<InMemoryTenantStore<String, AccountBalance>> = Arc::new(InMemoryTenantStore::new());
    let ledger_projection: Arc<AccountBalancesProjection<_>> =
        Arc::new(AccountBalancesProjection::new(ledger_store));

    let users_store: Arc<InMemoryTenantStore<UserId, UserReadModel>> = Arc::new(InMemoryTenantStore::new());
    let users_projection: Arc<UsersProjection<_>> = Arc::new(UsersProjection::new(users_store));

    let default_ledger_id = AggregateId::new();

    // Realtime channel (SSE): lossy broadcast, tenant-filtered in handlers.
    let (realtime_tx, _realtime_rx) = broadcast::channel::<RealtimeMessage>(256);

    // AI wiring (dev/test): in-memory insights + per-tenant anomaly runners.
    let ai_sink: Arc<ApiAiInsightSink> = Arc::new(ApiAiInsightSink::new(realtime_tx.clone()));
    let ai_runners: Arc<Mutex<HashMap<TenantId, InventoryAnomalyRunnerHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let ai_runner_cfg = InventoryAnomalyRunner::default();

    // Background subscriber: bus -> projections
    {
        let sub = bus.subscribe();
        let inventory_projection = inventory_projection.clone();
        let parties_projection = parties_projection.clone();
        let products_projection = products_projection.clone();
        let sales_projection = sales_projection.clone();
        let invoices_projection = invoices_projection.clone();
        let ar_aging_projection = ar_aging_projection.clone();
        let purchases_projection = purchases_projection.clone();
        let ledger_projection = ledger_projection.clone();
        let users_projection = users_projection.clone();
        let ai_sink = ai_sink.clone();
        let ai_runners = ai_runners.clone();
        let realtime_tx = realtime_tx.clone();
        tokio::task::spawn_blocking(move || loop {
            match sub.recv() {
                Ok(env) => {
                    let at = env.aggregate_type();

                    // Apply to the relevant projection(s) only.
                    let apply_ok = match at {
                        "inventory.item" => inventory_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "parties.party" => parties_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "products.product" => products_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "sales.order" => sales_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "invoicing.invoice" => {
                            if let Err(e) = invoices_projection.apply_envelope(&env) {
                                Err(e.to_string())
                            } else if let Err(e) = ar_aging_projection.apply_envelope(&env) {
                                Err(e.to_string())
                            } else {
                                Ok(())
                            }
                        }
                        "purchasing.order" => purchases_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "accounting.ledger" => ledger_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        "auth.user" => users_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                        _ => Ok(()),
                    };

                    if let Err(e) = apply_ok {
                        tracing::warn!("projection apply failed: {e}");
                        continue;
                    }

                    // Broadcast projection update (lossy; no backpressure on core).
                    let _ = realtime_tx.send(RealtimeMessage {
                        tenant_id: env.tenant_id(),
                        topic: format!("{at}.projection_updated"),
                        payload: serde_json::json!({
                            "kind": "projection_update",
                            "aggregate_type": at,
                            "aggregate_id": env.aggregate_id().to_string(),
                            "sequence_number": env.sequence_number(),
                        }),
                    });

                    // Event-triggered AI execution only for inventory updates.
                    if at == "inventory.item" {
                        let tenant_id = env.tenant_id();
                        let mut runners = ai_runners.lock().unwrap();
                        let handle = runners.entry(tenant_id).or_insert_with(|| {
                            ai_runner_cfg.spawn_for_tenant(
                                "ai.inventory_anomaly",
                                tenant_id,
                                inventory_projection.clone(),
                                ai_sink.clone(),
                            )
                        });
                        handle.trigger();
                    }
                }
                Err(_) => break,
            }
        });
    }

    let dispatcher: Arc<InMemoryDispatcher> = Arc::new(CommandDispatcher::new(store.clone(), bus.clone()));
    AppServices::InMemory {
        dispatcher,
        event_store: store,
        event_bus: bus,
        inventory_projection,
        parties_projection,
        products_projection,
        sales_projection,
        invoices_projection,
        ar_aging_projection,
        purchases_projection,
        ledger_projection,
        users_projection,
        default_ledger_id,
        ai_sink,
        realtime_tx,
    }
}

#[cfg(feature = "redis")]
async fn build_persistent_services() -> AppServices {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set when USE_PERSISTENT_STORES=true");
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    let store = Arc::new(PostgresEventStore::new(pool.clone()));

    let bus = Arc::new(
        RedisStreamsEventBus::new(&redis_url, None, None).expect("Failed to create Redis Streams event bus"),
    );

    bus.ensure_consumer_group("inventory.projection")
        .expect("Failed to create consumer group");

    let rm_store = Arc::new(PostgresInventoryStore::new(pool));
    let inventory_projection: Arc<InventoryStockProjection<_>> =
        Arc::new(InventoryStockProjection::new(rm_store));

    // Other projections currently use in-memory read models (can be swapped to Postgres later).
    let parties_store: Arc<InMemoryTenantStore<forgeerp_parties::PartyId, PartyReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let parties_projection: Arc<PartyDirectoryProjection<_>> =
        Arc::new(PartyDirectoryProjection::new(parties_store));

    let products_store: Arc<InMemoryTenantStore<forgeerp_products::ProductId, ProductReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let products_projection: Arc<ProductCatalogProjection<_>> =
        Arc::new(ProductCatalogProjection::new(products_store));

    let sales_store: Arc<InMemoryTenantStore<forgeerp_sales::SalesOrderId, SalesOrderReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let sales_projection: Arc<SalesOrdersProjection<_>> =
        Arc::new(SalesOrdersProjection::new(sales_store));

    let invoices_store: Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let invoices_projection: Arc<InvoicesProjection<_>> =
        Arc::new(InvoicesProjection::new(invoices_store));

    let ar_aging_store: Arc<InMemoryTenantStore<forgeerp_invoicing::InvoiceId, InvoiceAgingReadModel>> =
        Arc::new(InMemoryTenantStore::new());
    let ar_aging_projection: Arc<InvoiceAgingProjection<_>> =
        Arc::new(InvoiceAgingProjection::new(ar_aging_store));

    let purchases_store: Arc<
        InMemoryTenantStore<forgeerp_purchasing::PurchaseOrderId, PurchaseOrderReadModel>,
    > = Arc::new(InMemoryTenantStore::new());
    let purchases_projection: Arc<PurchaseOrdersProjection<_>> =
        Arc::new(PurchaseOrdersProjection::new(purchases_store));

    let ledger_store: Arc<InMemoryTenantStore<String, AccountBalance>> = Arc::new(InMemoryTenantStore::new());
    let ledger_projection: Arc<AccountBalancesProjection<_>> =
        Arc::new(AccountBalancesProjection::new(ledger_store));

    let users_store: Arc<InMemoryTenantStore<UserId, UserReadModel>> = Arc::new(InMemoryTenantStore::new());
    let users_projection: Arc<UsersProjection<_>> = Arc::new(UsersProjection::new(users_store));

    let default_ledger_id = AggregateId::new();

    let (realtime_tx, _realtime_rx) = broadcast::channel::<RealtimeMessage>(256);

    let ai_sink: Arc<ApiAiInsightSink> = Arc::new(ApiAiInsightSink::new(realtime_tx.clone()));
    let ai_runners: Arc<Mutex<HashMap<TenantId, InventoryAnomalyRunnerHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let ai_runner_cfg = InventoryAnomalyRunner::default();

    {
        let bus = bus.clone();
        let inventory_projection = inventory_projection.clone();
        let parties_projection = parties_projection.clone();
        let products_projection = products_projection.clone();
        let sales_projection = sales_projection.clone();
        let invoices_projection = invoices_projection.clone();
        let ar_aging_projection = ar_aging_projection.clone();
        let purchases_projection = purchases_projection.clone();
        let ledger_projection = ledger_projection.clone();
        let users_projection = users_projection.clone();
        let ai_sink = ai_sink.clone();
        let ai_runners = ai_runners.clone();
        let realtime_tx = realtime_tx.clone();
        tokio::task::spawn_blocking(move || {
            let sub = bus.subscribe_with_group(
                "inventory.projection",
                &format!("consumer-{}", uuid::Uuid::now_v7()),
                None,
            );
            loop {
                match sub.recv() {
                    Ok(env) => {
                        let at = env.aggregate_type();

                        let apply_ok = match at {
                            "inventory.item" => inventory_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "parties.party" => parties_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "products.product" => products_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "sales.order" => sales_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "invoicing.invoice" => {
                                if let Err(e) = invoices_projection.apply_envelope(&env) {
                                    Err(e.to_string())
                                } else if let Err(e) = ar_aging_projection.apply_envelope(&env) {
                                    Err(e.to_string())
                                } else {
                                    Ok(())
                                }
                            }
                            "purchasing.order" => purchases_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "accounting.ledger" => ledger_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            "auth.user" => users_projection.apply_envelope(&env).map_err(|e| e.to_string()),
                            _ => Ok(()),
                        };

                        if let Err(e) = apply_ok {
                            tracing::warn!("projection apply failed: {e}");
                            continue;
                        }

                        let _ = realtime_tx.send(RealtimeMessage {
                            tenant_id: env.tenant_id(),
                            topic: format!("{at}.projection_updated"),
                            payload: serde_json::json!({
                                "kind": "projection_update",
                                "aggregate_type": at,
                                "aggregate_id": env.aggregate_id().to_string(),
                                "sequence_number": env.sequence_number(),
                            }),
                        });

                        if at == "inventory.item" {
                            let tenant_id = env.tenant_id();
                            let mut runners = ai_runners.lock().unwrap();
                            let handle = runners.entry(tenant_id).or_insert_with(|| {
                                ai_runner_cfg.spawn_for_tenant(
                                    "ai.inventory_anomaly",
                                    tenant_id,
                                    inventory_projection.clone(),
                                    ai_sink.clone(),
                                )
                            });
                            handle.trigger();
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    let dispatcher: Arc<PersistentDispatcher> = Arc::new(CommandDispatcher::new(store.clone(), bus.clone()));
    AppServices::Persistent {
        dispatcher,
        event_store: store,
        inventory_projection,
        parties_projection,
        products_projection,
        sales_projection,
        invoices_projection,
        ar_aging_projection,
        purchases_projection,
        ledger_projection,
        users_projection,
        default_ledger_id,
        ai_sink,
        realtime_tx,
        bus,
    }
}

impl AppServices {
    pub fn realtime_tx(&self) -> &broadcast::Sender<RealtimeMessage> {
        match self {
            AppServices::InMemory { realtime_tx, .. } => realtime_tx,
            #[cfg(feature = "redis")]
            AppServices::Persistent { realtime_tx, .. } => realtime_tx,
        }
    }

    pub fn ai_sink(&self) -> &Arc<ApiAiInsightSink> {
        match self {
            AppServices::InMemory { ai_sink, .. } => ai_sink,
            #[cfg(feature = "redis")]
            AppServices::Persistent { ai_sink, .. } => ai_sink,
        }
    }

    pub fn default_ledger_id(&self) -> AggregateId {
        match self {
            AppServices::InMemory { default_ledger_id, .. } => *default_ledger_id,
            #[cfg(feature = "redis")]
            AppServices::Persistent { default_ledger_id, .. } => *default_ledger_id,
        }
    }

    pub fn dispatch<A>(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: &str,
        command: A::Command,
        make_aggregate: impl FnOnce(TenantId, AggregateId) -> A,
    ) -> Result<Vec<StoredEvent>, DispatchError>
    where
        A: forgeerp_core::Aggregate<Error = DomainError>,
        A::Event: forgeerp_events::Event + serde::Serialize + serde::de::DeserializeOwned,
    {
        match self {
            AppServices::InMemory { dispatcher, .. } => dispatcher.dispatch::<A>(
                tenant_id,
                aggregate_id,
                aggregate_type,
                command,
                make_aggregate,
            ),
            #[cfg(feature = "redis")]
            AppServices::Persistent { dispatcher, .. } => dispatcher.dispatch::<A>(
                tenant_id,
                aggregate_id,
                aggregate_type,
                command,
                make_aggregate,
            ),
        }
    }

    pub fn inventory_get(
        &self,
        tenant_id: TenantId,
        item_id: &forgeerp_inventory::InventoryItemId,
    ) -> Option<InventoryReadModel> {
        match self {
            AppServices::InMemory { inventory_projection, .. } => inventory_projection.get(tenant_id, item_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { inventory_projection, .. } => inventory_projection.get(tenant_id, item_id),
        }
    }

    pub fn products_get(
        &self,
        tenant_id: TenantId,
        product_id: &forgeerp_products::ProductId,
    ) -> Option<ProductReadModel> {
        match self {
            AppServices::InMemory { products_projection, .. } => products_projection.get(tenant_id, product_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { products_projection, .. } => products_projection.get(tenant_id, product_id),
        }
    }

    pub fn products_list(&self, tenant_id: TenantId) -> Vec<ProductReadModel> {
        match self {
            AppServices::InMemory { products_projection, .. } => products_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { products_projection, .. } => products_projection.list(tenant_id),
        }
    }

    pub fn parties_get(
        &self,
        tenant_id: TenantId,
        party_id: &forgeerp_parties::PartyId,
    ) -> Option<PartyReadModel> {
        match self {
            AppServices::InMemory { parties_projection, .. } => parties_projection.get(tenant_id, party_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { parties_projection, .. } => parties_projection.get(tenant_id, party_id),
        }
    }

    pub fn parties_list(&self, tenant_id: TenantId) -> Vec<PartyReadModel> {
        match self {
            AppServices::InMemory { parties_projection, .. } => parties_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { parties_projection, .. } => parties_projection.list(tenant_id),
        }
    }

    pub fn sales_get(
        &self,
        tenant_id: TenantId,
        order_id: &forgeerp_sales::SalesOrderId,
    ) -> Option<SalesOrderReadModel> {
        match self {
            AppServices::InMemory { sales_projection, .. } => sales_projection.get(tenant_id, order_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { sales_projection, .. } => sales_projection.get(tenant_id, order_id),
        }
    }

    pub fn sales_list(&self, tenant_id: TenantId) -> Vec<SalesOrderReadModel> {
        match self {
            AppServices::InMemory { sales_projection, .. } => sales_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { sales_projection, .. } => sales_projection.list(tenant_id),
        }
    }

    pub fn invoices_get(
        &self,
        tenant_id: TenantId,
        invoice_id: &forgeerp_invoicing::InvoiceId,
    ) -> Option<InvoiceReadModel> {
        match self {
            AppServices::InMemory { invoices_projection, .. } => invoices_projection.get(tenant_id, invoice_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { invoices_projection, .. } => invoices_projection.get(tenant_id, invoice_id),
        }
    }

    pub fn invoices_list(&self, tenant_id: TenantId) -> Vec<InvoiceReadModel> {
        match self {
            AppServices::InMemory { invoices_projection, .. } => invoices_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { invoices_projection, .. } => invoices_projection.list(tenant_id),
        }
    }

    pub fn ar_aging_list(&self, tenant_id: TenantId) -> Vec<InvoiceAgingReadModel> {
        match self {
            AppServices::InMemory { ar_aging_projection, .. } => ar_aging_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { ar_aging_projection, .. } => ar_aging_projection.list(tenant_id),
        }
    }

    pub fn purchases_get(
        &self,
        tenant_id: TenantId,
        order_id: &forgeerp_purchasing::PurchaseOrderId,
    ) -> Option<PurchaseOrderReadModel> {
        match self {
            AppServices::InMemory { purchases_projection, .. } => purchases_projection.get(tenant_id, order_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { purchases_projection, .. } => purchases_projection.get(tenant_id, order_id),
        }
    }

    pub fn purchases_list(&self, tenant_id: TenantId) -> Vec<PurchaseOrderReadModel> {
        match self {
            AppServices::InMemory { purchases_projection, .. } => purchases_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { purchases_projection, .. } => purchases_projection.list(tenant_id),
        }
    }

    pub fn ledger_balances_list(&self, tenant_id: TenantId) -> Vec<AccountBalance> {
        match self {
            AppServices::InMemory { ledger_projection, .. } => ledger_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { ledger_projection, .. } => ledger_projection.list(tenant_id),
        }
    }

    pub fn ledger_balance_get(&self, tenant_id: TenantId, code: &str) -> Option<AccountBalance> {
        match self {
            AppServices::InMemory { ledger_projection, .. } => ledger_projection.get(tenant_id, code),
            #[cfg(feature = "redis")]
            AppServices::Persistent { ledger_projection, .. } => ledger_projection.get(tenant_id, code),
        }
    }

    pub fn users_get(&self, tenant_id: TenantId, user_id: &UserId) -> Option<UserReadModel> {
        match self {
            AppServices::InMemory { users_projection, .. } => users_projection.get(tenant_id, user_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { users_projection, .. } => users_projection.get(tenant_id, user_id),
        }
    }

    pub fn users_list(&self, tenant_id: TenantId) -> Vec<UserReadModel> {
        match self {
            AppServices::InMemory { users_projection, .. } => users_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { users_projection, .. } => users_projection.list(tenant_id),
        }
    }

    pub fn users_effective_permissions<F>(
        &self,
        tenant_id: TenantId,
        user_id: &UserId,
        role_permissions: F,
    ) -> Option<EffectivePermissions>
    where
        F: Fn(&str) -> Vec<String>,
    {
        match self {
            AppServices::InMemory { users_projection, .. } => {
                users_projection.effective_permissions(tenant_id, user_id, role_permissions)
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { users_projection, .. } => {
                users_projection.effective_permissions(tenant_id, user_id, role_permissions)
            }
        }
    }

    /// Query events with filters and pagination.
    pub async fn query_events(
        &self,
        tenant_id: TenantId,
        filter: EventFilter,
        pagination: Pagination,
    ) -> Result<EventQueryResult, forgeerp_infra::event_store::EventStoreError> {
        match self {
            AppServices::InMemory { event_store, .. } => {
                event_store.query_events(tenant_id, filter, pagination).await
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { event_store, .. } => {
                event_store.query_events(tenant_id, filter, pagination).await
            }
        }
    }

    /// Get events for a specific aggregate.
    pub async fn get_aggregate_events(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        pagination: Option<Pagination>,
    ) -> Result<EventQueryResult, forgeerp_infra::event_store::EventStoreError> {
        match self {
            AppServices::InMemory { event_store, .. } => {
                event_store.get_aggregate_events(tenant_id, aggregate_id, pagination).await
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { event_store, .. } => {
                event_store.get_aggregate_events(tenant_id, aggregate_id, pagination).await
            }
        }
    }

    /// Get a single event by its ID.
    pub async fn get_event_by_id(
        &self,
        tenant_id: TenantId,
        event_id: uuid::Uuid,
    ) -> Result<Option<StoredEvent>, forgeerp_infra::event_store::EventStoreError> {
        match self {
            AppServices::InMemory { event_store, .. } => {
                event_store.get_event_by_id(tenant_id, event_id).await
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { event_store, .. } => {
                event_store.get_event_by_id(tenant_id, event_id).await
            }
        }
    }

    /// Get the event store for replay operations (InMemory).
    pub fn event_store_in_memory(&self) -> Option<Arc<InMemoryEventStore>> {
        match self {
            AppServices::InMemory { event_store, .. } => Some(event_store.clone()),
            #[cfg(feature = "redis")]
            AppServices::Persistent { .. } => None,
        }
    }

    /// Get the event store for replay operations (Postgres).
    #[cfg(feature = "redis")]
    pub fn event_store_persistent(&self) -> Option<Arc<PostgresEventStore>> {
        match self {
            AppServices::InMemory { .. } => None,
            AppServices::Persistent { event_store, .. } => Some(event_store.clone()),
        }
    }
}

/// Build an SSE stream for a tenant (used by `/stream`).
pub fn tenant_sse_stream(
    services: Arc<AppServices>,
    tenant_id: TenantId,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = services.realtime_tx().subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(m) if m.tenant_id == tenant_id => {
            let data = serde_json::to_string(&m.payload).unwrap_or_else(|_| "{}".to_string());
            Some(Ok(SseEvent::default().event(m.topic).data(data)))
        }
        _ => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}


