# `forgeerp-ai`

**Responsibility:** Optional AI/ML subsystem boundary.

This crate hosts AI logic that **consumes projections or event streams** and emits **AI insights**.
It does **not** emit domain events and does **not** mutate domain state.

## Boundaries
- May depend on `forgeerp-core` for `TenantId`.
- Must **not** depend on ERP module aggregates (Inventory/Sales/etc).
- No HTTP, no database code.

## What’s implemented (today)
- `AiJob` trait (tenant-scoped job, deterministic input snapshot)
- `AiResult` (score, confidence, optional explanation, metadata)
- `AiScheduler` + `LocalAiScheduler` (tenant-safe execution model)

## Tenant safety
Schedulers can be pinned to a tenant via `TenantScope::Tenant(tenant_id)`.
This is useful for single-tenant workers and safe initialization.

## Module map

```
ai/src/
  lib.rs
  job.rs
  result.rs
  scheduler.rs
```


