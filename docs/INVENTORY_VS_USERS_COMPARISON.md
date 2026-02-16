# How to Make User Registration Work Like Inventory Items

This document shows you the **exact pattern** used by inventory items and how to apply it to user registration.

## Side-by-Side Comparison

### Inventory Item Flow

```
CLIENT: POST /inventory/items
        ├─ name: "Widget A"
        └─ initial_quantity: 100
                ↓
        ┌────────────────────────────────────────┐
        │ HTTP Handler: create_item()             │
        │ (crates/api/src/app/routes/inventory.rs)│
        └────────────────────────────────────────┘
                ↓
        1. Validate input
        2. Create command:
           CreateItem {
             tenant_id,
             item_id (new UUID),
             name,
             occurred_at
           }
        3. Authorize with RBAC
        4. Dispatch command
                ↓
        ┌─────────────────────────────────────────┐
        │ services.dispatch::<InventoryItem>()    │
        │ ├─ Loads aggregate from event store     │
        │ ├─ Applies command                      │
        │ │  └─ Produces: ItemCreated event       │
        │ ├─ Appends event to store               │
        │ ├─ Publishes to event bus               │
        │ └─ Returns committed events             │
        └─────────────────────────────────────────┘
                ↓
        ┌─────────────────────────────────────────┐
        │ Event Bus: EventEnvelope published      │
        │ ├─ event_type: "inventory.item.created" │
        │ ├─ tenant_id                            │
        │ ├─ aggregate_id (item_id)               │
        │ └─ payload: ItemCreated                 │
        └─────────────────────────────────────────┘
                ↓
        ┌─────────────────────────────────────────┐
        │ Projection: InventoryStockProjection    │
        │ ├─ Subscribes to event bus              │
        │ ├─ Handles ItemCreated                  │
        │ │  └─ INSERT into inventory_stock       │
        │ ├─ Handles StockAdjusted                │
        │ │  └─ UPDATE inventory_stock.quantity   │
        │ └─ Rebuilds idempotently                │
        └─────────────────────────────────────────┘
                ↓
        RESPONSE: HTTP 201
        {
          "id": "550e8400-e29b-41d4-a716-...",
          "events_committed": 1
        }

LATER:  GET /inventory/items/:id
        ↓
        Queries inventory_stock READ MODEL
        (Fast! Pre-computed from events)
```

---

### User Registration Flow (Current - Should Match Above)

```
CLIENT: POST /auth/register
        ├─ username: "alice"
        ├─ email: "alice@example.com"
        ├─ password: "secure123"
        └─ invite_token: null (optional)
                ↓
        ┌────────────────────────────────────────┐
        │ HTTP Handler: register()                │
        │ (crates/api/src/app/routes/auth.rs)    │
        └────────────────────────────────────────┘
                ↓
        1. ✅ Validate input
        2. ✅ Check username not taken
        3. ✅ Hash password with bcrypt
        4. ✅ Create command:
           CreateUser {
             tenant_id,
             user_id (new UUID),
             email,
             display_name,
             initial_roles,
             occurred_at
           }
        5. ✅ Dispatch command (EVENT SOURCING!)
                ↓
        ┌─────────────────────────────────────────┐
        │ services.dispatch::<User>()             │
        │ ├─ Loads aggregate from event store     │
        │ ├─ Applies command                      │
        │ │  └─ Produces: UserCreated event       │
        │ ├─ Appends event to store               │
        │ ├─ Publishes to event bus               │
        │ └─ Returns committed events             │
        └─────────────────────────────────────────┘
                ↓
        ┌─────────────────────────────────────────┐
        │ Event Bus: EventEnvelope published      │
        │ ├─ event_type: "auth.user.created"      │
        │ ├─ tenant_id                            │
        │ ├─ aggregate_id (user_id)               │
        │ └─ payload: UserCreated                 │
        └─────────────────────────────────────────┘
                ↓
        🔄 CURRENT LIMITATION:
        6. ❌ Store credentials in IN-MEMORY store
           (Not in read model like inventory)
        7. ⚠️ Only works until server restart
                ↓
        (SHOULD DO LIKE INVENTORY:)
        ┌─────────────────────────────────────────┐
        │ Projection: UsersProjection             │
        │ ├─ Subscribes to event bus              │
        │ ├─ Handles UserCreated                  │
        │ │  ├─ INSERT into users_read_model      │
        │ │  └─ INSERT into user_credentials      │
        │ ├─ Handles UserSuspended                │
        │ │  └─ UPDATE users_read_model.status    │
        │ └─ Rebuilds idempotently                │
        └─────────────────────────────────────────┘
                ↓
        RESPONSE: HTTP 201
        {
          "token": "eyJ0eXA...",
          "user": { ... },
          "tenant": { ... }
        }

LATER:  POST /auth/login
        ├─ username: "alice"
        ├─ email: "alice@example.com"
        └─ password: "secure123"
                ↓
        (CURRENT - INCORRECT):
        Queries IN-MEMORY auth_stores.credentials
        (Lost after server restart!)
                ↓
        (SHOULD DO LIKE INVENTORY):
        Queries user_credentials READ MODEL
        (Persists in database like inventory_stock)
```

---

## The Pattern: Anatomy of an Event-Sourced Aggregate

### Inventory Module Structure

```
crates/inventory/src/
├── lib.rs              ← Exports ItemId, InventoryItem, Commands, Events
└── item.rs
    ├── InventoryItem (Aggregate Root)
    │   ├── id: InventoryItemId
    │   ├── name: String
    │   ├── stock: i64
    │   ├── version: u64
    │   └── created: bool
    │
    ├── Commands
    │   ├── CreateItem { item_id, name, ... }
    │   └── AdjustStock { item_id, delta, ... }
    │
    ├── Events
    │   ├── ItemCreated { item_id, name, ... }
    │   └── StockAdjusted { item_id, delta, ... }
    │
    ├── impl Aggregate for InventoryItem
    │   ├── fn apply(&mut self, event)
    │   │   ├─ ItemCreated → update id, name
    │   │   └─ StockAdjusted → update stock
    │   │
    │   └── fn validate(&self, cmd) -> Result
    │       ├─ CreateItem → name not empty
    │       └─ AdjustStock → stock >= 0 (invariant!)
    │
    └── impl AggregateRoot for InventoryItem
        └─ id(), version()
```

### User Module Structure (Should Match!)

```
crates/auth/src/
├── lib.rs              ← Exports UserId, User, Commands, Events
├── user.rs             ← THIS FILE (User aggregate)
├── roles.rs
├── claims.rs
└── permissions.rs

user.rs SHOULD contain:

├── User (Aggregate Root)
│   ├── id: UserId
│   ├── tenant_id: TenantId
│   ├── email: String
│   ├── display_name: String
│   ├── status: UserStatus (Active, Suspended)
│   ├── roles: Vec<Role>
│   └── version: u64
│
├── Commands
│   ├── CreateUser { user_id, email, display_name, ... }
│   ├── SuspendUser { user_id }
│   └── ChangePassword { user_id, new_hash }
│
├── Events
│   ├── UserCreated { user_id, email, display_name, ... }
│   ├── UserSuspended { user_id }
│   └── PasswordChanged { user_id }
│
├── impl Aggregate for User
│   ├── fn apply(&mut self, event)
│   │   ├─ UserCreated → update email, display_name, roles
│   │   ├─ UserSuspended → update status
│   │   └─ PasswordChanged → (note: doesn't store hash in aggregate!)
│   │
│   └── fn validate(&self, cmd) -> Result
│       ├─ CreateUser → email not empty
│       ├─ SuspendUser → status is Active
│       └─ ChangePassword → current user is admin
│
└── impl AggregateRoot for User
    └─ id(), version()
```

---

## How Projections Work

### Example: Inventory Projection

```rust
// In crates/infra/src/projections/inventory.rs

pub struct InventoryStockProjection<S> {
    store: Arc<InMemoryTenantStore<InventoryItemId, InventoryReadModel>>,
    bus: Arc<InMemoryEventBus<EventEnvelope<serde_json::Value>>>,
}

impl InventoryStockProjection {
    pub fn new(store, bus) -> Self { ... }
    
    pub async fn subscribe(self: Arc<Self>) {
        // Listen to ALL events on the bus
        let mut rx = bus.subscribe();
        
        while let Ok(envelope) = rx.recv().await {
            // Filter to inventory events only
            match envelope.event_type() {
                "inventory.item.created" => {
                    let evt = serde_json::from_value::<ItemCreated>(...)?;
                    
                    // Build read model
                    let rm = InventoryReadModel {
                        id: evt.item_id,
                        tenant_id: evt.tenant_id,
                        name: evt.name,
                        quantity: 0,
                    };
                    
                    // Store in read model store
                    self.store.insert(evt.tenant_id, evt.item_id, rm);
                }
                "inventory.item.stock_adjusted" => {
                    let evt = serde_json::from_value::<StockAdjusted>(...)?;
                    
                    // Update read model
                    if let Some(mut rm) = self.store.get(evt.tenant_id, &evt.item_id) {
                        rm.quantity += evt.delta;
                        self.store.insert(evt.tenant_id, evt.item_id, rm);
                    }
                }
                _ => {} // Ignore other events
            }
        }
    }
}
```

### What You Need to Implement: Users Projection

```rust
// In crates/infra/src/projections/users.rs (PARTIALLY EXISTS)

pub struct UsersProjection<S> {
    store: Arc<InMemoryTenantStore<UserId, UserReadModel>>,
    // TODO: Add credentials store for user_credentials table
}

impl UsersProjection {
    pub async fn subscribe(self: Arc<Self>) {
        let mut rx = bus.subscribe();
        
        while let Ok(envelope) = rx.recv().await {
            match envelope.event_type() {
                "auth.user.created" => {
                    let evt = serde_json::from_value::<UserCreated>(...)?;
                    
                    // 1. Insert into users_read_model
                    let rm = UserReadModel {
                        id: evt.user_id,
                        tenant_id: evt.tenant_id,
                        email: evt.email,
                        display_name: evt.display_name,
                        status: UserStatus::Active,
                        roles: evt.initial_roles,
                    };
                    self.store.insert(evt.tenant_id, evt.user_id, rm);
                    
                    // 2. INSERT into user_credentials (NEW!)
                    // Need password_hash from... where?
                    // Problem: Not stored in event (for security!)
                    // Solution: Pass via command context or store separately
                }
                "auth.user.suspended" => {
                    // Update users_read_model.status
                }
                _ => {}
            }
        }
    }
}
```

---

## The Key Difference: Where Password Hashes Go

### Inventory: Everything in Event
```
ItemCreated event contains:
├─ item_id
├─ tenant_id
├─ name ← Data stored in event
└─ [price, SKU, description...]
        ↓
InventoryStockProjection
    ├─ Reads event
    ├─ Stores ALL fields in inventory_stock table
    └─ Fast query: SELECT * FROM inventory_stock WHERE id=?
```

### Users: Password Hash is SEPARATE

```
UserCreated event contains:
├─ user_id
├─ tenant_id
├─ email
├─ display_name
├─ roles
└─ [❌ NOT password_hash - too sensitive!]
        ↓
Two separate storage paths:

1. users_read_model:
   ├─ Populated from UserCreated event
   ├─ email, display_name, roles, status
   └─ Used for: user lookup, permissions check

2. user_credentials:
   ├─ Populated from... COMMAND context?
   ├─ username, password_hash, user_id
   └─ Used for: login authentication

KEY INSIGHT:
Password hashes NEVER go in events (never log sensitive data!)
But they DO go in read models (user_credentials table)
So: Need to handle them differently!
```

### Solution: Store Password Hash at Registration Time

```rust
// In register() handler

// 1. Create and dispatch UserCreated event
let cmd = UserCommand::Create(CreateUser { ... });
services.dispatch::<User>(...)?;  // ✅ Event stored

// 2. Separately store credentials (not from event)
let password_hash = bcrypt::hash(&body.password, 12)?;
auth_stores.credentials.insert(StoredCredential {
    username: body.username,
    password_hash,  // ← From request, not event
    user_id,
    tenant_id,
});
// OR in production:
// INSERT INTO user_credentials VALUES (...)
```

---

## Step-by-Step: Implement Production Storage

### Step 1: Run SQL Migrations

Migration file: `docker/migrations/005_create_users_table.sql`

```sql
-- Creates these tables:
CREATE TABLE users_read_model { ... }
CREATE TABLE user_credentials { ... }
```

```bash
docker compose run --rm migrate
```

### Step 2: Update Login to Query Database

```rust
// Before (in-memory):
let creds = auth_stores.credentials.find(username)?;

// After (Postgres):
let creds = sqlx::query_as::<_, StoredCredential>(
    "SELECT * FROM user_credentials WHERE tenant_id=$1 AND username=$2"
)
.bind(tenant_id)
.bind(username)
.fetch_optional(pool)?;
```

### Step 3: Update Register to Store in DB

```rust
// After dispatching event, also persist credentials

let password_hash = bcrypt::hash(&body.password, 12)?;

sqlx::query!(
    "INSERT INTO user_credentials (tenant_id, username, password_hash, user_id) VALUES ($1, $2, $3, $4)",
    tenant_id,
    body.username,
    password_hash,
    user_id,
)
.execute(pool)
.await?;
```

### Step 4: Optional - Populate UsersProjection

For complete event sourcing:

```rust
impl UsersProjection {
    async fn handle_user_created(&self, evt: UserCreated) {
        // 1. Update users_read_model
        sqlx::query!(
            "INSERT INTO users_read_model (...) VALUES (...)",
            evt.tenant_id, evt.user_id, evt.email, ...
        ).execute(pool).await?;
        
        // 2. Password hash comes from event context or stored separately
        // Note: Projection receives ONLY the event, not the plaintext password
    }
}
```

---

## Testing: Verify It Works

### Scenario 1: Single Server Instance

```bash
# Terminal 1: Start server
RUST_LOG=debug cargo run -p forgeerp-api

# Terminal 2: Register
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"a@a.com","password":"pass1"}'

# Terminal 2: Login  ← THIS SHOULD WORK NOW!
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"a@a.com","password":"pass1"}'

Expected: Both return JWT tokens ✅
```

### Scenario 2: After Server Restart

```bash
# Terminal 1: Start server
cargo run -p forgeerp-api

# Register a user
curl -X POST http://localhost:8080/auth/register ...

# Terminal 1: CTRL+C to stop server

# Terminal 1: Start server again
cargo run -p forgeerp-api

# Terminal 2: Try to login
curl -X POST http://localhost:8080/auth/login ...

# Current behavior (in-memory): ❌ FAILS (data lost)
# After migration (Postgres):  ✅ PASSES (data persisted)
```

---

## Summary: Three Phases

| Phase | Storage | Status | Lifetime |
|-------|---------|--------|----------|
| **1: Current** | In-memory Arc<Mutex<>> | ✅ DONE | Per process |
| **2: Production** | Postgres user_credentials | 🔄 TODO | Persistent |
| **3: Full ES** | Event sourcing + Postgres | 🔄 TODO | Completely audit-able |

**You are here → Phase 1 ✅ (Fixed with Arc)**

**Next → Phase 2** (Migrate to Postgres with provided SQL migration)

---

## Code Locations

| Component | File | Status |
|-----------|------|--------|
| User Aggregate | `crates/auth/src/user.rs` | ✅ Exists |
| User Commands/Events | `crates/auth/src/user.rs` | ✅ Exists |
| UserCommand dispatch | `crates/api/src/app/routes/auth.rs` | ✅ Works |
| CredentialStore (in-memory) | `crates/api/src/app/services.rs` | ✅ Fixed with Arc |
| UsersProjection (in-memory) | `crates/infra/src/projections/users.rs` | ⚠️ Partial |
| SQL schema | `docker/migrations/005_create_users_table.sql` | ✅ Created |

**All pieces exist! Just need to wire them together for production.**

