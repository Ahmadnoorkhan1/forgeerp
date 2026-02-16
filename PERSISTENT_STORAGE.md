# Persistent Storage & Database Configuration

Consolidates setup, configuration, and verification for enabling persistent data storage using PostgreSQL and Redis.

## Table of Contents

1. [Quick Start (3 minutes)](#quick-start)
2. [Detailed Setup](#detailed-setup)
3. [Environment Configuration](#environment-configuration)
4. [Architecture](#architecture)
5. [Verification & Testing](#verification--testing)
6. [Troubleshooting](#troubleshooting)
7. [Reference](#reference)

---

## Quick Start

For those in a hurry, here's the 3-step setup:

### Step 1: Start Docker Services (30 seconds)

```bash
# Navigate to project root
cd /path/to/forgeerp

# Start PostgreSQL and Redis
docker compose up -d

# Verify services are running
docker compose ps
# Should show: forgeerp-postgres-1 running, forgeerp-redis-1 running
```

### Step 2: Run Database Migrations (10 seconds)

```bash
# Apply all migrations (creates/updates tables)
docker compose run --rm migrate

# Verify tables exist
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\dt"
# Should show: user_credentials, users_read_model, inventory_stock, etc.
```

### Step 3: Run API with Persistent Storage (Ongoing)

```bash
# Build with redis feature
cargo build -p forgeerp-api --features redis

# Run API with persistent storage enabled
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Check logs for:
# "Building persistent services..."
# "Connecting to Postgres..."
# "Connecting to Redis..."
```

### Test It Works

```bash
# Terminal 2: Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username":"john",
    "email":"john@example.com",
    "password":"pass123"
  }'

# Should return HTTP 201 with token

# Check database
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username, user_id FROM user_credentials LIMIT 1;"

# Should show the registered user

# Restart API (CTRL+C, then run again)
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Login should still work - data persisted! ✅
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"john",
    "email":"john@example.com",
    "password":"pass123"
  }'
```

**Result**: After Step 3, data persists across restarts ✅

---

## Detailed Setup

### Your Current Situation

```
✅ Registration works (returns JWT token)
✅ In-memory storage works
❌ Data NOT in user_credentials table
❌ Data NOT in users_read_model table
❌ Data lost on server restart

Reason: System uses in-memory storage by default
```

### What We're Installing

| Component | Purpose | Technology |
|-----------|---------|------------|
| PostgreSQL | Event store & read models | Postgres 13+ |
| Redis | Event bus & caching | Redis 6+ |
| Migrations | Database schema | SQL migrations |
| Features | Compilation flags | Cargo features |

---

### Prerequisites

Before starting, verify you have:

1. **Docker & Docker Compose**
   ```bash
   docker --version
   docker-compose --version
   ```

2. **Rust toolchain**
   ```bash
   rustc --version
   cargo --version
   ```

3. **PostgreSQL client (optional, for manual querying)**
   ```bash
   psql --version
   ```

### Step 1: Start PostgreSQL and Redis

#### Option A: Using docker-compose (Recommended)

```bash
# From project root
cd /path/to/forgeerp

# Start all services
docker compose up -d

# Verify status
docker compose ps

# Expected output:
# NAME                   IMAGE              STATUS
# forgeerp-postgres-1    postgres:13-alpine running
# forgeerp-redis-1       redis:7-alpine     running
```

#### Option B: Using docker commands directly

```bash
# Start PostgreSQL
docker run -d \
  --name forgeerp-postgres \
  -e POSTGRES_USER=forgeerp \
  -e POSTGRES_PASSWORD=forgeerp \
  -e POSTGRES_DB=forgeerp \
  -p 5432:5432 \
  postgres:13-alpine

# Start Redis
docker run -d \
  --name forgeerp-redis \
  -p 6379:6379 \
  redis:7-alpine
```

### Step 2: Apply Database Migrations

Migrations create all necessary tables and indexes.

```bash
# Run migrations from docker-compose
docker compose run --rm migrate

# Expected output:
# [INFO] Applying migration: 001_create_events_table.sql
# [INFO] Applying migration: 002_create_snapshots_table.sql
# [INFO] Applying migration: 003_create_rls_policies.sql
# [INFO] Applying migration: 004_create_read_models.sql
# [INFO] Applying migration: 005_create_users_table.sql  ← NEW!
# [INFO] All migrations applied successfully
```

#### Verify Migrations Succeeded

```bash
# List all tables
docker compose exec postgres psql \
  -U forgeerp -d forgeerp -c "\dt"

# Expected output:
# Schema |           Name           | Type  | Owner
# --------+----------------------------+-------+----------
# public | events                     | table | forgeerp
# public | inventory_stock            | table | forgeerp
# public | ledger_entries             | table | forgeerp
# public | projection_offsets         | table | forgeerp
# public | snapshots                  | table | forgeerp
# public | user_credentials           | table | forgeerp
# public | users_read_model           | table | forgeerp
```

Or using psql directly:

```bash
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\dt"
```

### Step 3: Configure API with Redis Feature

The API needs to be built with the `redis` feature enabled to use persistent storage.

#### Build with Redis Feature

```bash
# Build API with redis feature
cargo build -p forgeerp-api --features redis

# Or with release optimizations
cargo build -p forgeerp-api --release --features redis
```

#### Check Feature is Enabled

Verify in [crates/api/Cargo.toml](crates/api/Cargo.toml):

```toml
[features]
default = ["redis"]  # ← Should be present
redis = []            # ← Should be present
```

### Step 4: Run API with Persistent Storage

Now start the API with the environment variable enabled:

```bash
# Development with debug logging
RUST_LOG=debug USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Or production release build
USE_PERSISTENT_STORES=true ./target/release/forgeerp-api
```

#### Verify it's Using Persistent Storage

Check the startup logs:

```
[INFO] Building persistent services...
[INFO] Connecting to Postgres: postgres://...
[INFO] Connecting to Redis: redis://...
[INFO] Running migrations check...
[INFO] API listening on 0.0.0.0:8080
```

**Key indicators**:
- "Building persistent services" → ✅ Persistent mode active
- "Connecting to Postgres" → ✅ Database connected
- "Connecting to Redis" → ✅ Redis connected

---

## Environment Configuration

### Environment Variables

Create or update a `.env` file in the project root:

```dotenv
# API Configuration
API_HOST=0.0.0.0
API_PORT=8080

# Logging
RUST_LOG=debug

# Database (PostgreSQL)
DATABASE_URL=postgres://forgeerp:forgeerp@localhost:5432/forgeerp
POSTGRES_USER=forgeerp
POSTGRES_PASSWORD=forgeerp
POSTGRES_DB=forgeerp

# Cache & Event Bus (Redis)
REDIS_URL=redis://localhost:6379

# IMPORTANT: Enable persistent storage!
USE_PERSISTENT_STORES=true

# Authentication
AUTH_SECRET=your-secret-key-here-min-32-chars
AUTH_TOKEN_EXPIRY_HOURS=24

# Optional: Migrations
MIGRATE_ON_STARTUP=true
```

### Configuration Files

#### docker-compose.yml

The project includes `docker-compose.yml` with PostgreSQL and Redis services:

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:13-alpine
    environment:
      POSTGRES_USER: forgeerp
      POSTGRES_PASSWORD: forgeerp
      POSTGRES_DB: forgeerp
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

  migrate:
    build:
      context: .
      dockerfile: docker/migrate.Dockerfile
    depends_on:
      - postgres
    environment:
      DATABASE_URL: postgres://forgeerp:forgeerp@postgres:5432/forgeerp
    command: bash scripts/migrate.sh

  api:
    build:
      context: .
      dockerfile: docker/api.Dockerfile
    depends_on:
      - postgres
      - redis
      - migrate
    environment:
      USE_PERSISTENT_STORES: "true"
      DATABASE_URL: postgres://forgeerp:forgeerp@postgres:5432/forgeerp
      REDIS_URL: redis://redis:6379
      RUST_LOG: debug
    ports:
      - "8080:8080"

volumes:
  postgres_data:
  redis_data:
```

---

## Architecture

### In-Memory Mode (Default)

```
HTTP Request (POST /auth/register)
  ↓
Handler validates & creates command
  ↓
Event Sourcing (In-Memory):
  ├─ Event Store: In-memory Vec<Event>
  ├─ Event Bus: In-memory channel
  └─ Projections: In-memory HashMap
       ├─ user_credentials: HashMap<username, credential>
       ├─ users_read_model: HashMap<user_id, UserProfile>
       └─ inventory_stock: HashMap<item_id, stock>
  ↓
Response: 201 Created ✅

BUT: On server restart → All data lost ❌
```

### Persistent Storage Mode (After Step 3)

```
HTTP Request (POST /auth/register)
  ↓
Handler validates & creates command
  ↓
Event Sourcing (Database):
  ├─ Event Store: PostgreSQL events table
  │   └─ Each command generates event(s) stored durably
  ├─ Event Bus: Redis Streams
  │   └─ Events distributed to projections reliably
  └─ Projections: PostgreSQL tables
       ├─ user_credentials table: username → password_hash
       ├─ users_read_model table: user profiles + roles
       ├─ inventory_stock table: item quantities
       └─ ledger table: account balances
  ↓
Response: 201 Created ✅
AND: Data persisted in PostgreSQL ✅
AND: Survives restart ✅
```

### Data Flow Diagram

```
┌─────────────────────────────────────────────────────────┐
│ User Registration Request                               │
│ POST /auth/register                                     │
│ {username, email, password}                             │
└────────────────────┬────────────────────────────────────┘
                     ↓
        ┌────────────────────────┐
        │ Validation & Commands  │
        │ ├─ Validate input      │
        │ ├─ Hash password       │
        │ └─ Create UserCreated  │
        │    command             │
        └────────────┬───────────┘
                     ↓
        ┌─────────────────────────────────────┐
        │ Event Store (PostgreSQL)            │
        │ ├─ Load aggregate (new)             │
        │ ├─ Apply command                    │
        │ ├─ Generate UserCreated event       │
        │ ├─ Save to events table             │
        │ └─ Assign version 1                 │
        └────────────┬────────────────────────┘
                     ↓
        ┌─────────────────────────────────────┐
        │ Event Bus (Redis Streams)           │
        │ ├─ Publish UserCreated event        │
        │ ├─ Channel: user.events             │
        │ └─ Subscribers: projections         │
        └────────────┬────────────────────────┘
                     ↓
        ┌─────────────────────────────────────┐
        │ Projections (PostgreSQL)            │
        │                                     │
        │ UsersProjection:                    │
        │ ├─ Handle UserCreated event         │
        │ ├─ Extract username, password_hash  │
        │ ├─ INSERT into user_credentials    │
        │ ├─ Extract user details             │
        │ └─ INSERT into users_read_model    │
        └────────────┬────────────────────────┘
                     ↓
        ┌─────────────────────────────────────┐
        │ Response: 201 Created               │
        │ {token, user, tenant}               │
        │                                     │
        │ Data now persisted in tables! ✅    │
        └─────────────────────────────────────┘
```

---

## Verification & Testing

### Test 1: Registration Stores Data

```bash
# Terminal 1: Run API
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Terminal 2: Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username":"alice",
    "email":"alice@example.com",
    "password":"SecurePass123"
  }'

# Should return HTTP 201 with token and user info
```

**Expected logs** (Terminal 1):
```
[REGISTER] Attempting to store credentials for username: alice
[CredentialStore] Inserting credential for username: alice
[PostgreSQL] Inserting into user_credentials table...
[PostgreSQL] Inserting into users_read_model table...
```

### Test 2: Verify Data in Database

```bash
# Check user_credentials table
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp \
  -c "SELECT username, user_id FROM user_credentials LIMIT 5;"

# Expected output:
#  username | user_id
# ──────────┼────────────────────────────────────
#  alice    | 550e8400-e29b-41d4-a716-446655440000

# Check users_read_model table
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp \
  -c "SELECT user_id, email, status, roles FROM users_read_model LIMIT 5;"

# Expected output:
#              user_id              |       email       | status |    roles
# ────────────────────────────────────┼──────────────────────┼────────┼──────────────
#  550e8400-e29b-41d4-a716-446655440000 | alice@example.com │ Active │ {admin,user}
```

### Test 3: Data Persists After Restart

```bash
# Terminal 1: Stop API (CTRL+C)

# Wait a moment

# Terminal 1: Restart API
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Terminal 2: Login with credentials from Test 1
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"alice",
    "email":"alice@example.com",
    "password":"SecurePass123"
  }'

# Should return HTTP 200 with token
# If it works → Data persisted across restart! ✅
```

**Key indicator**: If login succeeds after restart, persistent storage is working!

### Test 4: Multiple Users

```bash
# Register User 1
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"bob","email":"bob@example.com","password":"Pass123"}'

# Register User 2
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"charlie","email":"charlie@example.com","password":"Pass456"}'

# Check both are in database
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp \
  -c "SELECT username FROM user_credentials ORDER BY username;"

# Expected output:
#  username
# ──────────
#  alice
#  bob
#  charlie
```

### Verification Checklist

- [ ] Docker services running: `docker compose ps` shows postgres + redis
- [ ] PostgreSQL accessible: `psql ...` connects successfully  
- [ ] Migrations applied: `psql ... \dt` shows all tables
- [ ] USE_PERSISTENT_STORES=true is set
- [ ] Redis feature enabled: `grep redis crates/api/Cargo.toml`
- [ ] API builds without errors: `cargo build -p forgeerp-api --features redis`
- [ ] API starts with persistent logs: search for "Building persistent services"
- [ ] Register succeeds (HTTP 201)
- [ ] Data appears in user_credentials table
- [ ] Data appears in users_read_model table  
- [ ] Login works after registration
- [ ] Login still works after API restart ✅

---

## Troubleshooting

### Issue: "Connection refused" when running migrations

**Cause**: PostgreSQL not running

**Solution**:
```bash
# Start services
docker compose up -d

# Verify running
docker compose ps

# Should show forgeerp-postgres-1 running
```

### Issue: "Table not found" errors

**Cause**: Migrations not applied

**Solution**:
```bash
# Run migrations
docker compose run --rm migrate

# Verify tables exist
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\dt"
```

### Issue: "Data still not in tables after registration"

**Cause**: One of the following:

**Debug steps**:
```bash
# 1. Check USE_PERSISTENT_STORES is set
echo $USE_PERSISTENT_STORES  # Should print: true

# 2. Check logs show "Building persistent services"
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api 2>&1 | \
  grep "Building\|Connecting"

# 3. Check Postgres connection works
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "SELECT 1"

# 4. Check migrations were applied
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\dt"

# 5. Register and check immediately
curl ... /auth/register  # Then...
psql ... -c "SELECT * FROM user_credentials"
```

### Issue: "feature not enabled" when building

**Cause**: redis feature not enabled during build

**Solution**:
```bash
# Build with redis feature explicitly
cargo build -p forgeerp-api --features redis

# Or check Cargo.toml has the feature defined
grep -A 2 "\[features\]" crates/api/Cargo.toml
```

### Issue: API works in-memory mode but not persistent

**Cause**: USE_PERSISTENT_STORES not set or PostgreSQL not connected

**Solution**:
```bash
# Always set this when testing persistent
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api

# Verify it connected (should see in logs)
RUST_LOG=debug USE_PERSISTENT_STORES=true cargo run -p forgeerp-api 2>&1 | \
  grep -i "postgres\|redis"
```

### Issue: Slow queries or timeouts

**Cause**: Missing indexes or database not optimized

**Solution**:
```bash
# Check indexes exist
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\d user_credentials"

# Should show indexes on: (tenant_id, username)

# Force index creation (runs from migrations)
docker compose run --rm migrate
```

### Troubleshooting Checklist

- [ ] Is Docker running? `docker ps`
- [ ] Are services running? `docker compose ps`
- [ ] Can you connect to Postgres? `psql postgres://...`
- [ ] Are tables created? `psql ... \dt`
- [ ] Is USE_PERSISTENT_STORES set? `echo $USE_PERSISTENT_STORES`
- [ ] Is redis feature enabled? `grep redis crates/api/Cargo.toml`
- [ ] Do logs show "Connecting to Postgres"? Run with RUST_LOG=debug
- [ ] Is there data in tables? `psql ... SELECT COUNT(*) FROM user_credentials`

---

## Reference

### Connection Strings

```
PostgreSQL:
  postgres://forgeerp:forgeerp@localhost:5432/forgeerp

Redis:
  redis://localhost:6379
```

### Useful Commands

```bash
# Docker operations
docker compose up -d          # Start all services
docker compose down           # Stop all services
docker compose ps             # Show status
docker compose logs postgres  # View PostgreSQL logs
docker compose logs redis     # View Redis logs

# Database operations
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp  # Connect
\dt                          # List tables
\d user_credentials          # Describe table
SELECT * FROM user_credentials LIMIT 10;  # View data

# API operations
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api        # Start API
cargo build -p forgeerp-api --features redis                # Build
RUST_LOG=debug cargo run -p forgeerp-api                    # Debug
```

### Database Tables

| Table | Purpose |
|-------|---------|
| user_credentials | username → password_hash mapping for login |
| users_read_model | User profiles with email, roles, status |
| events | Immutable event log (audit trail) |
| inventory_stock | Current inventory levels |
| ledger_entries | Account balances |
| projection_offsets | Projection state tracking |
| snapshots | Aggregate snapshots (optimization) |

### Environment Variables Quick Reference

| Variable | Default | Purpose |
|----------|---------|---------|
| USE_PERSISTENT_STORES | false | Enable database mode |
| DATABASE_URL | (none) | PostgreSQL connection string |
| REDIS_URL | (none) | Redis connection string |
| RUST_LOG | info | Logging level |
| API_HOST | 0.0.0.0 | Listen address |
| API_PORT | 8080 | Listen port |

---

## Summary

✅ **Quick Start Complete**: Data now persists in PostgreSQL
✅ **Verified**: Survives server restart
✅ **Scaled**: Multiple servers can share database
✅ **Ready**: For production deployment

**Next steps**:
1. See [AUTHENTICATION.md](AUTHENTICATION.md) for JWT and authentication details
2. See [REGISTRATION.md](REGISTRATION.md) for user registration flow  
3. See [TESTING.md](TESTING.md) for comprehensive test suite

**You're ready for production!** 🚀
