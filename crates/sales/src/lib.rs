//! Sales Orders domain module (event-sourced).
//!
//! This crate contains business rules for sales orders, implemented purely as
//! deterministic domain logic (no IO, no HTTP, no storage).

pub mod order;

pub use order::{
    AddLine, ConfirmOrder, CreateSalesOrder, LineAdded, MarkInvoiced, OrderConfirmed, SalesOrder,
    SalesOrderCommand, SalesOrderCreated, SalesOrderEvent, SalesOrderId, SalesOrderStatus,
};


