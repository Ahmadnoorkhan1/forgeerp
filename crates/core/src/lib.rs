//! `forgeerp-core` — domain foundation building blocks.
//!
//! This crate contains **pure domain** primitives (no infrastructure concerns).

pub mod aggregate;
pub mod entity;
pub mod error;
pub mod id;
pub mod value_object;

pub use aggregate::AggregateRoot;
pub use entity::Entity;
pub use error::{DomainError, DomainResult};
pub use id::{AggregateId, TenantId, UserId};
pub use value_object::ValueObject;


