# Registration & User Management

Covers the user registration flow, frontend components, API endpoints, and user management features.

## Table of Contents

1. [Registration Flow](#registration-flow)
2. [Frontend Components](#frontend-components)
3. [API Endpoints](#api-endpoints)
4. [Data Structures](#data-structures)
5. [Validation Rules](#validation-rules)
6. [User Management](#user-management)
7. [Testing](#testing)
8. [Implementation Status](#implementation-status)

---

## Registration Flow

```
User navigates to /register
  ↓
Registration form displayed
  ↓
User fills form and submits
  ↓
Frontend validates all fields
  ↓
Valid? → POST /auth/register → Backend creates user → Event sourced (UserCreated)
  ↓                                                    ↓
Success → Show message → Redirect to /login          Store credentials (in-memory or Postgres)
  ↓                                                    ↓
Error → Display error alert                          Return JWT token
```

### Backend Flow (Event Sourcing)

Registration follows the same event-sourced pattern as inventory items:

```
POST /auth/register
  ├─ Validate input
  ├─ Hash password (bcrypt)
  ├─ Check username availability
  ├─ Create UserCommand::Create { tenant_id, user_id, email, display_name, roles }
  ├─ Dispatch command → UserCreated event → Event store → Event bus
  ├─ Store credentials (username → password_hash mapping)
  └─ Return JWT token + user + tenant info
```

> [!NOTE]
> Password hashes are stored **separately** from events (never in event payloads) for security. Credentials are inserted directly by the registration handler, not through event projections.

---

## Frontend Components

### Registration Page (`/register`)

**File**: `crates/frontend/src/register.rs`

A glass-morphism styled registration form with:

- **Full Name** input (2+ characters)
- **Email** input (valid format validation)
- **Password** input (8+ chars, 1 uppercase, 1 number) with visibility toggle
- **Confirm Password** input (must match)
- Real-time field-level validation
- Loading state during submission
- Success notification with auto-redirect to `/login`
- Error alerts with details

### User List Page (`/users`)

**File**: `crates/frontend/src/users.rs`

A dashboard-style user management table with:

| Column | Description |
|---|---|
| Email | User's email address |
| Full Name | Display name |
| Status | Active/Inactive indicator |
| Created | Account creation date |
| Actions | "View Details" button |

**Features**:
- Real-time search (by email or name)
- User count display
- Details modal (click "View Details")
- "+ Add New User" button → navigates to `/register`

---

## API Endpoints

### Register User

```
POST /api/auth/register
Content-Type: application/json

{
  "email": "user@example.com",
  "full_name": "John Doe",
  "password": "SecurePass123",
  "password_confirm": "SecurePass123"
}

→ 201 Created
{
  "user_id": "uuid",
  "email": "user@example.com",
  "full_name": "John Doe"
}

Errors: 400 (validation), 409 (duplicate email), 500 (server)
```

### List Users

```
GET /api/users
Authorization: Bearer {token}

→ 200 OK
[
  {
    "user_id": "uuid",
    "email": "user@example.com",
    "full_name": "John Doe",
    "created_at": "2026-02-18T10:30:00Z",
    "is_active": true
  }
]

Errors: 401 (no token), 403 (insufficient permissions), 500 (server)
```

### Get User by ID

```
GET /api/users/{user_id}
Authorization: Bearer {token}

→ 200 OK  (same format as list item)

Errors: 404 (not found), 401 (no token), 500 (server)
```

---

## Data Structures

```rust
// Request
pub struct RegisterRequest {
    pub email: String,
    pub full_name: String,
    pub password: String,
    pub password_confirm: String,
}

// Response
pub struct RegisterResponse {
    pub user_id: String,
    pub email: String,
    pub full_name: String,
}

// User info (list/detail)
pub struct UserInfo {
    pub user_id: String,
    pub email: String,
    pub full_name: String,
    pub created_at: String,
    pub is_active: bool,
}
```

---

## Validation Rules

| Field | Rule |
|---|---|
| Email | Required, must contain `@` and `.` |
| Full Name | Required, 2+ characters |
| Password | Required, 8+ characters, ≥1 uppercase, ≥1 number |
| Confirm Password | Required, must match password |

**Error messages**:
- `❌ Email is required` / `Please enter a valid email`
- `❌ Full name must be at least 2 characters`
- `❌ Password must be at least 8 characters`
- `❌ Password must contain at least one uppercase letter`
- `❌ Password must contain at least one number`
- `❌ Passwords do not match`

---

## User Management

### Navigation

```
/login     → Login page
  ↓ "Sign up" link
/register  → Registration page
  ↓ Success
/login     → Login page (redirected)
  ↓ After login
/users     → User list page
  ↓ "+ Add New User"
/register  → Registration page
```

### Component Tree

```
App (Router)
├── Login
├── Register
│   ├── Full Name Input
│   ├── Email Input
│   ├── Password Input (with toggle)
│   ├── Confirm Password Input
│   └── Submit Button
└── UsersList
    ├── Search Bar
    ├── User Table
    └── User Details Modal
```

---

## Testing

### Registration Tests

```bash
# 1. Valid registration
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"alice@example.com","password":"SecurePass123"}'
# Expected: HTTP 201 with token

# 2. Login with registered user
curl -X POST http://localhost:8080/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"alice@example.com","password":"SecurePass123"}'
# Expected: HTTP 200 with token
```

### Frontend Tests

1. Navigate to `/register` → form should display
2. Try invalid email format → see validation error
3. Try short password (`test`) → see "at least 8 characters"
4. Try mismatched passwords → see "do not match"
5. Fill valid data → submit → see success message → redirect to `/login`
6. Navigate to `/users` → table loads → search works → "View Details" shows modal

---

## Implementation Status

### Frontend ✅ Complete

- [x] Registration component with full validation
- [x] User list component with search
- [x] User details modal
- [x] API functions (`register()`, `list_users()`, `get_user()`)
- [x] Routes configured (`/register`, `/users`)
- [x] Module exports in `lib.rs`
- [x] CSS styling and responsive design

### Backend ⏳ In Progress

- [ ] `POST /api/auth/register` endpoint
- [ ] `GET /api/users` endpoint
- [ ] `GET /api/users/{user_id}` endpoint
- [ ] Password hashing (bcrypt)
- [ ] Email uniqueness constraint
- [ ] Duplicate checking

### Files

| File | Location | Status |
|---|---|---|
| `register.rs` | `crates/frontend/src/` | ✅ Created |
| `users.rs` | `crates/frontend/src/` | ✅ Created |
| `api.rs` | `crates/frontend/src/` | ✅ Updated |
| `app.rs` | `crates/frontend/src/` | ✅ Updated |
| `lib.rs` | `crates/frontend/src/` | ✅ Updated |

---

## Related Documentation

- [AUTHENTICATION.md](AUTHENTICATION.md) — Auth system, Arc fix, JWT tokens
- [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) — Database setup for production
- [TESTING.md](TESTING.md) — Comprehensive test suite
- [docs/INVENTORY_VS_USERS_COMPARISON.md](docs/INVENTORY_VS_USERS_COMPARISON.md) — Event sourcing pattern comparison
