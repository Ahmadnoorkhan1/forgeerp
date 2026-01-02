//! Tenant-isolated read model storage abstractions.

pub mod tenant_store;

pub use tenant_store::{InMemoryTenantStore, TenantStore};


