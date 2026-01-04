# Event Store Database Migrations

This directory contains SQL migrations for the Postgres-backed event store.

## Migration Files

1. **`001_create_events_table.sql`**: Creates the `events` table with append-only constraints
2. **`002_create_snapshots_table.sql`**: Creates the `snapshots` table for aggregate state snapshots
3. **`003_create_rls_policies.sql`**: Optional Row-Level Security policies for tenant isolation

## Schema Overview

### Events Table

The `events` table stores all domain events in an append-only fashion:
- **Primary key**: `event_id` (UUID)
- **Unique constraint**: `(tenant_id, aggregate_id, sequence_number)`
- **Key indexes**: Stream queries, tenant-wide queries, time-based queries
- **Enforcement**: Triggers prevent UPDATE/DELETE operations

### Snapshots Table

The `snapshots` table stores aggregate state snapshots:
- **Primary key**: `(tenant_id, aggregate_id, version)`
- **Purpose**: Optimize aggregate rehydration (load latest snapshot + replay events after)
- **Policy**: Snapshots are optional; system works without them

## Running Migrations

ForgeERP uses SQLx for database migrations. See [`MIGRATIONS.md`](../../MIGRATIONS.md) for complete documentation.

### Quick Start

```bash
# Using Docker Compose (recommended)
docker compose run --rm migrate

# Using local sqlx-cli
./scripts/migrate.sh run

# Check migration status
./scripts/migrate.sh info
```

### Migration Files

1. **`001_create_events_table.sql`**: Creates the `events` table with append-only constraints
2. **`002_create_snapshots_table.sql`**: Creates the `snapshots` table for aggregate state snapshots
3. **`003_create_rls_policies.sql`**: Optional Row-Level Security policies for tenant isolation
4. **`004_create_read_models.sql`**: Creates `inventory_stock` and `projection_offsets` tables

All migrations are **idempotent** and can be run multiple times safely.

## Snapshot Policy

### When to Create Snapshots

Snapshots should be created:
1. **Periodically**: Every N events (e.g., every 100 events)
2. **Time-based**: Every M minutes/hours for active aggregates
3. **On-demand**: Before major schema migrations or aggregate rebuilds
4. **Selective**: Only for aggregates with long event histories (performance optimization)

### Snapshot Strategy

- **Frequency**: Balance between storage cost and rehydration speed
- **Retention**: Keep latest N snapshots per aggregate (e.g., latest 5)
- **Cleanup**: Purge old snapshots periodically (snapshots are disposable)
- **Failure tolerance**: If snapshot creation fails, replay all events (slower but correct)

### Example Snapshot Policy

```sql
-- Example: Create snapshot every 100 events
-- This would be implemented in application code, not SQL

CREATE OR REPLACE FUNCTION should_snapshot(
    p_tenant_id UUID,
    p_aggregate_id UUID,
    p_new_version BIGINT
) RETURNS BOOLEAN AS $$
DECLARE
    latest_snapshot_version BIGINT;
BEGIN
    -- Get latest snapshot version
    SELECT MAX(version) INTO latest_snapshot_version
    FROM snapshots
    WHERE tenant_id = p_tenant_id AND aggregate_id = p_aggregate_id;
    
    -- Snapshot if no snapshot exists or gap is >= 100
    RETURN latest_snapshot_version IS NULL 
        OR (p_new_version - latest_snapshot_version) >= 100;
END;
$$ LANGUAGE plpgsql;
```

## Index Strategy

### Events Table Indexes

1. **`idx_events_stream`**: Primary query path (load stream by tenant + aggregate)
2. **`idx_events_tenant`**: Tenant-wide queries (projection replay, analytics)
3. **`idx_events_aggregate_type`**: Type-based queries (find all items, etc.)
4. **`idx_events_occurred_at`**: Time-based queries (debugging, analytics)
5. **`idx_events_event_type`**: Event type queries (event handlers)
6. **`idx_events_metadata`**: JSONB metadata searches (GIN index)

### Snapshots Table Indexes

1. **`idx_snapshots_stream_latest`**: Find latest snapshot for rehydration
2. **`idx_snapshots_cleanup`**: Purge old snapshots
3. **`idx_snapshots_tenant`**: Tenant-wide snapshot queries
4. **`idx_snapshots_aggregate_type`**: Type-based snapshot queries

## Tenant Isolation

Tenant isolation is enforced at multiple layers:

1. **Application layer**: Command dispatcher validates tenant IDs
2. **Database layer**: RLS policies (optional, additional defense-in-depth)
3. **Index design**: Tenant ID is first column in composite indexes

RLS policies require the application to set `app.current_tenant_id` before queries. This is optional - application-level filtering is sufficient for most use cases.

## UUIDv7 Compatibility

All UUID columns use standard UUID type. UUIDv7 generation happens in application code (Rust's `uuid` crate with v7 feature). The database doesn't need special handling - UUIDv7 is just a UUID with time-ordering properties that improve index locality.

## Performance Considerations

- **Append-only writes**: Very fast (no updates, no deletes)
- **Stream loads**: Indexed by (tenant_id, aggregate_id, sequence_number) - O(log n)
- **Tenant-wide queries**: Use (tenant_id, created_at) index - efficient for time-ordered replay
- **Snapshot queries**: Latest snapshot lookup is O(log n) with DESC index

## Maintenance

### Event Table Maintenance

- **VACUUM**: Periodic vacuuming to reclaim space (Postgres automatic)
- **REINDEX**: Rebuild indexes if they become fragmented
- **Partitioning**: Consider time-based partitioning for very large tables (future)

### Snapshot Table Maintenance

- **Cleanup**: Periodically delete old snapshots (keep only latest N per aggregate)
- **Re-snapshot**: Rebuild snapshots if aggregate schema changes

Example cleanup query:
```sql
-- Delete old snapshots, keeping only the latest 5 per aggregate
WITH ranked_snapshots AS (
    SELECT 
        tenant_id, 
        aggregate_id, 
        version,
        ROW_NUMBER() OVER (
            PARTITION BY tenant_id, aggregate_id 
            ORDER BY version DESC
        ) AS rn
    FROM snapshots
)
DELETE FROM snapshots
WHERE (tenant_id, aggregate_id, version) IN (
    SELECT tenant_id, aggregate_id, version
    FROM ranked_snapshots
    WHERE rn > 5
);
```

