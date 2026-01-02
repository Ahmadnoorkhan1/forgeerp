//! Infrastructure layer: DB, Redis, config, external services.

pub mod event_bus;
pub mod event_store;
pub mod command_dispatcher;
pub mod read_model;
pub mod projections;

/// Database adapters (connection pools, repositories, migrations wiring).
pub mod db {}

/// Redis adapters (caching, distributed locks, pub/sub wiring).
pub mod redis {}

/// Configuration loading and representation.
pub mod config {}

/// External service clients/adapters.
pub mod external {}


