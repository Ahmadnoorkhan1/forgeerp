//! Tracing, logging, metrics (shared setup).

/// Initialize process-wide observability (tracing/logging).
///
/// This is safe to call multiple times; subsequent calls become no-ops.
pub fn init() {
    tracing::init();
}

/// Tracing configuration (filters, layers).
pub mod tracing;

/// Logging configuration.
pub mod logging {}

/// Metrics setup and exporters.
pub mod metrics {}


