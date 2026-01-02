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

## Module map

```
infra/src/
  lib.rs
  event_bus/
    mod.rs
    redis_pubsub.rs   # optional (feature = "redis")
  event_store/
    mod.rs
    trait.rs
    in_memory.rs
```


