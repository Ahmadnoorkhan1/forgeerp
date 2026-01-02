//! Infrastructure event bus implementations.
//!
//! The core event bus abstraction lives in `forgeerp-events` as pure mechanics.
//! This module provides infrastructure-backed implementations (e.g. Redis).

#[cfg(feature = "redis")]
pub mod redis_pubsub;


