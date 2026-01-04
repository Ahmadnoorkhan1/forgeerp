# Dockerfile for running database migrations

FROM postgres:16-alpine

# Install any migration tools if needed (e.g., sqlx, refinery)
# For now, migrations are run via psql

WORKDIR /migrations

# Copy migration files
COPY docker/migrations/*.sql /migrations/

# Default command: list available migrations
# To run migrations, override with: psql $DATABASE_URL -f 001_create_events_table.sql ...
CMD ["sh", "-c", "ls -la /migrations/*.sql"]


