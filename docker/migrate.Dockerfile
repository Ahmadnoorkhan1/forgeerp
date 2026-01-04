# Dockerfile for running SQLx database migrations
#
# This image installs sqlx-cli and runs migrations against Postgres.
# Migrations are idempotent and ordered by filename.

FROM rust:1-bookworm AS builder

# Install sqlx-cli
RUN cargo install sqlx-cli --no-default-features --features postgres

FROM debian:bookworm-slim

# Install PostgreSQL client (for manual debugging if needed)
RUN apt-get update && apt-get install -y --no-install-recommends \
    postgresql-client \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy sqlx-cli from builder
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

WORKDIR /app

# Copy migration files
COPY docker/migrations /app/migrations

# Set default command to show migration status
# Override with: docker compose run --rm migrate <command>
# Available commands:
#   - sqlx migrate --source /app/migrations run (apply pending migrations)
#   - sqlx migrate --source /app/migrations revert (revert last migration)
#   - sqlx migrate --source /app/migrations info (show migration status)
CMD ["sqlx", "migrate", "--source", "/app/migrations", "info"]
