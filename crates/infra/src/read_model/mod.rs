//! Tenant-isolated read model storage abstractions.

pub mod postgres;
pub mod tenant_store;

pub use postgres::PostgresInventoryStore;
pub use tenant_store::{InMemoryTenantStore, TenantStore};


