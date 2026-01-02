# `forgeerp-infra`

**Responsibility:** Infrastructure adapters (storage, config, external services) that support the domain.

## Boundaries
- Implements adapters/drivers for the outside world.
- Depends on **`core` + `events`**, never the reverse.
- Must not be depended on by `core`.
- `api` may depend on `infra` to wire everything together.

## What’s implemented (today)

### Command execution pipeline (application orchestration)

Infra includes a reusable command execution engine that wires together event sourcing primitives
without pulling HTTP or auth concerns into the domain:

- `command_dispatcher::CommandDispatcher`
  - Flow: **Command → Load events → Rehydrate aggregate → Decide → Persist → Publish**
  - Enforces **tenant isolation** by validating loaded streams (tenant_id + aggregate_id + monotonic sequence)
  - Surfaces deterministic domain failures (validation/invariant) and optimistic concurrency conflicts

### Event store abstraction (append-only)

`infra` provides a storage-agnostic **append-only event store** boundary:

- `EventStore` trait
  - `append(events, expected_version)`
  - `load_stream(tenant_id, aggregate_id)`
- Guarantees / enforcement
  - **Tenant isolation** (tenant-scoped reads and writes)
  - **Optimistic concurrency** via `forgeerp_core::ExpectedVersion`
  - **Append-only streams** with monotonically increasing `sequence_number`

Types:
- `UncommittedEvent`: event ready to append (no sequence number yet)
- `StoredEvent`: persisted event with assigned `sequence_number`
- `EventStoreError`: concurrency / tenant isolation / validation errors

### In-memory implementation (tests/dev)

- `InMemoryEventStore`: an in-memory, tenant-scoped store intended for tests and local development.
  - No IO / no async
  - Enforces the same isolation + concurrency rules as real backends should

### Publish-after-append adapter

Infra also provides an adapter to guarantee the ordering invariant:

- **Publish happens only after append succeeds**

`PublishingEventStore<S, B>` wraps any `EventStore` + an `EventBus` and publishes
committed events after a successful append. This supports at-least-once delivery
(consumers must be idempotent).

### Event bus implementations (optional)

The pure event bus abstraction (`EventBus`) lives in **`forgeerp-events`**.
Infra provides infrastructure-backed implementations:

- `event_bus::redis_pubsub::RedisPubSubEventBus` *(feature: `redis`)*
  - Publishes `EventEnvelope<serde_json::Value>` via Redis Pub/Sub
  - Intended for local/dev; not durable (messages may be missed if offline)

### Read model storage (tenant-isolated, disposable)

Infra provides a tenant-isolated storage abstraction for **disposable read models**:

- `TenantStore<K, V>`: key/value store scoped by `TenantId`
- `InMemoryTenantStore<K, V>`: in-memory implementation for tests/dev

This supports **rebuild-from-scratch** projections by clearing a tenant’s read model and replaying events.

### Inventory stock projection (read model)

Infra includes a first concrete projection for the Inventory module:

- `projections::inventory_stock::InventoryStockProjection`
  - Consumes published `EventEnvelope<serde_json::Value>` messages
  - Deserializes payload into `forgeerp_inventory::InventoryEvent`
  - Maintains a queryable `InventoryReadModel` (current stock per item)
  - Enforces tenant isolation + monotonic sequence per (tenant, aggregate) stream
  - Rebuildable from scratch by replaying envelopes

### AI snapshot adapters (read model → AI input)

Infra can expose tenant-isolated read models as **AI-safe snapshots** (without granting
direct access to the raw event store).

- Implements `forgeerp_ai::ReadModelReader<S>` for selected projections.
- Example (today): Inventory stock projection produces `forgeerp_ai::InventorySnapshot`
  (including a derived `historical_trend`, currently minimal).

### AI job orchestration (schedule + event-trigger)

Infra provides optional orchestration helpers to run AI jobs without affecting core workflows:

- `ai::inventory_anomaly_runner::InventoryAnomalyRunner`
  - **Interval** execution (cron-like cadence)
  - **Event-trigger hook** (`InventoryAnomalyRunnerHandle::trigger()`) intended to be called after a successful projection update
  - **Backpressure** via trigger coalescing (bounded queue)
  - **Retry safety** with bounded exponential backoff (failures are logged; never propagated to the command/projection pipeline)
  - Emits insights to an `AiInsightSink` (AI outputs are not domain events)

### Background workers (projection runners)

Infra provides reusable worker loops to run projection handlers asynchronously:

- `workers::ProjectionWorker` + `workers::WorkerHandle`
  - Subscribes to an `EventBus`
  - Runs idempotent handlers (at-least-once delivery safe)
  - Graceful shutdown support
  - Optional tenant filtering for tenant-safe initialization

## Module map

```
infra/src/
  lib.rs
  command_dispatcher.rs
  event_bus/
    mod.rs
    redis_pubsub.rs   # optional (feature = "redis")
  read_model/
    mod.rs
    tenant_store.rs   # TenantStore + InMemoryTenantStore
  projections/
    mod.rs
    inventory_stock.rs
  event_store/
    mod.rs
    trait.rs
    in_memory.rs
  ai/
    mod.rs
    inventory_anomaly_runner.rs
  workers/
    mod.rs
    projection_worker.rs
```


