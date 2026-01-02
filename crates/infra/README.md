# `forgeerp-infra`

**Responsibility:** Infrastructure adapters (storage, config, external services) that support the domain.

## Boundaries
- Implements adapters/drivers for the outside world.
- Depends on **`core` + `events`**, never the reverse.
- Must not be depended on by `core`.
- `api` may depend on `infra` to wire everything together.

## What’s implemented (today)

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

## Module map

```
infra/src/
  lib.rs
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
```


