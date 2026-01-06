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
- `explain_authorization(&Principal, &Permission, role_mapping)` provides detailed explanations of authorization decisions

### Command-boundary authorization (centralized)

To ensure commands cannot execute without the right permissions, this crate provides:

- `CommandAuthorization` trait
  - `required_permissions() -> &[Permission]`

The API layer should enforce required permissions **before** dispatching commands.

#### Wildcard permissions

`Permission("*")` is treated as **allow-all** for the current tenant context. This is useful for
admin/service roles without enumerating every permission in tokens.

### Authorization Explanation & Auditing

The `explain_authorization()` function provides transparent, debuggable explanations of authorization decisions. It returns:

- **Whether access was granted** (`granted: bool`)
- **Human-readable reason** for the decision
- **Principal state**: roles, permissions, tenant context
- **Denial reason** (if denied): why it was denied and suggestions for fixing

This enables answering the question: **"Why was this request denied?"**

The `RbacRegistry` provides a complete view of the RBAC system:
- All available roles and their granted permissions
- All available permissions with descriptions and categories
- Role-to-permission mappings for auditing

### User Aggregate (event-sourced identity management)

This crate includes a `User` aggregate for managing user identities within tenants:

#### Commands
- `CreateUser` - create a new user with email, display name, and initial roles
- `AssignRole` - assign a role to a user (with privilege escalation check)
- `RevokeRole` - revoke a role from a user
- `SuspendUser` - suspend a user (prevents authentication and role changes)
- `ActivateUser` - reactivate a suspended user

#### Events
- `UserCreated`, `RoleAssigned`, `RoleRevoked`, `UserSuspended`, `UserActivated`

#### Invariants
- **Tenant isolation**: Users belong to exactly one tenant
- **No privilege escalation**: Users cannot assign roles they don't have (unless admin)
- **Suspended users**: Cannot have roles assigned while suspended

### Admin Permission Constants

The `permissions::admin` module provides standardized permission constants for identity management:

- `admin.users.create`, `admin.users.list`, `admin.users.read`
- `admin.users.assign_role`, `admin.users.revoke_role`
- `admin.users.suspend`, `admin.users.activate`

## Module map

```
auth/src/
  lib.rs
  principal.rs    # PrincipalId, TenantMembership
  roles.rs        # Role
  permissions.rs  # Permission + admin permission constants
  claims.rs       # JwtClaims + validate_claims
  authorize.rs    # authorize(...)
  user.rs         # User aggregate (event-sourced)
```


