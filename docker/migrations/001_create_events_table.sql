-- Event Store Schema: Events Table
-- 
-- This table stores all domain events in an append-only fashion.
-- Events are organized into streams, where each stream is identified by
-- (tenant_id, aggregate_id). Sequence numbers are monotonically increasing
-- within each stream.

-- Enable UUID extension (Postgres 13+)
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Events table (append-only)
CREATE TABLE IF NOT EXISTS events (
    -- Event identity
    event_id UUID NOT NULL PRIMARY KEY,
    
    -- Stream identification (tenant + aggregate)
    tenant_id UUID NOT NULL,
    aggregate_id UUID NOT NULL,
    aggregate_type TEXT NOT NULL,
    
    -- Stream position (monotonically increasing per stream)
    sequence_number BIGINT NOT NULL,
    
    -- Event metadata
    event_type TEXT NOT NULL,
    event_version INTEGER NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    
    -- Event payload (domain event serialized as JSON)
    payload JSONB NOT NULL,
    
    -- Infrastructure metadata (for debugging, monitoring)
    metadata JSONB DEFAULT '{}'::jsonb,
    
    -- Timestamp when event was persisted (infrastructure concern)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Constraints
    CONSTRAINT events_sequence_positive CHECK (sequence_number > 0),
    CONSTRAINT events_stream_unique UNIQUE (tenant_id, aggregate_id, sequence_number),
    CONSTRAINT events_aggregate_type_not_empty CHECK (aggregate_type != ''),
    CONSTRAINT events_event_type_not_empty CHECK (event_type != '')
);

-- Indexes for query performance

-- Primary query: Load stream for a specific tenant + aggregate
-- This is the most common query (used by CommandDispatcher)
CREATE INDEX IF NOT EXISTS idx_events_stream 
    ON events (tenant_id, aggregate_id, sequence_number);

-- Tenant-wide queries: Load all events for a tenant (for projections, replay)
CREATE INDEX IF NOT EXISTS idx_events_tenant 
    ON events (tenant_id, created_at);

-- Aggregate type queries: Find all aggregates of a specific type
CREATE INDEX IF NOT EXISTS idx_events_aggregate_type 
    ON events (tenant_id, aggregate_type, sequence_number);

-- Time-based queries: Find events by time range (for analytics, debugging)
CREATE INDEX IF NOT EXISTS idx_events_occurred_at 
    ON events (occurred_at DESC);

-- Event type queries: Find events of a specific type (for event handlers)
CREATE INDEX IF NOT EXISTS idx_events_event_type 
    ON events (tenant_id, event_type, created_at);

-- Metadata queries: GIN index for JSONB metadata searches
CREATE INDEX IF NOT EXISTS idx_events_metadata 
    ON events USING GIN (metadata);

-- Payload queries: GIN index for JSONB payload searches (optional, for debugging)
-- Note: This index can be large. Only create if you need to query payload contents.
-- CREATE INDEX IF NOT EXISTS idx_events_payload 
--     ON events USING GIN (payload);

-- Prevent updates and deletes (append-only enforcement)
-- Note: Postgres doesn't support CHECK constraints for UPDATE/DELETE.
-- Application code must enforce this, or use RLS policies:
CREATE OR REPLACE FUNCTION prevent_event_modification()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'Events are append-only. Updates are not allowed.'
            USING ERRCODE = 'P0001';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Events are append-only. Deletes are not allowed.'
            USING ERRCODE = 'P0001';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER events_append_only
    BEFORE UPDATE OR DELETE ON events
    FOR EACH ROW
    EXECUTE FUNCTION prevent_event_modification();

-- Comments for documentation
COMMENT ON TABLE events IS 'Append-only event store. All domain events are persisted here with tenant isolation and stream ordering.';
COMMENT ON COLUMN events.event_id IS 'Unique identifier for this event instance (UUIDv7 recommended for time-ordering)';
COMMENT ON COLUMN events.tenant_id IS 'Tenant identifier for multi-tenant isolation';
COMMENT ON COLUMN events.aggregate_id IS 'Aggregate root identifier (identifies the stream)';
COMMENT ON COLUMN events.sequence_number IS 'Monotonically increasing position in the stream (starts at 1)';
COMMENT ON COLUMN events.aggregate_type IS 'Type of aggregate (e.g., "inventory.item")';
COMMENT ON COLUMN events.event_type IS 'Type of domain event (e.g., "inventory.item.stock_adjusted")';
COMMENT ON COLUMN events.event_version IS 'Schema version of the event payload (for schema evolution)';
COMMENT ON COLUMN events.occurred_at IS 'Business timestamp when the event logically occurred';
COMMENT ON COLUMN events.payload IS 'Domain event serialized as JSON (schema determined by event_type + event_version)';
COMMENT ON COLUMN events.metadata IS 'Infrastructure metadata (can store source, correlation IDs, etc.)';
COMMENT ON COLUMN events.created_at IS 'Timestamp when event was persisted to the store (infrastructure concern)';

