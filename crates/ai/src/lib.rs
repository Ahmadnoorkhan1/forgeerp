//! `forgeerp-ai`
//!
//! **Responsibility:** Optional AI/ML subsystem boundary.
//!
//! This crate is intentionally **not** part of the domain model:
//! - It must not depend on ERP aggregates (Inventory/Sales/etc).
//! - It must not mutate domain state.
//! - It emits **AI insights/results**, not domain events.

pub mod job;
pub mod result;
pub mod scheduler;

pub use job::AiJob;
pub use result::{AiError, AiResult};
pub use scheduler::{
    AiScheduler, InventoryItemSnapshot, InventorySnapshot, LocalAiScheduler, ReadModelReader, TenantScope,
};


