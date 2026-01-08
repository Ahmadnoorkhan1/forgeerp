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

// Shared types (WASM-compatible, no backend dependencies)
pub mod types;

// Backend modules (require tokio/sqlx, excluded for WASM)
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod sync;
#[cfg(not(target_arch = "wasm32"))]
pub mod offline;
#[cfg(not(target_arch = "wasm32"))]
pub mod command_queue;
#[cfg(all(feature = "tauri", not(target_arch = "wasm32")))]
pub mod sync_manager;
#[cfg(all(feature = "tauri", not(target_arch = "wasm32")))]
pub mod commands;
#[cfg(all(feature = "tauri", not(target_arch = "wasm32")))]
pub mod sync_worker;

// Frontend module (WASM-only)
#[cfg(target_arch = "wasm32")]
pub mod frontend;

// Re-export shared types
pub use types::*;

// Re-export backend types (only for non-WASM)
#[cfg(not(target_arch = "wasm32"))]
pub use cache::LocalCache;
#[cfg(not(target_arch = "wasm32"))]
pub use sync::SyncClient;
#[cfg(not(target_arch = "wasm32"))]
pub use offline::OfflineMode;
#[cfg(not(target_arch = "wasm32"))]
pub use command_queue::CommandQueue;
#[cfg(all(feature = "tauri", not(target_arch = "wasm32")))]
pub use sync_manager::{SyncManager, SyncError};

