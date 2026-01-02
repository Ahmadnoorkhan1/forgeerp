# `forgeerp > crates > events`

**Responsibility:** Generic **event sourcing + CQRS primitives** (mechanics only).

## Boundaries
- Defines **event sourcing mechanics**, not business rules.
- May depend on `core` (for strongly-typed identifiers like `TenantId` / `AggregateId`).
- Must not depend on `api` or `infra` (no storage/HTTP assumptions).

## What’s implemented (today)

- **`Event` trait** (`event_type`, `version`, `occurred_at`)
- **`EventEnvelope<E>`** (multi-tenant, stream metadata + payload)
  - `event_id`
  - **`tenant_id`** (multi-tenancy enforced at the event level)
  - `aggregate_id`
  - `aggregate_type`
  - `sequence_number` (monotonic per aggregate stream)
  - `payload`
- **CQRS primitives**
  - `Command` (targets an aggregate via `target_aggregate_id`)
  - `CommandHandler` (handles commands and emits events; no storage assumptions)
  - `Projection` (consumes envelopes to build read models)
- **Aggregate execution helper**
  - `handler::execute(&mut aggregate, &command)` runs **handle → apply** deterministically
  - No async, no IO, no side effects

## Event model guarantees

- **Immutable**: treat events as facts; do not mutate after creation.
- **Versioned**: events expose a schema version (`Event::version()`).
- **Append-only**: envelopes carry a monotonically increasing `sequence_number`.

## Module map

```
events/src/
  lib.rs
  event.rs       # Event trait
  envelope.rs    # EventEnvelope<E>
  command.rs     # Command trait
  handler.rs     # CommandHandler trait
  projection.rs  # Projection trait
```

## Minimal usage (example)

An ERP module crate would define concrete event/command types and use the kernel:

```rust
use chrono::{DateTime, Utc};
use forgeerp_core::AggregateId;
use forgeerp_events::{Command, Event};

#[derive(Clone, Debug)]
struct MyCommand {
    id: AggregateId,
}

impl Command for MyCommand {
    fn target_aggregate_id(&self) -> AggregateId {
        self.id
    }
}

#[derive(Clone, Debug)]
struct MyEvent {
    occurred_at: DateTime<Utc>,
}

impl Event for MyEvent {
    fn event_type(&self) -> &'static str { "my.event" }
    fn version(&self) -> u32 { 1 }
    fn occurred_at(&self) -> DateTime<Utc> { self.occurred_at }
}
```


