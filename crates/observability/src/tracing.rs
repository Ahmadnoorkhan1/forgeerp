//! Tracing/logging initialization.
//!
//! Minimal stub for now; this can evolve into layered JSON logging, filtering,
//! correlation IDs, etc.

use tracing_subscriber::EnvFilter;

/// Initialize tracing/logging for the process.
///
/// Safe to call multiple times (subsequent calls are no-ops).
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // JSON logs + timestamps, configurable via RUST_LOG.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_timer(tracing_subscriber::fmt::time::SystemTime)
        .with_target(false)
        .try_init();
}


