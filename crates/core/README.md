# `forgeerp > crates > core`

**Responsibility:** Pure domain foundation — entities, value objects, invariants, and shared domain primitives.

## Boundaries
- Owns the core business domain vocabulary.
- **Must not** depend on `infra` or `api`.
- Should remain stable and reusable across interfaces (HTTP, jobs, etc.).

## What’s implemented (today)

- **Core traits**
  - `AggregateRoot`: minimal aggregate interface (id + version)
  - `Aggregate`: deterministic event-sourced execution semantics (`handle` + `apply`)
  - `Entity`: identity-based domain object marker
  - `ValueObject`: value-equality marker
- **Optimistic concurrency**
  - `ExpectedVersion`: `Any` or `Exact(u64)` + `check(actual)` → `DomainError::Conflict`
- **Strongly-typed identifiers** (UUIDv7-backed)
  - `TenantId`: multi-tenant boundary identifier
  - `UserId`: actor identifier
  - `AggregateId`: aggregate root identifier
- **Domain error model**
  - `DomainError` (via `thiserror`)
  - `DomainResult<T>` alias

## Module map

```
core/src/
  lib.rs
  aggregate.rs     # AggregateRoot / Aggregate / ExpectedVersion
  entity.rs        # Entity
  value_object.rs  # ValueObject
  id.rs            # TenantId/UserId/AggregateId
  error.rs         # DomainError/DomainResult
```

## Design notes

- **No infrastructure code**: no DB/Redis/HTTP imports, no IO, no runtime assumptions.
- **Deterministic and testable**: domain failures are modeled as `DomainError`.
- **Clear separation of concerns**:
  - `handle(&self, cmd)` is **decision logic** (returns events, no mutation)
  - `apply(&mut self, event)` is **state mutation** (evolves state)


