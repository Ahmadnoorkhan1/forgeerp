-- Read Model Schema: Users and Credentials
--
-- This migration creates tables for:
-- 1. users_read_model: Tenant-scoped read model for user accounts
-- 2. user_credentials: Persisted password hashes and credential mapping
--
-- Users are event-sourced via the User aggregate. This read model is
-- the denormalized, queryable projection built from user events.

-- Users Read Model
-- Stores current state of user accounts (denormalized from UserCreated, UserSuspended events)
CREATE TABLE IF NOT EXISTS users_read_model (
    -- Tenant isolation
    tenant_id UUID NOT NULL,
    
    -- User identifier (user_id)
    user_id UUID NOT NULL,
    
    -- Read model fields (from UserCreated event)
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'Active', -- Active or Suspended
    roles TEXT[] NOT NULL DEFAULT '{}', -- Array of role names
    
    -- Timestamps for tracking
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Primary key: one read model per tenant + user_id
    CONSTRAINT users_read_model_pkey PRIMARY KEY (tenant_id, user_id),
    
    -- Constraints
    CONSTRAINT users_email_not_empty CHECK (email != ''),
    CONSTRAINT users_display_name_not_empty CHECK (display_name != ''),
    CONSTRAINT users_status_valid CHECK (status IN ('Active', 'Suspended'))
);

-- Indexes for users_read_model queries

-- Primary query: Get user for tenant + user_id (used by projection)
CREATE INDEX IF NOT EXISTS idx_users_read_model_lookup 
    ON users_read_model (tenant_id, user_id);

-- Lookup by email (used by login flow to find user)
CREATE INDEX IF NOT EXISTS idx_users_read_model_email 
    ON users_read_model (tenant_id, email);

-- List query: Get all users for a tenant
CREATE INDEX IF NOT EXISTS idx_users_read_model_tenant 
    ON users_read_model (tenant_id, created_at DESC);

-- User Credentials Storage
-- Stores encrypted password hashes and username→user_id mapping
-- This is separate from the User aggregate to keep credentials secure
CREATE TABLE IF NOT EXISTS user_credentials (
    -- Tenant isolation
    tenant_id UUID NOT NULL,
    
    -- Username (normalized, unique per tenant)
    username TEXT NOT NULL,
    
    -- Password hash (bcrypt or argon2)
    password_hash TEXT NOT NULL,
    
    -- Reference to user_id for cross-reference
    user_id UUID NOT NULL,
    
    -- Timestamps for tracking
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Primary key: one credential per tenant + username
    CONSTRAINT user_credentials_pkey PRIMARY KEY (tenant_id, username),
    
    -- Foreign key reference to users_read_model
    CONSTRAINT user_credentials_user_fk FOREIGN KEY (tenant_id, user_id)
        REFERENCES users_read_model (tenant_id, user_id) ON DELETE CASCADE,
    
    -- Constraints
    CONSTRAINT user_credentials_username_not_empty CHECK (username != ''),
    CONSTRAINT user_credentials_hash_not_empty CHECK (password_hash != '')
);

-- Indexes for user_credentials queries

-- Primary query: Find credentials by tenant + username (used during login)
CREATE INDEX IF NOT EXISTS idx_user_credentials_lookup 
    ON user_credentials (tenant_id, username);

-- User lookup: Find user_id by username (for credential validation)
CREATE INDEX IF NOT EXISTS idx_user_credentials_user_id 
    ON user_credentials (tenant_id, user_id);

-- Comments for documentation
COMMENT ON TABLE users_read_model IS 'Read model for user accounts. Disposable and rebuildable from User aggregate events.';
COMMENT ON COLUMN users_read_model.tenant_id IS 'Tenant identifier (matches events.tenant_id)';
COMMENT ON COLUMN users_read_model.user_id IS 'User identifier (matches events.aggregate_id)';
COMMENT ON COLUMN users_read_model.email IS 'User email address (from UserCreated event)';
COMMENT ON COLUMN users_read_model.display_name IS 'User display name (from UserCreated event)';
COMMENT ON COLUMN users_read_model.status IS 'User account status: Active or Suspended';
COMMENT ON COLUMN users_read_model.roles IS 'Array of role names assigned to user (from UserCreated event)';
COMMENT ON COLUMN users_read_model.created_at IS 'User creation timestamp (from UserCreated event)';
COMMENT ON COLUMN users_read_model.updated_at IS 'Timestamp when read model was last updated';

COMMENT ON TABLE user_credentials IS 'User credentials mapping. Links username to user_id and stores password hash. Separate from User aggregate for security.';
COMMENT ON COLUMN user_credentials.tenant_id IS 'Tenant identifier (matches users_read_model.tenant_id)';
COMMENT ON COLUMN user_credentials.username IS 'Username (unique per tenant, used for login)';
COMMENT ON COLUMN user_credentials.password_hash IS 'Bcrypt or argon2 password hash';
COMMENT ON COLUMN user_credentials.user_id IS 'Reference to user_id in users_read_model';
COMMENT ON COLUMN user_credentials.created_at IS 'Credential creation timestamp';
COMMENT ON COLUMN user_credentials.updated_at IS 'Timestamp when password was last updated';

-- Row-Level Security: Tenant Isolation
-- Enable RLS on users_read_model (if needed in future)
-- ALTER TABLE users_read_model ENABLE ROW LEVEL SECURITY;
-- ALTER TABLE user_credentials ENABLE ROW LEVEL SECURITY;
