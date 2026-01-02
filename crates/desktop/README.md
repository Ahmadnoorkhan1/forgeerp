# `forgeerp-desktop`

**Responsibility:** Optional desktop client with offline support (Tauri shell around the ForgeERP API).

## Boundaries
- Thin client wrapper around the API (API remains the authority).
- No offline writes (read-only offline mode).
- May depend on `core` and domain modules (for types), but not `infra` or `api`.

## What's implemented (today)

### Local cache
- `LocalCache`: in-memory cache for read models (foundation)
  - Currently supports inventory items
  - Staleness tracking via timestamps
  - Tenant-scoped storage

### Offline mode
- `OfflineMode`: connectivity state management
  - Online/offline detection
  - Enforces read-only behavior when offline
  - API URL configuration

### Sync client
- `SyncClient`: explicit sync/reconnect (requires `tauri` feature)
  - Connectivity checks
  - Per-item sync from API
  - Full sync placeholder (foundation only)

## Features

- **`tauri`** (optional): enables full Tauri desktop app with HTTP client support
  - Requires `reqwest` and `tokio`

## Usage (foundation)

```rust
use forgeerp_desktop::{LocalCache, SyncClient, OfflineMode};

let cache = LocalCache::new();
let mut offline = OfflineMode::new("http://localhost:8080".to_string());

// In a Tauri app with the tauri feature:
#[cfg(feature = "tauri")]
{
    let client = SyncClient::with_token(offline.api_url().to_string(), "bearer-token".to_string());
    // Sync items, check connectivity, etc.
}
```

## Future work

- Full Tauri app setup (frontend + backend integration)
- Persistent storage (IndexedDB or SQLite)
- Full sync implementation (list + batch sync all items)
- Background sync workers
- Write queue for offline writes (future enhancement)

