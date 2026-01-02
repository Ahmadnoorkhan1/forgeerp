//! `forgeerp-events` — event sourcing + CQRS primitives.
//!
//! This crate defines **mechanics**, not business logic.
//! Events are intended to be **immutable**, **versioned**, and **append-only**.
//! Multi-tenancy is enforced at the **envelope** level.

pub mod command;
pub mod envelope;
pub mod event;
pub mod handler;
pub mod projection;

pub use command::Command;
pub use envelope::EventEnvelope;
pub use event::Event;
pub use handler::CommandHandler;
pub use projection::Projection;


