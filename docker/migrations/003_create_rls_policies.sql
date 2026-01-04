-- Row-Level Security (RLS) Policies for Tenant Isolation
--
-- These policies provide database-level enforcement of tenant isolation.
-- This is defense-in-depth: application code also enforces tenant isolation,
-- but RLS provides an additional security layer.
--
-- Note: RLS must be enabled per table and requires a function to determine
-- the current tenant context (typically from application session/JWT).

-- Enable RLS on events table
ALTER TABLE events ENABLE ROW LEVEL SECURITY;

-- Enable RLS on snapshots table
ALTER TABLE snapshots ENABLE ROW LEVEL SECURITY;

-- Create a function to get current tenant context
-- This function should be set by the application connection (using SET LOCAL)
-- Example: SET LOCAL app.current_tenant_id = '...';
CREATE OR REPLACE FUNCTION current_tenant_id() RETURNS UUID AS $$
    SELECT NULLIF(current_setting('app.current_tenant_id', TRUE), '')::UUID;
$$ LANGUAGE sql STABLE;

COMMENT ON FUNCTION current_tenant_id IS 'Returns the current tenant ID from session variable. Application code must set app.current_tenant_id before queries.';

-- RLS Policy: Users can only see events for their tenant
-- This policy assumes the application sets app.current_tenant_id before queries
CREATE POLICY events_tenant_isolation ON events
    FOR ALL
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

COMMENT ON POLICY events_tenant_isolation ON events IS 'Enforces tenant isolation: users can only access events for their tenant context.';

-- RLS Policy: Users can only see snapshots for their tenant
CREATE POLICY snapshots_tenant_isolation ON snapshots
    FOR ALL
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

COMMENT ON POLICY snapshots_tenant_isolation ON snapshots IS 'Enforces tenant isolation: users can only access snapshots for their tenant context.';

-- Note: In production, you may want to:
-- 1. Use connection pooling with per-tenant connections
-- 2. Use application-level tenant filtering (simpler, more common)
-- 3. Use database roles per tenant (complex, but strongest isolation)
--
-- RLS is optional and provides additional defense-in-depth. The application
-- layer MUST enforce tenant isolation regardless of RLS status.

