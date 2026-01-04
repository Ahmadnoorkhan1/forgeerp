-- Event Store Schema: Snapshots Table
--
-- Snapshots store the current state of aggregates at specific sequence numbers.
-- This enables fast aggregate rehydration by loading the latest snapshot and
-- then replaying only events after the snapshot version.
--
-- Snapshot Policy:
-- - Snapshots are optional optimizations (not required for correctness)
-- - Snapshots should be taken periodically (e.g., every N events or every M minutes)
-- - Multiple snapshots per aggregate are allowed (snapshot versioning)
-- - When loading, use the latest snapshot with sequence_number <= target version

-- Snapshots table
CREATE TABLE IF NOT EXISTS snapshots (
    -- Stream identification (tenant + aggregate)
    tenant_id UUID NOT NULL,
    aggregate_id UUID NOT NULL,
    aggregate_type TEXT NOT NULL,
    
    -- Snapshot version (matches sequence_number of last event included)
    version BIGINT NOT NULL,
    
    -- Serialized aggregate state (JSON)
    state JSONB NOT NULL,
    
    -- Timestamp when snapshot was created
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Primary key: one snapshot per (tenant, aggregate, version)
    -- Note: Multiple snapshots per aggregate are allowed (e.g., snapshot at v100, v200, etc.)
    CONSTRAINT snapshots_pkey PRIMARY KEY (tenant_id, aggregate_id, version),
    
    -- Constraints
    CONSTRAINT snapshots_version_positive CHECK (version > 0),
    CONSTRAINT snapshots_aggregate_type_not_empty CHECK (aggregate_type != '')
);

-- Indexes for snapshot queries

-- Primary query: Find latest snapshot for a stream (for rehydration)
-- This index allows efficient queries like:
--   SELECT * FROM snapshots 
--   WHERE tenant_id = ? AND aggregate_id = ? 
--   ORDER BY version DESC LIMIT 1
CREATE INDEX IF NOT EXISTS idx_snapshots_stream_latest 
    ON snapshots (tenant_id, aggregate_id, version DESC);

-- Cleanup queries: Find old snapshots to purge (keep only latest N per aggregate)
CREATE INDEX IF NOT EXISTS idx_snapshots_cleanup 
    ON snapshots (tenant_id, aggregate_id, created_at DESC);

-- Tenant-wide snapshot queries (for maintenance, analytics)
CREATE INDEX IF NOT EXISTS idx_snapshots_tenant 
    ON snapshots (tenant_id, created_at);

-- Aggregate type queries
CREATE INDEX IF NOT EXISTS idx_snapshots_aggregate_type 
    ON snapshots (tenant_id, aggregate_type, version DESC);

-- State queries: GIN index for JSONB state searches (optional, for debugging)
-- Only create if you need to query snapshot state contents.
-- CREATE INDEX IF NOT EXISTS idx_snapshots_state 
--     ON snapshots USING GIN (state);

-- Comments for documentation
COMMENT ON TABLE snapshots IS 'Aggregate state snapshots for fast rehydration. Optional optimization - system works without snapshots by replaying all events.';
COMMENT ON COLUMN snapshots.tenant_id IS 'Tenant identifier (matches events.tenant_id)';
COMMENT ON COLUMN snapshots.aggregate_id IS 'Aggregate identifier (matches events.aggregate_id)';
COMMENT ON COLUMN snapshots.version IS 'Sequence number of the last event included in this snapshot (matches events.sequence_number)';
COMMENT ON COLUMN snapshots.state IS 'Serialized aggregate state as JSON (exact format depends on aggregate type)';
COMMENT ON COLUMN snapshots.aggregate_type IS 'Type of aggregate (e.g., "inventory.item")';
COMMENT ON COLUMN snapshots.created_at IS 'Timestamp when snapshot was created';

-- Optional: Function to find latest snapshot for a stream
CREATE OR REPLACE FUNCTION get_latest_snapshot(
    p_tenant_id UUID,
    p_aggregate_id UUID
) RETURNS TABLE (
    tenant_id UUID,
    aggregate_id UUID,
    aggregate_type TEXT,
    version BIGINT,
    state JSONB,
    created_at TIMESTAMPTZ
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        s.tenant_id,
        s.aggregate_id,
        s.aggregate_type,
        s.version,
        s.state,
        s.created_at
    FROM snapshots s
    WHERE s.tenant_id = p_tenant_id
      AND s.aggregate_id = p_aggregate_id
    ORDER BY s.version DESC
    LIMIT 1;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION get_latest_snapshot IS 'Find the latest snapshot for a given tenant + aggregate stream. Returns NULL if no snapshot exists.';

