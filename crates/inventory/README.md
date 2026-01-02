# `forgeerp-inventory`

**Responsibility:** First ERP module — Inventory domain implemented as a pure, event-sourced aggregate.

## Boundaries
- Depends on `forgeerp-core` (domain primitives) and `forgeerp-events` (mechanics).
- No database, no HTTP, no infrastructure concerns.
- All rules are deterministic and testable.

## What’s implemented (today)

### Aggregate
- `InventoryItem` (event-sourced aggregate)

### Commands
- `CreateItem`
- `AdjustStock`

### Events
- `ItemCreated`
- `StockAdjusted`

### Invariants / rules
- **Stock cannot go negative**
- **Item identity is tenant-scoped** (tenant_id carried in commands/events and validated by the aggregate)

## Module map

```
inventory/src/
  lib.rs
  item.rs   # InventoryItem + commands/events + invariants
```


