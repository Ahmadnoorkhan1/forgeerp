# `forgeerp > crates > auth`

**Responsibility:** Pure **authentication + authorization boundary** (zero-trust), decoupled from HTTP and storage.

## Boundaries
- Defines auth primitives and enforcement APIs (no business logic).
- May depend on `core` (for `TenantId`).
- Must not depend on `api` or `infra` (no HTTP, no DB/Redis).

## What’s implemented (today)

### Auth primitives
- `PrincipalId`: identity of an authenticated principal (UUIDv7-backed)
- `Role`: RBAC role identifier (opaque string)
- `Permission`: permission identifier (opaque string, e.g. `inventory.read`)
- `TenantMembership`: tenant-scoped roles + permissions

### JWT claims model (transport-agnostic)
- `JwtClaims`
  - `sub` (principal_id)
  - `tenant_id`
  - `roles`
  - `issued_at` / `expires_at`

### Token validation (claims-level)
- `validate_claims(&JwtClaims, now)` performs deterministic time-window validation
- Signature verification / decoding is intentionally **out of scope** for this crate

### RBAC enforcement
- `authorize(&Principal, &Permission)` returns explicit `AuthzError` (no panics)

## Module map

```
auth/src/
  lib.rs
  principal.rs    # PrincipalId, TenantMembership
  roles.rs        # Role
  permissions.rs  # Permission
  claims.rs       # JwtClaims + validate_claims
  authorize.rs    # authorize(...)
```


