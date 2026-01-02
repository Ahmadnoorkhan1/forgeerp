//! `forgeerp-desktop`
//!
//! **Responsibility:** Optional desktop client with offline support.
//!
//! This crate provides:
//! - Local read model caching
//! - Offline read-only mode (API remains the authority; no offline writes)
//! - Explicit sync/reconnect
//!
//! The desktop client is a **thin shell** around the ForgeERP API.

pub mod cache;
pub mod sync;
pub mod offline;

pub use cache::LocalCache;
pub use sync::SyncClient;
pub use offline::{OfflineMode, ConnectivityState};

