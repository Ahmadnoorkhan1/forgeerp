use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use chrono::Utc;
use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::EventEnvelope;
use forgeerp_events::InMemoryEventBus;
use forgeerp_infra::command_dispatcher::CommandDispatcher;
use forgeerp_infra::event_store::{EventStore, InMemoryEventStore, UncommittedEvent};
use forgeerp_infra::projections::inventory_stock::InventoryStockProjection;
use forgeerp_infra::read_model::InMemoryTenantStore;
use forgeerp_inventory::{
    AdjustStock, CreateItem, InventoryCommand, InventoryEvent, InventoryItem, InventoryItemId,
    ItemCreated, StockAdjusted,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Naive CRUD simulation: direct key-value updates (no events, no history).
#[derive(Debug, Clone)]
struct NaiveCrudStore {
    inner: Arc<RwLock<HashMap<(TenantId, AggregateId), CrudState>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CrudState {
    name: String,
    quantity: i64,
    version: u64, // For optimistic concurrency (not used in benchmarks)
}

impl NaiveCrudStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn create(&self, tenant_id: TenantId, item_id: AggregateId, name: String) {
        let mut map = self.inner.write().unwrap();
        map.insert(
            (tenant_id, item_id),
            CrudState {
                name,
                quantity: 0,
                version: 1,
            },
        );
    }

    fn adjust_stock(&self, tenant_id: TenantId, item_id: AggregateId, delta: i64) -> Result<(), ()> {
        let mut map = self.inner.write().unwrap();
        if let Some(state) = map.get_mut(&(tenant_id, item_id)) {
            let new_qty = state.quantity + delta;
            if new_qty < 0 {
                return Err(());
            }
            state.quantity = new_qty;
            state.version += 1;
            Ok(())
        } else {
            Err(())
        }
    }

}

fn setup_event_sourcing() -> (
    CommandDispatcher<InMemoryEventStore, Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>>,
    TenantId,
    AggregateId,
) {
    let store = InMemoryEventStore::new();
    let bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>> = Arc::new(InMemoryEventBus::new());
    let dispatcher = CommandDispatcher::new(store, bus);
    let tenant_id = TenantId::new();
    let item_id = AggregateId::new();
    (dispatcher, tenant_id, item_id)
}

fn bench_command_execution_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("command_execution_latency");
    group.sample_size(1000);

    // Benchmark: CreateItem command (first command, no history)
    group.bench_function("create_item_fresh", |b| {
        let (dispatcher, tenant_id, _) = setup_event_sourcing();
        b.iter(|| {
            let item_id = AggregateId::new();
            let create_cmd = CreateItem {
                tenant_id,
                item_id: InventoryItemId::new(item_id),
                name: black_box("Test Item".to_string()),
                occurred_at: Utc::now(),
            };
            dispatcher
                .dispatch(
                    tenant_id,
                    item_id,
                    "inventory.item",
                    InventoryCommand::CreateItem(create_cmd),
                    |_, id| InventoryItem::empty(InventoryItemId::new(id)),
                )
                .unwrap();
        });
    });

    // Benchmark: AdjustStock command after creation (with history)
    group.bench_function("adjust_stock_with_history", |b| {
        let (dispatcher, tenant_id, item_id) = setup_event_sourcing();
        let item_id_typed = InventoryItemId::new(item_id);

        // Create item once
        let create_cmd = CreateItem {
            tenant_id,
            item_id: item_id_typed,
            name: "Test Item".to_string(),
            occurred_at: Utc::now(),
        };
        dispatcher
            .dispatch(
                tenant_id,
                item_id,
                "inventory.item",
                InventoryCommand::CreateItem(create_cmd),
                |_, id| InventoryItem::empty(InventoryItemId::new(id)),
            )
            .unwrap();

        b.iter(|| {
            let adjust_cmd = AdjustStock {
                tenant_id,
                item_id: item_id_typed,
                delta: black_box(5),
                occurred_at: Utc::now(),
            };
            dispatcher
                .dispatch(
                    tenant_id,
                    item_id,
                    "inventory.item",
                    InventoryCommand::AdjustStock(adjust_cmd),
                    |_, id| InventoryItem::empty(InventoryItemId::new(id)),
                )
                .unwrap();
        });
    });

    group.finish();
}

fn bench_event_append_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_append_throughput");
    group.throughput(Throughput::Elements(1));

    for batch_size in [1, 10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_append", batch_size),
            batch_size,
            |b, &size| {
                let store = InMemoryEventStore::new();
                let tenant_id = TenantId::new();
                let item_id = AggregateId::new();

                b.iter(|| {
                    let events: Vec<UncommittedEvent> = (0..size)
                        .map(|i| {
                            let event = InventoryEvent::StockAdjusted(
                                forgeerp_inventory::StockAdjusted {
                                    tenant_id,
                                    item_id: InventoryItemId::new(item_id),
                                    delta: i as i64,
                                    occurred_at: Utc::now(),
                                },
                            );
                            UncommittedEvent::from_typed(
                                tenant_id,
                                item_id,
                                "inventory.item",
                                uuid::Uuid::now_v7(),
                                &event,
                            )
                            .unwrap()
                        })
                        .collect();

                    black_box(
                        store
                            .append(events, forgeerp_core::ExpectedVersion::Any)
                            .unwrap(),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_projection_rebuild_speed(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_rebuild_speed");

    for event_count in [10, 100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("rebuild_from_events", event_count),
            event_count,
            |b, &count| {
                let store = InMemoryEventStore::new();
                let tenant_id = TenantId::new();
                let item_id = AggregateId::new();
                let item_id_typed = InventoryItemId::new(item_id);

                // Pre-generate events
                let mut all_envelopes = Vec::new();
                {
                    // Create item
                    let create_event = InventoryEvent::ItemCreated(ItemCreated {
                        tenant_id,
                        item_id: item_id_typed,
                        name: "Test Item".to_string(),
                        occurred_at: Utc::now(),
                    });
                    let uncommitted = UncommittedEvent::from_typed(
                        tenant_id,
                        item_id,
                        "inventory.item",
                        uuid::Uuid::now_v7(),
                        &create_event,
                    )
                    .unwrap();
                    let stored = store
                        .append(vec![uncommitted], forgeerp_core::ExpectedVersion::Any)
                        .unwrap();
                    all_envelopes.push(stored[0].to_envelope());

                    // Add stock adjustments
                    for i in 0..(count - 1) {
                        let adjust_event = InventoryEvent::StockAdjusted(StockAdjusted {
                            tenant_id,
                            item_id: item_id_typed,
                            delta: (i % 10) as i64,
                            occurred_at: Utc::now(),
                        });
                        let uncommitted = UncommittedEvent::from_typed(
                            tenant_id,
                            item_id,
                            "inventory.item",
                            uuid::Uuid::now_v7(),
                            &adjust_event,
                        )
                        .unwrap();
                        let stored = store
                            .append(
                                vec![uncommitted],
                                forgeerp_core::ExpectedVersion::Exact((i + 1) as u64),
                            )
                            .unwrap();
                        all_envelopes.push(stored[0].to_envelope());
                    }
                }

                let read_model_store: Arc<
                    InMemoryTenantStore<InventoryItemId, forgeerp_infra::projections::inventory_stock::InventoryReadModel>,
                > = Arc::new(InMemoryTenantStore::new());
                let projection = InventoryStockProjection::new(read_model_store);

                b.iter(|| {
                    projection.rebuild_from_scratch(black_box(all_envelopes.clone())).unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_event_sourcing_vs_naive_crud(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_sourcing_vs_naive_crud");
    group.sample_size(1000);

    // Benchmark: Event sourcing (create + adjust)
    group.bench_function("event_sourcing_create_and_adjust", |b| {
        let (dispatcher, tenant_id, _) = setup_event_sourcing();

        b.iter(|| {
            let item_id = AggregateId::new();
            let item_id_typed = InventoryItemId::new(item_id);

            // Create
            let create_cmd = CreateItem {
                tenant_id,
                item_id: item_id_typed,
                name: "Test Item".to_string(),
                occurred_at: Utc::now(),
            };
            dispatcher
                .dispatch(
                    tenant_id,
                    item_id,
                    "inventory.item",
                    InventoryCommand::CreateItem(create_cmd),
                    |_, id| InventoryItem::empty(InventoryItemId::new(id)),
                )
                .unwrap();

            // Adjust
            let adjust_cmd = AdjustStock {
                tenant_id,
                item_id: item_id_typed,
                delta: 10,
                occurred_at: Utc::now(),
            };
            dispatcher
                .dispatch(
                    tenant_id,
                    item_id,
                    "inventory.item",
                    InventoryCommand::AdjustStock(adjust_cmd),
                    |_, id| InventoryItem::empty(InventoryItemId::new(id)),
                )
                .unwrap();
        });
    });

    // Benchmark: Naive CRUD (create + adjust)
    group.bench_function("naive_crud_create_and_adjust", |b| {
        let store = NaiveCrudStore::new();
        let tenant_id = TenantId::new();
        let item_id = AggregateId::new();

        b.iter(|| {
            store.create(tenant_id, item_id, "Test Item".to_string());
            store.adjust_stock(tenant_id, item_id, 10).unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_command_execution_latency,
    bench_event_append_throughput,
    bench_projection_rebuild_speed,
    bench_event_sourcing_vs_naive_crud
);
criterion_main!(benches);

