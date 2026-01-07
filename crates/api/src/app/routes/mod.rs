use axum::{routing::get, Router};

pub mod admin;
pub mod ar;
pub mod common;
pub mod customers;
pub mod event_stream;
pub mod events;
pub mod inventory;
pub mod invoices;
pub mod ledger;
pub mod products;
pub mod purchases;
pub mod rbac;
pub mod replay;
pub mod sales;
pub mod suppliers;
pub mod system;

/// Router for all authenticated (tenant-scoped) endpoints.
pub fn router() -> Router {
    Router::new()
        .route("/whoami", get(system::whoami))
        .route("/stream", get(system::stream))
        .nest("/inventory", inventory::router())
        .nest("/products", products::router())
        .nest("/customers", customers::router())
        .nest("/suppliers", suppliers::router())
        .nest("/sales", sales::router())
        .nest("/invoices", invoices::router())
        .nest("/purchases", purchases::router())
        .nest("/ledger", ledger::router())
        .nest("/ar", ar::router())
        .nest("/admin", admin::router())
        .nest("/admin/rbac", rbac::router())
        .nest("/admin/events", events::router())
        .nest("/admin/replay", replay::router())
        .nest("/admin/stream", event_stream::router())
}


