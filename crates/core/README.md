# `forgeerp-core`

**Responsibility:** Domain entities, value objects, and invariants.

## Boundaries
- Owns the core business domain vocabulary.
- **Must not** depend on `infra` or `api`.
- Should remain stable and reusable across interfaces (HTTP, jobs, etc.).


