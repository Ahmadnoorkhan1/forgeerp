# `forgeerp-observability`

**Responsibility:** Tracing/logging setup shared across binaries.

## Boundaries
- Provides initialization/config helpers for telemetry.
- Should not depend on `api`.
- `api` may depend on this crate to initialize tracing/logging.


