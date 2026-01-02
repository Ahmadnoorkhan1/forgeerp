//! Infrastructure layer: DB, Redis, config, external services.

pub mod event_store;

/// Database adapters (connection pools, repositories, migrations wiring).
pub mod db {}

/// Redis adapters (caching, distributed locks, pub/sub wiring).
pub mod redis {}

/// Configuration loading and representation.
pub mod config {}

/// External service clients/adapters.
pub mod external {}


