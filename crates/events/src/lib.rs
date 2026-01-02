//! `forgeerp-events` — event sourcing + CQRS primitives.
//!
//! This crate defines **mechanics**, not business logic.
//! Events are intended to be **immutable**, **versioned**, and **append-only**.
//! Multi-tenancy is enforced at the **envelope** level.

pub mod command;
pub mod bus;
pub mod envelope;
pub mod event;
pub mod handler;
pub mod in_memory_bus;
pub mod projection;
pub mod runner;

pub use bus::{EventBus, Subscription};
pub use command::Command;
pub use envelope::EventEnvelope;
pub use event::Event;
pub use handler::CommandHandler;
pub use in_memory_bus::InMemoryEventBus;
pub use projection::Projection;
pub use runner::{ProjectionCursor, ProjectionError, ProjectionRunner};


