# `forgeerp-products`

**Responsibility:** Products/Catalog domain module implemented as a pure, event-sourced aggregate.

## Boundaries
- Depends on `forgeerp-core` (domain primitives) and `forgeerp-events` (mechanics).
- No database, no HTTP, no infrastructure concerns.
- All rules are deterministic and testable.

## What's implemented (today)

### Aggregate
- `Product` (event-sourced aggregate)

### Commands
- `CreateProduct`
- `ActivateProduct`
- `ArchiveProduct`

### Events
- `ProductCreated`
- `ProductActivated`
- `ProductArchived`

### Invariants / rules
- **SKU cannot be empty** (uniqueness per tenant requires infrastructure support)
- **Name cannot be empty**
- **Archived products cannot be activated**
- **Archived products cannot be sold** (`can_be_sold()` returns `false`)
- **Product identity is tenant-scoped** (tenant_id carried in commands/events and validated by the aggregate)

### Status Lifecycle
- **Draft**: Initial state after creation (cannot be sold)
- **Active**: Product is available for sale (`can_be_sold()` returns `true`)
- **Archived**: Product is no longer available (cannot be sold, cannot be activated)

### Pricing Metadata
- Optional `PricingMetadata` struct with:
  - `base_price`: Price in smallest currency unit (e.g., cents)
  - `currency`: ISO currency code (e.g., "USD", "EUR")
- No accounting integration yet (pricing is metadata only)

## SKU Uniqueness Note

True SKU uniqueness per tenant requires infrastructure support (checking the event store or read model before command dispatch). The aggregate enforces that SKU is non-empty, but cross-aggregate uniqueness validation must be handled by the infrastructure layer (e.g., CommandDispatcher can check a read model before dispatching CreateProduct commands).

## Module map

```
products/src/
  lib.rs
  product.rs   # Product aggregate + commands/events + invariants
```

