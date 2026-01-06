//! Purchasing domain module (Purchase Orders, event-sourced).
//!
//! This crate contains business rules for purchase orders, implemented purely as
//! deterministic domain logic (no IO, no HTTP, no storage).

pub mod order;

pub use order::{
    Approve, CreatePurchaseOrder, GoodsReceived, LineItem, PurchaseOrder, PurchaseOrderApproved,
    PurchaseOrderCommand, PurchaseOrderCreated, PurchaseOrderEvent, PurchaseOrderId,
    PurchaseOrderStatus, ReceiveGoods,
};


