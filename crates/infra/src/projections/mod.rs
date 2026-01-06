//! Projection implementations (read model builders).
//!
//! Projections consume domain events and build query-optimized read models.
//! All projections are:
//! - **Rebuildable**: Can be reconstructed from the event stream
//! - **Tenant-isolated**: Data is partitioned by tenant
//! - **Idempotent**: Safe for at-least-once delivery

pub mod cursor_store;

// Domain projections
pub mod inventory_stock;
pub mod parties;
pub mod invoicing;
pub mod accounting;
pub mod products;
pub mod sales_orders;
pub mod purchasing;
pub mod invoices;
pub mod users;

// ERP read models
pub mod customer_balances;
pub mod inventory_valuation;
pub mod open_invoices;

pub use cursor_store::{PostgresCursorStore, ProjectionCursorStore};

// Re-export ERP read models
pub use customer_balances::{CustomerBalance, CustomerBalancesProjection, CustomerBalanceProjectionError};
pub use inventory_valuation::{InventoryValuation, InventoryValuationProjection, InventoryValuationSummary, InventoryValuationError};
pub use open_invoices::{OpenInvoice, OpenInvoicesProjection, OpenInvoicesSummary, OpenInvoicesProjectionError};
pub use users::{default_role_permissions, EffectivePermissions, UserReadModel, UsersProjection};


