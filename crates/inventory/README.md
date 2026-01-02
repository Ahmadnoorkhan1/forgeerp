# `forgeerp-inventory`

**Responsibility:** First ERP module — Inventory domain implemented as a pure, event-sourced aggregate.

## Boundaries
- Depends on `forgeerp-core` (domain primitives) and `forgeerp-events` (mechanics).
- No database, no HTTP, no infrastructure concerns.
- All rules are deterministic and testable.


