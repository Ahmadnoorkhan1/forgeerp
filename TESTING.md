# Comprehensive Testing Guide

This document consolidates all testing procedures for authentication, registration, and projections.

## Table of Contents

1. [Quick Test (5 minutes)](#quick-test)
2. [In-Memory Mode Tests](#in-memory-mode-tests)
3. [Persistent Storage Tests](#persistent-storage-tests)
4. [Projection Tests](#projection-tests)
5. [Debugging Tips](#debugging-tips)
6. [Troubleshooting](#troubleshooting)

---

## Quick Test

For a quick verification that the fix is working, run this 5-minute test:

### Setup (1 minute)

```bash
# Terminal 1: Run API with debug logging
RUST_LOG=debug cargo run -p forgeerp-api

# Wait for: "API listening on 0.0.0.0:8080"
```

### Test Registration (2 minutes)

```bash
# Terminal 2: Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username":"alice",
    "email":"alice@example.com",
    "password":"secure123"
  }' | jq '.'

# Expected: HTTP 201 with token and user data
```

**Check Terminal 1 logs for**:
```
[REGISTER] Attempting to store credentials for username: alice
[CredentialStore] Inserting credential for username: alice
[REGISTER] ✅ Credentials stored successfully
```

### Test Login (2 minutes)

```bash
# Terminal 2: Login with same credentials
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"alice",
    "email":"alice@example.com",
    "password":"secure123"
  }' | jq '.'

# Expected: HTTP 200 with token and user data
```

**Check Terminal 1 logs for**:
```
[LOGIN] Attempting login for username: alice
[CredentialStore] Looking for username: 'alice' | Store has 1 total credentials | Found: true
[LOGIN] ✅ Credentials found for username: alice
[LOGIN] ✅ Password verified for username: alice
```

**Key Indicator**: If you see `Found: true` → Arc fix is working! ✅

---

## In-Memory Mode Tests

These tests verify the in-memory authentication system (Phase 1).

### Test 1: Basic Registration

**Objective**: Verify registration stores credentials

```bash
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username":"testuser",
    "email":"test@example.com",
    "password":"Pass123"
  }'
```

**Expected Response**: HTTP 201 Created
```json
{
  "token": "eyJ0eXAiOiJKV1QiLCJhbGc...",
  "user": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "username": "testuser",
    "email": "test@example.com",
    "roles": ["admin", "user"],
    "display_name": "testuser",
    "tenant_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479"
  },
  "tenant": {
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "name": "testuser's Workspace"
  }
}
```

**Expected Logs**:
```
[REGISTER] Attempting to store credentials for username: testuser
[CredentialStore] Inserting credential for username: testuser
[CredentialStore] Store now contains 1 credentials
```

### Test 2: Immediate Login After Registration

**Objective**: Verify login works right after registration (this was broken before)

```bash
# From Test 1, immediately run login with same credentials
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"testuser",
    "email":"test@example.com",
    "password":"Pass123"
  }'
```

**Expected Response**: HTTP 200 OK (same format as register)

**Expected Logs**:
```
[LOGIN] Attempting login for username: testuser
[CredentialStore] Looking for username: 'testuser' | Store has 1 total credentials | Found: true
[LOGIN] ✅ Credentials found for username: testuser
[LOGIN] ✅ Password verified for username: testuser
```

**This test verifies the Arc fix is working!** ✅

### Test 3: Wrong Password Rejection

**Objective**: Verify incorrect passwords are rejected

```bash
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"testuser",
    "email":"test@example.com",
    "password":"wrongpassword"
  }'
```

**Expected Response**: HTTP 401 Unauthorized
```json
{
  "error": "invalid credentials",
  "error_type": "unauthorized"
}
```

**Expected Logs**:
```
[LOGIN] ❌ Password verification failed for username: testuser
```

### Test 4: Multiple Users

**Objective**: Verify the store handles multiple users correctly

```bash
# Register User 1
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"alice@example.com","password":"Pass123"}'

# Register User 2
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"bob","email":"bob@example.com","password":"Pass456"}'

# Register User 3
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"charlie","email":"charlie@example.com","password":"Pass789"}'
```

**Expected Logs** (after each registration):
```
[CredentialStore] Store now contains 1 credentials  ← after alice
[CredentialStore] Store now contains 2 credentials  ← after bob
[CredentialStore] Store now contains 3 credentials  ← after charlie
```

**Verify all can login**:
```bash
# Login as alice
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"alice@example.com","password":"Pass123"}'

# Login as bob
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"bob","email":"bob@example.com","password":"Pass456"}'

# Login as charlie
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"charlie","email":"charlie@example.com","password":"Pass789"}'
```

**Each login should succeed with HTTP 200**

**Expected Logs for alice login**:
```
[CredentialStore] Store now contains 3 total credentials
[CredentialStore] Looking for username: 'alice' | Store has 3 total credentials | Found: true
```

### Test 5: Non-Existent User

**Objective**: Verify login fails for non-registered users

```bash
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"nonexistent",
    "email":"none@example.com",
    "password":"Pass123"
  }'
```

**Expected Response**: HTTP 401 Unauthorized

**Expected Logs**:
```
[CredentialStore] Looking for username: 'nonexistent' | Store has N total credentials | Found: false
[LOGIN] ❌ Credentials NOT found
```

### In-Memory Test Summary

| Test | Status | Indicator |
|------|--------|-----------|
| Register stores credentials | ✅ | HTTP 201 + logs show insert |
| Login after register | ✅ | HTTP 200 + logs show "Found: true" |
| Multiple users | ✅ | Store count increases, all can login |
| Wrong password rejected | ✅ | HTTP 401 |
| Non-existent user rejected | ✅ | HTTP 401, "Found: false" |

---

## Persistent Storage Tests

These tests verify persistence with PostgreSQL and Redis (Phase 2).

### Prerequisites

```bash
# Start Docker services
docker compose up -d

# Apply migrations
docker compose run --rm migrate

# Verify services
docker compose ps

# Verify migrations applied
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "\dt"
```

### Test 1: Registration Stores in Database

**Objective**: Verify data is persisted to PostgreSQL

```bash
# Terminal 1: Run API with persistent storage
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api --features redis

# Terminal 2: Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username":"persistence_test",
    "email":"persist@example.com",
    "password":"PersistPass123"
  }'
```

**Check user_credentials table**:
```bash
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username, user_id FROM user_credentials WHERE username='persistence_test';"
```

**Expected Output**:
```
        username        |              user_id
─────────────────────────┼──────────────────────────────
 persistence_test        | 550e8400-e29b-41d4-a716-...
```

**Check users_read_model table**:
```bash
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT user_id, email, status, roles FROM users_read_model WHERE email='persist@example.com';"
```

**Expected Output**:
```
              user_id              |       email       | status |    roles
────────────────────────────────────┼──────────────────────┼────────┼──────────────
 550e8400-e29b-41d4-a716-446655... │ persist@example.com │ Active │ {admin,user}
```

### Test 2: Data Persists After API Restart

**Objective**: Verify data survives server restart

```bash
# Terminal 1: Stop API (CTRL+C)

# Terminal 1: Restart API
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api --features redis

# Terminal 2: Login with credentials from Test 1
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username":"persistence_test",
    "email":"persist@example.com",
    "password":"PersistPass123"
  }'
```

**Expected Response**: HTTP 200 OK with token

**Key Indicator**: If login succeeds → data persisted across restart! ✅

**Verify in database one more time**:
```bash
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username FROM user_credentials WHERE username='persistence_test';"
```

### Test 3: Redis Event Bus Delivery

**Objective**: Verify events are delivered via Redis

```bash
# Check Redis has event streams
redis-cli

# In redis-cli:
> XLEN user.events
(integer) N  ← Should be > 0

# See recent events
> XRANGE user.events - + COUNT 5

# Exit
> EXIT
```

### Test 4: Multiple Registrations Accumulate

**Objective**: Verify multiple registrations all persist

```bash
# Register 3 users
for i in 1 2 3; do
  curl -X POST http://localhost:8080/auth/register \
    -H "Content-Type: application/json" \
    -d "{
      \"username\":\"user$i\",
      \"email\":\"user$i@example.com\",
      \"password\":\"Pass${i}${i}${i}\"
    }"
done

# Check database
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username FROM user_credentials WHERE username LIKE 'user%' ORDER BY created_at;"
```

**Expected Output**:
```
 username
──────────
 user1
 user2
 user3
```

### Persistent Storage Test Summary

| Test | Status | Verification |
|------|--------|--------------|
| Data in user_credentials | ✅ | Query shows registered user |
| Data in users_read_model | ✅ | Query shows user profile |
| Survives restart | ✅ | Login works after API restart |
| Redis event bus | ✅ | redis-cli shows events |
| Multiple registrations | ✅ | All users in table |

---

## Projection Tests

These tests verify the projection system is working correctly.

### Test 1: Projection Offset Tracking

**Objective**: Verify projections track processing progress

```bash
# Before registering anything
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT projection_name, aggregate_id, sequence_number, updated_at FROM projection_offsets LIMIT 5;"

# Register a user (from persistent storage test)
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"proj_test","email":"proj@example.com","password":"Pass123"}'

# Check offsets updated
sleep 1
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT projection_name, COUNT(*) FROM projection_offsets GROUP BY projection_name;"
```

**Expected Output**: Should show increased counts indicating projections processed events

### Test 2: Concurrent Registrations

**Objective**: Verify projections handle concurrent events

```bash
# Register 5 users concurrently
for i in {1..5}; do
  (curl -s -X POST http://localhost:8080/auth/register \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"concurrent$i\",\"email\":\"conc$i@example.com\",\"password\":\"Pass$i$i$i\"}" &)
done
wait

# Verify all in database
sleep 1
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT COUNT(*) FROM user_credentials WHERE username LIKE 'concurrent%';"
```

**Expected Output**: `5` (all registrations succeeded)

### Test 3: Event Ordering

**Objective**: Verify events are processed in order

```bash
# Register with specific order
curl -s -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"first","email":"first@example.com","password":"Pass123"}' &

curl -s -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"second","email":"second@example.com","password":"Pass123"}' &

curl -s -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"third","email":"third@example.com","password":"Pass123"}' &

wait

# Check created_at order in database
sleep 1
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username, created_at FROM user_credentials WHERE username IN ('first','second','third') ORDER BY created_at;"
```

**Expected Output**: Timestamps should reflect registration order (may vary with concurrent requests)

---

## Debugging Tips

### Enable Trace Logging

For maximum debugging information:

```bash
RUST_LOG=trace cargo run -p forgeerp-api 2>&1 | tee api.log
```

Then search the logs for specific information:

```bash
# See all credential store operations
grep -i credential api.log | head -20

# See all login attempts
grep -i "^\[LOGIN\]" api.log

# See all registration attempts
grep -i "^\[REGISTER\]" api.log

# See all database operations
grep -i "database\|postgres\|sql" api.log
```

### Check Memory Addresses

Verify Arc is working by comparing pointer addresses:

```bash
RUST_LOG=debug cargo run -p forgeerp-api 2>&1 | grep "auth_stores ptr"
```

Should show **same address** for all requests:
```
[REGISTER] auth_stores ptr: 0x7f8a9c5e2f80
[LOGIN] auth_stores ptr: 0x7f8a9c5e2f80  ← Same!
```

### Count Credentials in Store

From trace logs, find how many credentials exist:

```bash
RUST_LOG=trace cargo run -p forgeerp-api 2>&1 | grep "Store now contains"
```

After 3 registrations:
```
[CredentialStore] Store now contains 1 credentials
[CredentialStore] Store now contains 2 credentials
[CredentialStore] Store now contains 3 credentials
```

### Monitor Database Queries

Watch all database operations:

```bash
USE_PERSISTENT_STORES=true RUST_LOG=debug cargo run -p forgeerp-api 2>&1 | \
  grep -i "insert\|select\|update\|delete\|query"
```

### Check Redis Streams

Monitor Redis event delivery:

```bash
# Terminal 1: Watch Redis
redis-cli XREAD COUNT 1 STREAMS user.events '$' --no-auth-warning

# Terminal 2: Register a user
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"test","email":"test@example.com","password":"Pass123"}'

# Terminal 1 should show the event in Redis
```

---

## Troubleshooting

### Problem: Login fails after register

**Symptom**: Register returns 201, login returns 401

**Debug**:
```bash
# Check logs show "Found: true"
RUST_LOG=debug cargo run -p forgeerp-api 2>&1 | grep "Found:"

# If "Found: false" → Arc fix not applied
# If "Found: true" → Different issue
```

**Solution**:
1. Verify Arc fix is in [crates/api/src/app/services.rs](crates/api/src/app/services.rs)
2. Recompile: `cargo clean && cargo build -p forgeerp-api`
3. Run only one API instance
4. Check pointer addresses match (see debugging tips)

### Problem: Logs show "Store now contains 0 credentials"

**Symptom**: Registrations don't increase store count

**Cause**: Credentials not being inserted

**Debug**:
```bash
# Check if register handler was called
RUST_LOG=debug cargo run -p forgeerp-api 2>&1 | grep "\[REGISTER\]"

# Check if credentials insert was called
RUST_LOG=trace cargo run -p forgeerp-api 2>&1 | grep "Inserting credential"
```

**Solution**:
1. Verify register endpoint is accessible: `curl -v http://localhost:8080/auth/register`
2. Check request body is valid JSON
3. Check password is at least 6 characters

### Problem: "Feature not enabled" error

**Symptom**: Build fails with "feature redis not enabled"

**Cause**: Building without `--features redis`

**Solution**:
```bash
# Build with redis feature
cargo build -p forgeerp-api --features redis

# Or set as default in Cargo.toml
```

### Problem: Data in database but still can't login

**Symptom**: Register succeeds, data in tables, but login fails

**Cause**: Projection not fully populated or caching issue

**Debug**:
```bash
# Check database has complete data
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT * FROM user_credentials ORDER BY created_at DESC LIMIT 1;"

# Check for errors in logs
RUST_LOG=debug cargo run -p forgeerp-api 2>&1 | grep -i "error"

# Check password hash exists
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c \
  "SELECT username, LENGTH(password_hash) as hash_len FROM user_credentials WHERE username='testuser';"
```

**Solution**:
1. Wait a moment for projections to catch up (projections are async)
2. Check database connection is working: `psql ... SELECT 1;`
3. Check password hasn't expired or been trimmed

### Problem: PostgreSQL connection refused

**Symptom**: "Connection refused" when starting API

**Cause**: PostgreSQL not running

**Solution**:
```bash
# Start services
docker compose up -d postgres redis

# Verify running
docker compose ps

# Test connection
psql postgres://forgeerp:forgeerp@localhost:5432/forgeerp -c "SELECT 1;"
```

### Troubleshooting Checklist

- [ ] Is API running? `curl http://localhost:8080/auth/register -X OPTIONS`
- [ ] Are logs showing request? `RUST_LOG=debug` and check output
- [ ] Is password correct? (must match exactly between register and login)
- [ ] Is username correct? (must match exactly, case-sensitive?)
- [ ] Are pointer addresses same? (indicates Arc is working)
- [ ] Is store count increasing? (indicates insert happening)
- [ ] Is data in database? (for persistent mode)
- [ ] Is Redis running? (for persistent mode)

---

## Success Criteria

You have successfully completed testing when:

### In-Memory Mode
- ✅ Register returns HTTP 201
- ✅ Login immediately after register returns HTTP 200
- ✅ Multiple users can register and login
- ✅ Wrong password is rejected
- ✅ Logs show "Found: true" for login
- ✅ Logs show pointer addresses match

### Persistent Storage Mode  
- ✅ All in-memory tests pass
- ✅ Data appears in user_credentials table
- ✅ Data appears in users_read_model table
- ✅ Login works after API restart
- ✅ Redis has event streams
- ✅ Concurrent registrations work

### Full Coverage
- ✅ 3+ users successfully registered and logged in
- ✅ Password validation working
- ✅ Multiple concurrent requests handled
- ✅ Database consistency verified
- ✅ Logs don't show any errors

**Congratulations!** 🎉 Your authentication system is working correctly!

---

## Next Steps

Once all tests pass:
1. See [AUTHENTICATION.md](AUTHENTICATION.md) for architecture details
2. See [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) for production setup
3. See [REGISTRATION.md](REGISTRATION.md) for user registration features
