# `forgeerp-api`

**Responsibility:** HTTP server, routing, and request/response mapping.

## Boundaries
- Owns transport concerns (HTTP) and maps to/from domain types.
- May depend on `core`, `events`, `auth`, `infra`, and `observability`.
- The only binary crate in the workspace.


