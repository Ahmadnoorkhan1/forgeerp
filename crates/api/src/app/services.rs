use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use forgeerp_ai::AiResult;
use forgeerp_core::{AggregateId, DomainError, TenantId, ExpectedVersion};
use forgeerp_events::{EventBus, EventEnvelope, InMemoryEventBus};
use forgeerp_auth::UserId;
use forgeerp_infra::{
    ai::{AiInsightSink, InventoryAnomalyRunner, InventoryAnomalyRunnerHandle},
    command_dispatcher::{CommandDispatcher, DispatchError},
    event_store::{EventFilter, EventQuery, EventQueryResult, InMemoryEventStore, Pagination, StoredEvent, EventStoreError, UncommittedEvent},
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
        UsersDatabaseProjection, DatabaseCredentialStore,
    },
    read_model::InMemoryTenantStore,
    saga::{sales_ar::SalesArSaga, CommandExecutor as SagaCommandExecutor, SagaRepository},
};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

#[cfg(feature = "redis")]
use forgeerp_infra::{
    event_bus::RedisStreamsEventBus,
    event_store::PostgresEventStore,
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

/// Minimal command executor implementation for the Sales→AR saga using the in-memory dispatcher.
struct InMemorySagaExecutor {
    dispatcher: Arc<InMemoryDispatcher>,
    default_ledger_id: AggregateId,
}

impl SagaCommandExecutor for InMemorySagaExecutor {
    type Error = DispatchError;

    fn execute(
        &self,
        _tenant_id: TenantId,
        aggregate_type: &str,
        command_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), Self::Error> {
        match (aggregate_type, command_type) {
            ("Invoice", "IssueInvoice") => {
                // Build IssueInvoice from sales order read model is implemented upstream (saga runner)
                // Here we expect full payload with fields matching IssueInvoice
                let cmd: forgeerp_invoicing::IssueInvoice =
                    serde_json::from_value(payload.clone()).map_err(|e| DispatchError::Validation(e.to_string()))?;
                let _ = self.dispatcher.dispatch::<forgeerp_invoicing::Invoice>(
                    cmd.tenant_id,
                    cmd.invoice_id.0,
                    "invoicing.invoice",
                    forgeerp_invoicing::InvoiceCommand::IssueInvoice(cmd),
                    |_, id| forgeerp_invoicing::Invoice::empty(forgeerp_invoicing::InvoiceId::new(id)),
                )?;
                Ok(())
            }
            ("Ledger", "PostJournalEntry") => {
                let cmd: forgeerp_accounting::PostJournalEntry =
                    serde_json::from_value(payload.clone()).map_err(|e| DispatchError::Validation(e.to_string()))?;
                let _ = self.dispatcher.dispatch::<forgeerp_accounting::Ledger>(
                    cmd.tenant_id,
                    self.default_ledger_id,
                    "accounting.ledger",
                    forgeerp_accounting::JournalCommand::PostJournalEntry(cmd),
                    |_, id| forgeerp_accounting::Ledger::empty(forgeerp_accounting::LedgerId::new(id)),
                )?;
                Ok(())
            }
            ("Invoice", "VoidInvoice") => {
                let cmd: forgeerp_invoicing::VoidInvoice =
                    serde_json::from_value(payload.clone()).map_err(|e| DispatchError::Validation(e.to_string()))?;
                let _ = self.dispatcher.dispatch::<forgeerp_invoicing::Invoice>(
                    cmd.tenant_id,
                    cmd.invoice_id.0,
                    "invoicing.invoice",
                    forgeerp_invoicing::InvoiceCommand::VoidInvoice(cmd),
                    |_, id| forgeerp_invoicing::Invoice::empty(forgeerp_invoicing::InvoiceId::new(id)),
                )?;
                Ok(())
            }
            _ => Err(DispatchError::Validation(format!(
                "Unsupported saga command: {}.{}",
                aggregate_type, command_type
            ))),
        }
    }
}

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
    // Background subscriber: Sales→Invoice→Ledger saga
    {
        let sub = bus.subscribe();
        let saga_repo = SagaRepository::<SalesArSaga, _>::new(store.clone());
        let executor = InMemorySagaExecutor {
            dispatcher: dispatcher.clone(),
            default_ledger_id,
        };
        // Sales orders projection to build invoice lines
        let sales_projection = sales_projection.clone();
        tokio::task::spawn_blocking(move || loop {
            match sub.recv() {
                Ok(env) => {
                    if let Some(correlation) = <SalesArSaga as forgeerp_events::Saga>::correlate(&env) {
                        let tenant_id = env.tenant_id();
                        let saga_id = <SalesArSaga as forgeerp_events::Saga>::saga_id(tenant_id, &correlation);
                        // Rehydrate saga state
                        let mut state = <SalesArSaga as forgeerp_events::Saga>::initial_state(tenant_id, &correlation);
                        if let Ok(history) = saga_repo.load(tenant_id, saga_id) {
                            for se in history {
                                if let Ok(saga_evt) = serde_json::from_value::<forgeerp_infra::saga::sales_ar::SalesArSagaEvent>(se.payload.clone()) {
                                    <SalesArSaga as forgeerp_events::Saga>::apply(&mut state, &saga_evt);
                                }
                            }
                        }
                        // React
                        let actions = <SalesArSaga as forgeerp_events::Saga>::react(&state, tenant_id, &correlation, &env);
                        for action in actions {
                            match action {
                                forgeerp_events::SagaAction::Emit { event_type, payload } => {
                                    let _ = saga_repo.append_emit(tenant_id, saga_id, &event_type, payload);
                                }
                                forgeerp_events::SagaAction::Command { aggregate_type, command_type, mut payload } => {
                                    // Fill IssueInvoice payload from sales read model if missing
                                    if aggregate_type == "Invoice" && command_type == "IssueInvoice" {
                                        if let Some(order) = sales_projection.get(tenant_id, &correlation) {
                                            let invoice_id = forgeerp_invoicing::InvoiceId::new(AggregateId::new());
                                            let lines: Vec<forgeerp_invoicing::InvoiceLine> = order.lines.iter().map(|l| {
                                                forgeerp_invoicing::InvoiceLine {
                                                    line_no: l.line_no,
                                                    sales_order_id: order.order_id,
                                                    product_id: l.product_id,
                                                    quantity: l.quantity,
                                                    unit_price: l.unit_price,
                                                }
                                            }).collect();
                                            let due = chrono::Utc::now() + chrono::Duration::days(30);
                                            let obj = payload.as_object_mut().unwrap();
                                            obj.entry("tenant_id").or_insert(serde_json::json!(tenant_id));
                                            obj.entry("invoice_id").or_insert(serde_json::json!(invoice_id));
                                            obj.entry("lines").or_insert(serde_json::json!(lines));
                                            obj.entry("due_date").or_insert(serde_json::json!(due));
                                            obj.entry("occurred_at").or_insert(serde_json::json!(chrono::Utc::now()));
                                        }
                                    }
                                    let _ = executor.execute(tenant_id, &aggregate_type, &command_type, &payload);
                                }
                                forgeerp_events::SagaAction::Compensate { aggregate_type, command_type, payload } => {
                                    let _ = executor.execute(tenant_id, &aggregate_type, &command_type, &payload);
                                }
                                forgeerp_events::SagaAction::Complete => {
                                    let _ = saga_repo.append_emit(tenant_id, saga_id, "saga.completed", serde_json::json!({}));
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        });
    }
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

    let rm_store = Arc::new(PostgresInventoryStore::new(pool.clone()));
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

    // Database-backed projection for users (persists to user_credentials and users_read_model tables)
    let users_db_projection = Arc::new(UsersDatabaseProjection::new(pool.clone()));
    let _credentials_db_store = Arc::new(DatabaseCredentialStore::new(pool.clone()));

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

    // Spawn separate async task for database projection of users
    {
        let bus = bus.clone();
        let users_db_projection = users_db_projection.clone();
        tokio::spawn(async move {
            let sub = bus.subscribe_with_group(
                "users.database.projection",
                &format!("consumer-users-{}", uuid::Uuid::now_v7()),
                None,
            );
            loop {
                match sub.recv() {
                    Ok(env) => {
                        if let Err(e) = users_db_projection.apply_envelope(&env).await {
                            tracing::warn!("users database projection apply failed: {e}");
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
            // AppServices::Persistent { .. } => {
            //     // Persistent backend with Postgres requires async context
            //     Err(DispatchError::Store(
            //         EventStoreError::InvalidAppend(
            //             "PostgresEventStore requires async context. Use dispatch within async context.".to_string()
            //         )
            //     ))
            // }
        }
    }

    /// Async version of dispatch for Postgres backend.
    /// Must be called from async context.
    pub async fn dispatch_async<A>(
        &self,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: impl Into<String>,
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
            AppServices::Persistent { event_store, bus, .. } => {
                // Direct dispatch without going through EventStore trait
                // This allows us to use async methods directly
                self.dispatch_persistent_async::<A>(
                    event_store.clone(),
                    bus.clone(),
                    tenant_id,
                    aggregate_id,
                    aggregate_type,
                    command,
                    make_aggregate,
                ).await
            }
        }
    }

    #[cfg(feature = "redis")]
    async fn dispatch_persistent_async<A>(
        &self,
        event_store: Arc<PostgresEventStore>,
        bus: Arc<RedisStreamsEventBus>,
        tenant_id: TenantId,
        aggregate_id: AggregateId,
        aggregate_type: impl Into<String>,
        command: A::Command,
        make_aggregate: impl FnOnce(TenantId, AggregateId) -> A,
    ) -> Result<Vec<StoredEvent>, DispatchError>
    where
        A: forgeerp_core::Aggregate<Error = DomainError>,
        A::Event: forgeerp_events::Event + serde::Serialize + serde::de::DeserializeOwned,
    {
        // 1) Load history (tenant-scoped) using async method
        let history = event_store.load_stream(tenant_id, aggregate_id)
            .await
            .map_err(DispatchError::Store)?;

        // Validate loaded stream
        let mut last = 0u64;
        for (idx, e) in history.iter().enumerate() {
            if e.tenant_id != tenant_id {
                return Err(DispatchError::TenantIsolation(format!(
                    "loaded stream contains wrong tenant_id at index {idx}"
                )));
            }
            if e.aggregate_id != aggregate_id {
                return Err(DispatchError::TenantIsolation(format!(
                    "loaded stream contains wrong aggregate_id at index {idx}"
                )));
            }
            if e.sequence_number == 0 {
                return Err(DispatchError::Store(EventStoreError::InvalidAppend(
                    "stored event has sequence_number=0".to_string(),
                )));
            }
            if e.sequence_number <= last {
                return Err(DispatchError::Store(EventStoreError::InvalidAppend(format!(
                    "non-monotonic sequence_number in loaded stream (last={last}, found={})",
                    e.sequence_number
                ))));
            }
            last = e.sequence_number;
        }

        let expected = ExpectedVersion::Exact(last);

        // 2) Rehydrate aggregate
        let mut aggregate = make_aggregate(tenant_id, aggregate_id);
        let mut sorted = history.clone();
        sorted.sort_by_key(|e| e.sequence_number);
        for stored in sorted {
            let ev: A::Event = serde_json::from_value(stored.payload)
                .map_err(|e| DispatchError::Deserialize(e.to_string()))?;
            aggregate.apply(&ev);
        }

        // 3) Decide events (no mutation)
        let decided = aggregate.handle(&command).map_err(DispatchError::from)?;
        if decided.is_empty() {
            return Ok(vec![]);
        }

        // 4) Persist (append-only, optimistic)
        let aggregate_type_str = aggregate_type.into();
        let uncommitted: Vec<_> = decided
            .iter()
            .map(|ev| {
                UncommittedEvent::from_typed(
                    tenant_id,
                    aggregate_id,
                    aggregate_type_str.clone(),
                    Uuid::now_v7(),
                    ev,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let committed = event_store.append_events(tenant_id, aggregate_id, uncommitted, expected)
            .await
            .map_err(DispatchError::Store)?;

        // 5) Publish committed events ke Redis (fire-and-forget).
        //
        // Event sudah aman tersimpan di Postgres (step 4). Publish ke Redis hanya
        // untuk notifikasi real-time ke projection subscribers.
        //
        // MENGAPA fire-and-forget:
        // publish_sync() → redis::get_connection() adalah SYNC BLOCKING.
        // Jika dipanggil langsung (tanpa spawn_blocking) dari async fn ini
        // → Tokio worker thread tersita → request lain (login dll) tidak bisa diproses.
        // Jika dipanggil dengan spawn_blocking + AWAIT → register menunggu Redis,
        // padahal data sudah aman di Postgres → tidak perlu.
        //
        // Dengan fire-and-forget (spawn_blocking tanpa await):
        // - Register langsung return setelah Postgres commit → CEPAT
        // - Worker thread tidak tersita → login tetap bisa diproses → CEPAT
        // - Jika Redis gagal → hanya real-time feed yang terganggu, data tetap aman
        let envelopes: Vec<_> = committed.iter().map(|s| s.to_envelope()).collect();
        let bus_clone = bus.clone();
        tokio::task::spawn_blocking(move || {
            for envelope in envelopes {
                if let Err(e) = bus_clone.publish(envelope) {
                    tracing::warn!("[dispatch] Redis publish failed (non-fatal): {e:?}");
                }
            }
        });
        // Tidak di-await — fire and forget

        Ok(committed)
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

    pub fn inventory_list(&self, tenant_id: TenantId) -> Vec<InventoryReadModel> {
        match self {
            AppServices::InMemory { inventory_projection, .. } => inventory_projection.list(tenant_id),
            #[cfg(feature = "redis")]
            AppServices::Persistent { inventory_projection, .. } => inventory_projection.list(tenant_id),
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

    pub async fn tenants_list(&self) -> Vec<TenantId> {
        match self {
            AppServices::InMemory { event_store, .. } => event_store.list_tenants(),
            #[cfg(feature = "redis")]
            AppServices::Persistent { event_store, .. } => event_store.list_tenants_async().await.unwrap_or_default(),
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

// ─────────────────────────────────────────────────────────────────────────────
// Auth stores (credential, tenant registry, invite)
// ─────────────────────────────────────────────────────────────────────────────

/// Stored credentials for a user (username → hashed password + ids).
#[derive(Debug, Clone)]
pub struct StoredCredential {
    pub username: String,
    pub password_hash: String,
    pub user_id: UserId,
    pub tenant_id: TenantId,
}

/// Stored invite token (short-lived, single-use).
#[derive(Debug, Clone)]
pub struct StoredInvite {
    pub token: String,
    pub tenant_id: TenantId,
    pub tenant_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Thread-safe in-memory credential store.
#[derive(Debug)]
pub struct CredentialStore {
    /// username → credential
    inner: Arc<Mutex<HashMap<String, StoredCredential>>>,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Clone for CredentialStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl CredentialStore {
    pub fn new() -> Self { Self::default() }

    pub fn insert(&self, cred: StoredCredential) {
        let username = cred.username.clone();
        tracing::debug!("[CredentialStore] Inserting credential for username: {}", username);
        self.inner.lock().unwrap().insert(cred.username.clone(), cred);
        let count = self.inner.lock().unwrap().len();
        tracing::debug!("[CredentialStore] Store now contains {} credentials", count);
    }

    pub fn find(&self, username: &str) -> Option<StoredCredential> {
        let store = self.inner.lock().unwrap();
        let count = store.len();
        let exists = store.contains_key(username);
        tracing::debug!("[CredentialStore] Looking for username: '{}' | Store has {} total credentials | Found: {}", username, count, exists);
        if !exists {
            let keys: Vec<_> = store.keys().cloned().collect();
            tracing::debug!("[CredentialStore] Available usernames in store: {:?}", keys);
        }
        drop(store);
        self.inner.lock().unwrap().get(username).cloned()
    }

    /// Find a credential by username, searching across all stored credentials
    /// Returns Option of (credential, tenant_id, user_id) if found
    pub fn find_all(&self, username: &str) -> Vec<StoredCredential> {
        let store = self.inner.lock().unwrap();
        store
            .iter()
            .filter(|(key, _)| key.as_str() == username)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Check if a credential exists
    pub fn exists(&self, username: &str) -> bool {
        self.inner.lock().unwrap().contains_key(username)
    }
}

/// Thread-safe in-memory tenant registry.
#[derive(Debug)]
pub struct TenantRegistry {
    inner: Arc<Mutex<HashMap<TenantId, String>>>,
}

impl Default for TenantRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Clone for TenantRegistry {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl TenantRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn insert(&self, id: TenantId, name: String) {
        self.inner.lock().unwrap().insert(id, name);
    }

    pub fn get_name(&self, id: TenantId) -> Option<String> {
        self.inner.lock().unwrap().get(&id).cloned()
    }
}

/// Thread-safe invite store.
#[derive(Debug)]
pub struct InviteStore {
    inner: Arc<Mutex<HashMap<String, StoredInvite>>>,
}

impl Default for InviteStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Clone for InviteStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl InviteStore {
    pub fn new() -> Self { Self::default() }

    pub fn insert(&self, invite: StoredInvite) {
        self.inner.lock().unwrap().insert(invite.token.clone(), invite);
    }

    /// Validate and consume invite (single-use).
    pub fn consume(&self, token: &str) -> Option<(TenantId, String)> {
        let mut store = self.inner.lock().unwrap();
        let invite = store.get(token)?;
        if chrono::Utc::now() > invite.expires_at {
            return None;
        }
        let result = (invite.tenant_id, invite.tenant_name.clone());
        store.remove(token);
        Some(result)
    }
}

// Shared auth stores injected into AppServices as Extension.
/// Combined auth stores, cheap to clone (all Arc-backed internally).
#[derive(Debug, Clone)]
pub struct AuthStores {
    pub credentials: Arc<CredentialStore>,
    pub tenants: Arc<TenantRegistry>,
    pub invites: Arc<InviteStore>,
}

impl AuthStores {
    pub fn new() -> Self {
        Self {
            credentials: Arc::new(CredentialStore::new()),
            tenants: Arc::new(TenantRegistry::new()),
            invites: Arc::new(InviteStore::new()),
        }
    }
}