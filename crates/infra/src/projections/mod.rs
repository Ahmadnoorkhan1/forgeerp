//! Projection implementations (read model builders).

pub mod cursor_store;
pub mod inventory_stock;
pub mod parties;
pub mod invoicing;
pub mod accounting;
pub mod products;
pub mod sales_orders;
pub mod purchasing;
pub mod invoices;

pub use cursor_store::{PostgresCursorStore, ProjectionCursorStore};


