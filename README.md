# ForgeERP

An **open-source micro‑ERP** built in **Rust**, designed as a modular platform for building ERP features (inventory, sales, purchasing, accounting, etc.) with clean domain boundaries and a production-friendly architecture.

This repository is intentionally **bootstrapped** first: workspace layout, shared dependencies, observability, and an API skeleton. Business logic and integrations will evolve incrementally.

## What is ForgeERP?

ForgeERP aims to be a modern, developer-friendly ERP backend:

- **Modular by design**: domain code is separated from infrastructure and delivery (HTTP).
- **Fast and reliable**: Rust + Tokio for efficient concurrency.
- **Observable by default**: structured JSON logs with `RUST_LOG` filtering.
- **Open-source**: built in public with clear contribution paths.

## Repo structure (Cargo workspace)

ForgeERP is a Cargo workspace with focused crates under `crates/`:

- **`crates/core`**: **domain foundation** (IDs, errors, aggregate semantics)  
  - Read more: `crates/core/README.md`
- **`crates/events`**: **event sourcing + CQRS mechanics** (immutable, versioned, append-only)  
  - Read more: `crates/events/README.md`
- **`crates/auth`**: users, roles, permissions, JWT logic (future)  
  - Read more: `crates/auth/README.md`
- **`crates/infra`**: infrastructure adapters (event store, event bus, future DB/Redis/etc)  
  - Read more: `crates/infra/README.md`
- **`crates/observability`**: shared tracing/logging setup  
  - Read more: `crates/observability/README.md`
- **`crates/api`**: Axum HTTP server + routing + request/response mapping  
  - Read more: `crates/api/README.md`

## Architecture laws (hard constraints)

These are non-negotiable boundaries enforced across the codebase:

- **`core` has zero infrastructure dependencies**
- **`events` defines mechanics, not business**
- **`infra` depends on `core` + `events`, never the reverse**
- **API only orchestrates**
- **Multi-tenancy is enforced at the event level**
- **Events are immutable, versioned, append-only**

## Event-sourced command flow (current building blocks)

ForgeERP is being built around an event-sourced execution model:

**Command → Load events → Rehydrate aggregate → Decide → Persist → Publish**

In the current repo state, these responsibilities live in:

- **Domain semantics (`crates/core`)**
  - `Aggregate` trait separates:
    - **decision**: `handle(&self, cmd) -> Vec<Event>` (no mutation)
    - **mutation**: `apply(&mut self, event)` (state evolution)
  - Optimistic concurrency via `ExpectedVersion`
- **Mechanics (`crates/events`)**
  - `Event` + `EventEnvelope` (tenant-scoped metadata + payload)
  - `EventBus` + `InMemoryEventBus`
  - Projections: `Projection` + `ProjectionRunner` (cursor/version tracking + rebuild from scratch)
- **Infrastructure (`crates/infra`)**
  - `EventStore` trait + `InMemoryEventStore`
  - `PublishingEventStore` adapter (publish only after successful append)
  - `CommandDispatcher` (orchestrates the full pipeline without HTTP/auth)

## Quickstart (local, without Docker)

Requirements:
- Rust (edition 2024 toolchain)

Run the API:

```bash
cargo run -p forgeerp-api
```

Health check:

```bash
curl -i http://localhost:8080/health
```

## Quickstart (Docker development)

Requirements:
- Docker engine (Docker Desktop or Colima)

Create a local env file:

```bash
cp .env.example .env
```

Build & start services:

```bash
docker build -f docker/api.Dockerfile .
docker compose up --build
```

If your Docker installation doesn’t support `docker compose`, use:

```bash
docker-compose up --build
```

This starts:
- **API** on `localhost:8080`
- **Postgres** on `localhost:5432` (with a named volume)
- **Redis** on `localhost:6379` (with a named volume)

## Configuration

Environment variables (see `.env.example`):

- **API**
  - `API_HOST` (default `0.0.0.0`)
  - `API_PORT` (default `8080`)
- **Logging**
  - `RUST_LOG` (example: `info`, `debug`, `forgeerp_api=debug,tower_http=info`)
- **Postgres**
  - `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`
  - `DATABASE_URL` (Docker uses hostname `postgres`)
- **Redis**
  - `REDIS_URL` (Docker uses hostname `redis`)

## Development building blocks (what exists today)

- **In-memory event store**: `forgeerp_infra::event_store::InMemoryEventStore`
- **In-memory event bus**: `forgeerp_events::InMemoryEventBus`
- **Command dispatcher**: `forgeerp_infra::command_dispatcher::CommandDispatcher`
  - Handles concurrency conflicts + deterministic domain validation errors
  - Enforces tenant isolation by validating loaded streams (tenant_id + aggregate_id + monotonic sequence)
- **Projection runner**: `forgeerp_events::ProjectionRunner`
  - Replay events to build disposable read models, with cursor/version tracking
  - Supports tenant-pinned runners via `ProjectionRunner::new_for_tenant(...)`

## Optional features

- **Redis pub/sub event bus** (infra feature flag):
  - Feature: `forgeerp-infra/redis`
  - Implementation: `forgeerp_infra::event_bus::redis_pubsub::RedisPubSubEventBus`
  - Note: Redis Pub/Sub is **not durable**; it’s intended for local/dev. Durable delivery would use a different mechanism (e.g., Redis Streams / a broker) later.

## Observability

ForgeERP uses `tracing` + `tracing-subscriber`:
- **JSON logs**
- **Timestamps**
- **Env filter** via `RUST_LOG`

The shared initializer is:
- **`forgeerp_observability::init()`** (usable from any binary: API and future workers)

## API

Current endpoints:
- `GET /health` → **200 OK**

The API is built with:
- **Axum**
- **Tokio**
- **Tower**

## Contributing

Contributions are welcome—issues and PRs are encouraged.

Suggested workflow:
- Open an issue describing the change (or pick an existing one)
- Keep PRs focused and small
- Prefer changes that respect crate boundaries (domain vs infra vs API)

## Roadmap (high level)

- Domain modeling in `core` (entities/value objects/invariants)
- Persistence layer in `infra` (Postgres, migrations)
- Auth primitives in `auth` (users/roles/permissions/JWT)
- Domain and integration events in `events`
- More API routes and request/response mapping in `api`

## License

MIT — see `LICENSE`.
