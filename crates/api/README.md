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

## Authorization at the command boundary

Commands must not be dispatched unless the caller is authorized.

This crate provides an API-side helper:
- `authorize_command(tenant_ctx, principal_ctx, command)`

It checks `forgeerp_auth::CommandAuthorization::required_permissions()` **before** dispatching.
This keeps aggregates and infra **auth-agnostic**.

### Required config

- `JWT_SECRET`: HS256 secret used by the validator (dev default is used if unset; don’t rely on it in real deployments).

## Module map

```
api/src/
  main.rs        # server + route wiring
  lib.rs
  authz.rs       # command-boundary authorization guard
  context.rs     # TenantContext / PrincipalContext
  middleware.rs  # auth middleware (Bearer JWT)
```


