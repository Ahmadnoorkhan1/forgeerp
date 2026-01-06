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

### Inventory (first end-to-end ERP feature)
- `POST /inventory/items` → create an inventory item (requires auth)
- `POST /inventory/items/{id}/adjust` → adjust stock (requires auth)
- `GET /inventory/items/{id}` → fetch current stock read model (requires auth)

### AI insights (read-only)
- `GET /inventory/anomalies` → list detected inventory anomalies for the current tenant (requires auth)
- `GET /inventory/{id}/insights` → fetch AI insights for a specific inventory item (requires auth)

### Real-time (SSE)
- `GET /stream` → Server-Sent Events stream for real-time updates (requires auth)
  - Tenant-scoped: a client only receives events for its authenticated tenant
  - Lossy / no backpressure: slow clients may miss events; core workflows are never blocked

### Products
- `POST /products` → create product
- `POST /products/{id}/activate`
- `POST /products/{id}/archive`
- `GET /products/{id}`
- `GET /products`

### Customers / Suppliers
- `POST /customers` / `POST /suppliers` → register
- `PATCH /customers/{id}` / `PATCH /suppliers/{id}` → update details
- `POST /customers/{id}/suspend` / `POST /suppliers/{id}/suspend`
- `GET /customers` / `GET /suppliers`
- `GET /customers/{id}` / `GET /suppliers/{id}`

### Sales Orders
- `POST /sales/orders` → create order
- `POST /sales/orders/{id}/lines` → add line
- `POST /sales/orders/{id}/confirm`
- `POST /sales/orders/{id}/mark-invoiced`
- `GET /sales/orders` / `GET /sales/orders/{id}`

### Invoices + AR aging
- `POST /invoices` → issue invoice
- `POST /invoices/{id}/payments`
- `POST /invoices/{id}/void`
- `GET /invoices` / `GET /invoices/{id}`
- `GET /ar/aging`

### Purchases
- `POST /purchases/orders` → create purchase order (with lines)
- `POST /purchases/orders/{id}/lines`
- `POST /purchases/orders/{id}/approve`
- `POST /purchases/orders/{id}/receive`
- `GET /purchases/orders` / `GET /purchases/orders/{id}`

### Ledger views
- `POST /ledger/journal` → post journal entry
- `GET /ledger/balances` / `GET /ledger/balances/{code}`

### Admin - Identity Management
- `POST /admin/users` → create a new user in the tenant
- `GET /admin/users` → list all users in the tenant
- `GET /admin/users/{id}` → get a specific user
- `POST /admin/users/{id}/roles` → assign a role to a user
- `DELETE /admin/users/{id}/roles/{role}` → revoke a role from a user
- `POST /admin/users/{id}/suspend` → suspend a user
- `POST /admin/users/{id}/activate` → activate a suspended user
- `GET /admin/users/{id}/permissions` → inspect effective permissions for a user

**Note:** Admin endpoints require specific permissions (`admin.users.*`) and enforce privilege escalation prevention - users cannot assign roles they don't have (unless they have the `admin` role).

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

## Inventory request flow

For inventory commands, the API follows this flow (no business rules in HTTP handlers):

**HTTP → Auth → TenantContext → CommandDispatcher → EventStore → Publish → Projection**

In the current implementation this uses in-memory components for local development:
- `forgeerp_infra::event_store::InMemoryEventStore`
- `forgeerp_events::InMemoryEventBus`
- `forgeerp_infra::projections::inventory_stock::InventoryStockProjection`

## Structured errors

Inventory endpoints return JSON errors in the form:

```json
{ "error": "<code>", "message": "<human-readable>" }
```

Common status codes:
- `401` unauthenticated / malformed token
- `403` forbidden / tenant isolation
- `409` optimistic concurrency conflict
- `422` invariant violations (e.g. stock would go negative)

## AI insights notes

- These endpoints are **read-only** and never execute commands.
- Responses are explicitly labeled as **`kind: "insights"`**.
- In the current dev wiring, insights are generated in-process from projections and stored in-memory.

## Real-time stream notes (SSE)

The SSE endpoint streams messages **derived from projections and AI insight availability** (not commands).

Event types currently emitted:
- `{aggregate_type}.projection_updated`
  - Emitted after a successful projection apply (e.g. `inventory.item.projection_updated`)
- `ai.insight_available`
  - Emitted when new AI insights are stored/available

Each event is sent with:
- SSE `event`: the topic name above
- SSE `data`: a JSON payload describing the update

### Required config

- `JWT_SECRET`: HS256 secret used by the validator (dev default is used if unset; don’t rely on it in real deployments).

## Module map

```
api/src/
  main.rs        # server bootstrap (bind + serve)
  lib.rs
  app/           # router construction + handler modules (see below)
    mod.rs       # build_app()
    services.rs  # event store/bus/dispatcher/projection wiring
    errors.rs    # JSON error responses + DispatchError mapping
    dto.rs       # request DTOs + response JSON mapping helpers
    routes/      # one file per REST area
      mod.rs
      common.rs    # shared auth helpers
      system.rs
      inventory.rs
      products.rs
      customers.rs
      suppliers.rs
      sales.rs
      invoices.rs
      purchases.rs
      ledger.rs
      ar.rs
      admin.rs     # identity management
  authz.rs       # command-boundary authorization guard
  context.rs     # TenantContext / PrincipalContext
  middleware.rs  # auth middleware (Bearer JWT)
```


