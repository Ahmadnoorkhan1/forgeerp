# `forgeerp-infra`

**Responsibility:** Database, Redis, configuration, and external services.

## Boundaries
- Implements adapters/drivers for the outside world.
- Must not be depended on by `core`.
- `api` may depend on `infra` to wire everything together.


