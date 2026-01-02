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
- `ReadModelReader<S>`: tenant-isolated snapshot reader API for AI inputs
- Example snapshot schema: `InventorySnapshot` / `InventoryItemSnapshot`
- First AI feature: inventory anomaly detection (`InventoryAnomalyJob`)

## Tenant safety
Schedulers can be pinned to a tenant via `TenantScope::Tenant(tenant_id)`.
This is useful for single-tenant workers and safe initialization.

Read model snapshots are always requested by `tenant_id` via `ReadModelReader::get_snapshot(tenant_id)`.

## Read models as inputs (safe by default)
AI jobs should prefer **read model snapshots** over reading the raw event store.

- The AI crate defines the **contract** (`ReadModelReader<S>` + snapshot schemas).
- Infrastructure crates provide **adapters** that implement the contract by reading
  projections / read model stores.

Example (today):
- Inventory snapshots can be produced from the Inventory stock projection in `forgeerp-infra`
  (without direct event store access).

## Inventory anomaly detection (first AI insight)

Use case: **detect unusual stock movements**.

- **Job**: `InventoryAnomalyJob`
  - Input: `InventorySnapshot` (per-tenant)
  - Model: rolling mean + sample standard deviation over recent **stock deltas**
  - Deterministic given the same snapshot (no randomness, no IO)
- **Insight payload**: `AnomalyDetected`
  - `item_id`
  - `severity` (scaled by z-score)
  - `explanation`

The job emits an `AiResult` with:
- `metadata.kind = "inventory.anomaly_detection"`
- `metadata.anomalies = [AnomalyDetected, ...]`

## Module map

```
ai/src/
  lib.rs
  inventory_anomaly.rs
  job.rs
  result.rs
  scheduler.rs
```


