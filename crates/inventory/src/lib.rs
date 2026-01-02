//! Inventory domain module (event-sourced).
//!
//! This crate contains business rules for inventory, implemented purely as
//! deterministic domain logic (no IO, no HTTP, no storage).

pub mod item;

pub use item::{
    AdjustStock, CreateItem, InventoryCommand, InventoryEvent, InventoryItem, InventoryItemId,
    ItemCreated, StockAdjusted,
};


