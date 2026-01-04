# Database Migration Guide

ForgeERP uses [SQLx](https://github.com/launchbadge/sqlx) for database migrations. Migrations are stored in `docker/migrations/` and are applied in lexicographical order by filename.

## Migration File Layout

Migrations are stored in `docker/migrations/` with the following naming convention:

```
docker/migrations/
├── 001_create_events_table.sql
├── 002_create_snapshots_table.sql
├── 003_create_rls_policies.sql
├── 004_create_read_models.sql
└── README.md
```

**Naming Convention**: Use zero-padded numbers (001, 002, etc.) followed by a descriptive name. SQLx applies migrations in lexicographical order.

## Idempotency

All migrations use `IF NOT EXISTS` and `CREATE OR REPLACE` clauses to ensure they are idempotent:

- Tables: `CREATE TABLE IF NOT EXISTS`
- Indexes: `CREATE INDEX IF NOT EXISTS`
- Functions: `CREATE OR REPLACE FUNCTION`
- Triggers: Check existence before creation

This means:
- ✅ Migrations can be run multiple times safely
- ✅ Partially applied migrations won't cause errors on re-run
- ✅ CI/CD pipelines can run migrations without checking state

## Running Migrations

### Local Development (Docker Compose)

The easiest way to run migrations is via Docker Compose:

```bash
# Run all pending migrations
docker compose run --rm migrate

# Check migration status
docker compose run --rm migrate sqlx migrate --source /app/migrations info

# Revert last migration (if supported)
docker compose run --rm migrate sqlx migrate --source /app/migrations revert
```

The `migrate` service:
- Waits for Postgres to be healthy
- Uses connection string from `.env` file
- Applies migrations automatically

### Local Development (Direct)

If you have `sqlx-cli` installed locally:

```bash
# Install sqlx-cli (if not already installed)
cargo install sqlx-cli --no-default-features --features postgres

# Set DATABASE_URL
export DATABASE_URL="postgres://forgeerp:forgeerp@localhost:5432/forgeerp"

# Or use the helper script
./scripts/migrate.sh run

# Check status
./scripts/migrate.sh info
```

### CI/CD Pipelines

For CI environments, migrations should run before application startup:

```yaml
# Example GitHub Actions
- name: Run database migrations
  env:
    DATABASE_URL: ${{ secrets.DATABASE_URL }}
  run: |
    cargo install sqlx-cli --no-default-features --features postgres
    sqlx migrate --source docker/migrations run
```

**Important**: Always run migrations before starting the application in CI/CD.

## Creating New Migrations

### Using SQLx CLI

```bash
# Create a new migration file
sqlx migrate --source docker/migrations add <migration_name>

# Example
sqlx migrate --source docker/migrations add add_user_preferences_table
```

This creates a new file: `docker/migrations/005_add_user_preferences_table.sql`

### Manual Creation

1. Create a new file with the next sequential number: `005_description.sql`
2. Write your SQL migration (ensure it's idempotent)
3. Test locally: `./scripts/migrate.sh run`

### Migration Template

```sql
-- Migration: <description>
-- Created: YYYY-MM-DD
-- 
-- Description of what this migration does

-- Example: Create a new table
CREATE TABLE IF NOT EXISTS example_table (
    id UUID NOT NULL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT example_tenant_id_not_null CHECK (tenant_id IS NOT NULL)
);

-- Example: Create indexes
CREATE INDEX IF NOT EXISTS idx_example_tenant_id 
    ON example_table (tenant_id);

-- Example: Create function
CREATE OR REPLACE FUNCTION example_function()
RETURNS void AS $$
BEGIN
    -- Function body
END;
$$ LANGUAGE plpgsql;
```

## Migration Status

SQLx tracks applied migrations in the `_sqlx_migrations` table:

```sql
-- View applied migrations
SELECT * FROM _sqlx_migrations ORDER BY installed_on;

-- Check migration status via CLI
sqlx migrate --source docker/migrations info
```

The `_sqlx_migrations` table contains:
- `version`: Migration number (BigInt)
- `description`: Migration description
- `installed_on`: Timestamp when migration was applied
- `success`: Whether migration succeeded
- `checksum`: File checksum for validation

## Production Safety

### Pre-Deployment Checklist

Before running migrations in production:

1. ✅ **Backup Database**: Always backup production database before migrations
   ```bash
   pg_dump -h $DB_HOST -U $DB_USER -d $DB_NAME > backup_$(date +%Y%m%d_%H%M%S).sql
   ```

2. ✅ **Test in Staging**: Run migrations in staging environment first
   ```bash
   # Staging
   DATABASE_URL=$STAGING_DB ./scripts/migrate.sh run
   ```

3. ✅ **Review Migration SQL**: Carefully review migration files before production
   - Check for breaking changes
   - Verify idempotency
   - Ensure rollback plan exists (if needed)

4. ✅ **Maintenance Window**: Schedule migrations during low-traffic periods
   - Use `LOCK` statements sparingly
   - Avoid migrations during peak hours
   - Communicate maintenance window to users

5. ✅ **Monitor Migration**: Watch for errors during migration
   ```bash
   # Run with verbose output
   sqlx migrate --source docker/migrations run --verbose
   ```

### Safe Migration Practices

**DO**:
- ✅ Use `IF NOT EXISTS` for all CREATE statements
- ✅ Use `CREATE OR REPLACE` for functions/views
- ✅ Test migrations on production-like data volumes
- ✅ Have rollback scripts ready (for destructive changes)
- ✅ Run migrations during maintenance windows
- ✅ Monitor application logs after migration

**DON'T**:
- ❌ Modify existing migration files (create new ones instead)
- ❌ Delete migration files (they're historical record)
- ❌ Run migrations without backups
- ❌ Use `DROP TABLE` without backup and rollback plan
- ❌ Add non-nullable columns without defaults to existing tables
- ❌ Run migrations during high-traffic periods (for large changes)

### Rollback Strategy

SQLx supports reverting migrations, but **not all migrations can be automatically reverted**. For safety:

1. **Test Rollback in Staging**: Always test rollback in staging first
2. **Manual Rollback Scripts**: Keep rollback SQL scripts for critical migrations
3. **Version Control**: Tag database state after successful migrations

Example rollback script (`docker/migrations/rollbacks/004_rollback_read_models.sql`):

```sql
-- Rollback for 004_create_read_models.sql
-- ⚠️ WARNING: This will delete all read model data!

-- Drop functions
DROP FUNCTION IF EXISTS clear_tenant_read_models(UUID);
DROP FUNCTION IF EXISTS clear_tenant_offsets(UUID, TEXT);

-- Drop tables (only if empty or backup exists)
-- DROP TABLE IF EXISTS projection_offsets;
-- DROP TABLE IF EXISTS inventory_stock;
```

### Zero-Downtime Migrations

For zero-downtime deployments:

1. **Additive Changes First**: Add new columns/tables without removing old ones
2. **Backfill Data**: Populate new columns before making them required
3. **Deploy Application**: Deploy application that uses new schema
4. **Cleanup Later**: Remove old columns/tables in a later migration

Example zero-downtime migration pattern:

```sql
-- Step 1: Add new column (nullable)
ALTER TABLE events ADD COLUMN IF NOT EXISTS new_field TEXT;

-- Step 2: Backfill data (application does this)
-- UPDATE events SET new_field = ... WHERE new_field IS NULL;

-- Step 3: Make column non-nullable (in separate migration after backfill)
-- ALTER TABLE events ALTER COLUMN new_field SET NOT NULL;
```

## Troubleshooting

### Migration Fails

If a migration fails:

1. **Check Error Message**: Review SQLx error output
2. **Verify Database Connection**: Ensure DATABASE_URL is correct
3. **Check Migration File**: Validate SQL syntax
4. **Inspect `_sqlx_migrations`**: See which migrations succeeded
5. **Manual Fix**: If needed, fix database state manually, then mark migration as applied

```sql
-- Mark migration as applied (use with caution!)
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum)
VALUES (4, 'create_read_models', NOW(), true, '<checksum>');
```

### Migration Already Applied

If you see "migration already applied" errors:

- This is normal for idempotent migrations
- SQLx tracks migrations in `_sqlx_migrations` table
- Re-running is safe (migrations use `IF NOT EXISTS`)

### Connection Errors

```bash
# Verify database connection
psql $DATABASE_URL -c "SELECT 1"

# Check if Postgres is running (Docker)
docker compose ps postgres

# Verify environment variables
echo $DATABASE_URL
```

## Migration Ordering

Migrations are applied in lexicographical order:

- ✅ `001_create_events_table.sql` (applied first)
- ✅ `002_create_snapshots_table.sql`
- ✅ `003_create_rls_policies.sql`
- ✅ `004_create_read_models.sql` (applied last)

**Important**: Never rename existing migration files. If you need to reorder, create new migrations with the desired order.

## Best Practices

1. **One Concern Per Migration**: Each migration should do one logical change
2. **Descriptive Names**: Use clear, descriptive filenames
3. **Idempotency**: Always use `IF NOT EXISTS` and `CREATE OR REPLACE`
4. **Test Locally**: Test migrations on local database before committing
5. **Document Breaking Changes**: Add comments for schema changes that affect application code
6. **Version Control**: Commit migration files immediately after creating them

## Additional Resources

- [SQLx Migrations Documentation](https://github.com/launchbadge/sqlx/blob/main/sqlx-cli/README.md)
- [PostgreSQL Migration Best Practices](https://www.postgresql.org/docs/current/ddl-alter.html)
- [Database Migration Patterns](https://martinfowler.com/articles/evodb.html)

