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

- **`crates/core`**: domain entities, value objects, invariants  
  - Read more: `crates/core/README.md`
- **`crates/events`**: domain & integration events  
  - Read more: `crates/events/README.md`
- **`crates/auth`**: users, roles, permissions, JWT logic (future)  
  - Read more: `crates/auth/README.md`
- **`crates/infra`**: database/redis/config/external services (future)  
  - Read more: `crates/infra/README.md`
- **`crates/observability`**: shared tracing/logging setup  
  - Read more: `crates/observability/README.md`
- **`crates/api`**: Axum HTTP server + routing + request/response mapping  
  - Read more: `crates/api/README.md`

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
