# `forgeerp-api`

**Responsibility:** HTTP gateway (Axum) — routing + request/response mapping + request context propagation.

## Boundaries
- Owns transport concerns (HTTP) and maps to/from domain types.
- May depend on `core`, `events`, `auth`, `infra`, and `observability`.
- The only binary crate in the workspace.

## What’s implemented (today)

### Public endpoints
- `GET /health` → **200 OK** (no auth)

### Authenticated endpoints (example)
- `GET /whoami` → returns the authenticated principal + tenant context (requires auth)

## Authentication + tenant context propagation

This crate implements an Axum middleware that:
- Extracts `Authorization: Bearer <JWT>`
- Validates the token via **`forgeerp-auth`** (API does **not** decode JWTs itself)
- Inserts immutable request extensions:
  - `TenantContext { tenant_id }`
  - `PrincipalContext { principal_id, roles }`
- Rejects malformed/unauthenticated requests with **401**

### Required config

- `JWT_SECRET`: HS256 secret used by the validator (dev default is used if unset; don’t rely on it in real deployments).

## Module map

```
api/src/
  main.rs        # server + route wiring
  lib.rs
  context.rs     # TenantContext / PrincipalContext
  middleware.rs  # auth middleware (Bearer JWT)
```


