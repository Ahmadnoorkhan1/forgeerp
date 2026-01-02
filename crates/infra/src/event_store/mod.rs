//! Append-only event store boundary.
//!
//! This module defines an infrastructure-facing abstraction for storing and
//! loading tenant-scoped event streams without making any storage assumptions.

pub mod in_memory;
pub mod r#trait;

pub use in_memory::InMemoryEventStore;
pub use r#trait::{EventStore, EventStoreError, StoredEvent, UncommittedEvent};


