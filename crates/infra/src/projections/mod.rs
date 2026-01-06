//! Projection implementations (read model builders).

pub mod cursor_store;
pub mod inventory_stock;
pub mod parties;

pub use cursor_store::{PostgresCursorStore, ProjectionCursorStore};


