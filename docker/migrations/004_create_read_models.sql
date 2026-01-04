-- Read Model Schema: Inventory Stock and Projection Offsets
--
-- This migration creates tables for:
-- 1. inventory_stock: Tenant-scoped read model for inventory items
-- 2. projection_offsets: Cursor tracking for projection resume/rebuild support
--
-- Read models are disposable and rebuildable from the event stream.
-- Offsets enable projections to resume after crashes and support deterministic rebuilds.

-- Inventory Stock Read Model
-- Stores current state of inventory items (denormalized, queryable state)
CREATE TABLE IF NOT EXISTS inventory_stock (
    -- Tenant isolation
    tenant_id UUID NOT NULL,
    
    -- Aggregate identifier (item_id)
    item_id UUID NOT NULL,
    
    -- Read model fields
    name TEXT NOT NULL,
    quantity BIGINT NOT NULL,
    
    -- Timestamps for tracking
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Primary key: one read model per tenant + item
    CONSTRAINT inventory_stock_pkey PRIMARY KEY (tenant_id, item_id),
    
    -- Constraints
    CONSTRAINT inventory_stock_name_not_empty CHECK (name != '')
);

-- Indexes for inventory_stock queries

-- Primary query: Get item for tenant + item_id (used by projection.get)
CREATE INDEX IF NOT EXISTS idx_inventory_stock_lookup 
    ON inventory_stock (tenant_id, item_id);

-- List query: Get all items for a tenant (used by projection.list)
CREATE INDEX IF NOT EXISTS idx_inventory_stock_tenant 
    ON inventory_stock (tenant_id, updated_at DESC);

-- Projection Offsets (Cursors/Checkpoints)
-- Tracks the last processed sequence_number per (tenant, aggregate) stream
-- Enables idempotent projections and resume after crash
CREATE TABLE IF NOT EXISTS projection_offsets (
    -- Tenant isolation
    tenant_id UUID NOT NULL,
    
    -- Aggregate identifier (stream identifier)
    aggregate_id UUID NOT NULL,
    
    -- Projection identifier (e.g., "inventory.stock")
    projection_name TEXT NOT NULL,
    
    -- Last processed sequence_number for this stream
    last_sequence_number BIGINT NOT NULL,
    
    -- Timestamp when offset was last updated
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Primary key: one offset per (tenant, aggregate, projection)
    CONSTRAINT projection_offsets_pkey PRIMARY KEY (tenant_id, aggregate_id, projection_name),
    
    -- Constraints
    CONSTRAINT projection_offsets_sequence_positive CHECK (last_sequence_number >= 0),
    CONSTRAINT projection_offsets_name_not_empty CHECK (projection_name != '')
);

-- Indexes for projection_offsets queries

-- Primary query: Get offset for tenant + aggregate + projection (used during apply_envelope)
CREATE INDEX IF NOT EXISTS idx_projection_offsets_lookup 
    ON projection_offsets (tenant_id, aggregate_id, projection_name);

-- Tenant-wide offset queries (used for rebuild/replay)
CREATE INDEX IF NOT EXISTS idx_projection_offsets_tenant 
    ON projection_offsets (tenant_id, projection_name);

-- Comments for documentation
COMMENT ON TABLE inventory_stock IS 'Read model for inventory stock. Disposable and rebuildable from events.';
COMMENT ON COLUMN inventory_stock.tenant_id IS 'Tenant identifier (matches events.tenant_id)';
COMMENT ON COLUMN inventory_stock.item_id IS 'Inventory item identifier (matches events.aggregate_id)';
COMMENT ON COLUMN inventory_stock.name IS 'Item name (from ItemCreated event)';
COMMENT ON COLUMN inventory_stock.quantity IS 'Current stock quantity (sum of StockAdjusted events)';
COMMENT ON COLUMN inventory_stock.updated_at IS 'Timestamp when read model was last updated';

COMMENT ON TABLE projection_offsets IS 'Projection cursors/checkpoints. Tracks last processed sequence_number per stream for idempotency and resume support.';
COMMENT ON COLUMN projection_offsets.tenant_id IS 'Tenant identifier (matches events.tenant_id)';
COMMENT ON COLUMN projection_offsets.aggregate_id IS 'Aggregate identifier (matches events.aggregate_id)';
COMMENT ON COLUMN projection_offsets.projection_name IS 'Projection identifier (e.g., "inventory.stock")';
COMMENT ON COLUMN projection_offsets.last_sequence_number IS 'Last processed sequence_number for this stream (cursor position)';
COMMENT ON COLUMN projection_offsets.updated_at IS 'Timestamp when offset was last updated';

-- Function to clear all read model data for a tenant (for rebuilds)
CREATE OR REPLACE FUNCTION clear_tenant_read_models(p_tenant_id UUID)
RETURNS void AS $$
BEGIN
    DELETE FROM inventory_stock WHERE tenant_id = p_tenant_id;
    -- Note: projection_offsets are also cleared on rebuild (separate operation)
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION clear_tenant_read_models IS 'Clear all read model data for a tenant (used during projection rebuilds)';

-- Function to clear projection offsets for a tenant (for rebuilds)
CREATE OR REPLACE FUNCTION clear_tenant_offsets(
    p_tenant_id UUID,
    p_projection_name TEXT
)
RETURNS void AS $$
BEGIN
    DELETE FROM projection_offsets 
    WHERE tenant_id = p_tenant_id AND projection_name = p_projection_name;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION clear_tenant_offsets IS 'Clear projection offsets for a tenant + projection (used during rebuilds)';

