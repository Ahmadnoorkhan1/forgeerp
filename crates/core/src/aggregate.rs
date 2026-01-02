//! Aggregate root trait for event-sourced (and non-event-sourced) domain models.

use crate::error::{DomainError, DomainResult};

/// Aggregate root marker + minimal interface.
///
/// This is intentionally small so ERP modules can decide how they model state
/// transitions (pure functions, event application, etc.) without bringing in any
/// infrastructure concerns.
pub trait AggregateRoot {
    /// Strongly-typed aggregate identifier.
    type Id: Clone + Eq + core::hash::Hash + core::fmt::Debug;

    /// Returns the aggregate identifier.
    fn id(&self) -> &Self::Id;

    /// Monotonically increasing version of the aggregate's state.
    ///
    /// For event-sourced aggregates, this typically corresponds to the number of
    /// events applied (or the stream revision).
    fn version(&self) -> u64;
}

/// Optimistic concurrency expectation for an aggregate.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// Skip version checking (useful for idempotent commands, migrations, etc.).
    Any,
    /// Require the aggregate to be at an exact version.
    Exact(u64),
}

impl ExpectedVersion {
    pub fn matches(self, actual: u64) -> bool {
        match self {
            ExpectedVersion::Any => true,
            ExpectedVersion::Exact(v) => v == actual,
        }
    }

    pub fn check(self, actual: u64) -> DomainResult<()> {
        if self.matches(actual) {
            Ok(())
        } else {
            Err(DomainError::conflict(format!(
                "optimistic concurrency check failed (expected: {self:?}, actual: {actual})"
            )))
        }
    }
}

/// Aggregate execution semantics (pure, deterministic).
///
/// - **Decision logic**: `handle(&self, cmd)` returns events.
/// - **State mutation**: `apply(&mut self, event)` evolves state.
///
/// Aggregates must not perform IO or side effects. They should only return events
/// describing what happened.
pub trait Aggregate: AggregateRoot {
    type Command: Clone + core::fmt::Debug;
    type Event: Clone + core::fmt::Debug;
    type Error: core::fmt::Debug;

    /// Evolve in-memory state from a single event.
    ///
    /// Implementations should remain deterministic and should typically update
    /// their internal `version()` tracking consistently (e.g. +1 per applied event).
    fn apply(&mut self, event: &Self::Event);

    /// Decide which events to emit given the current state and a command.
    ///
    /// This must not mutate state. State evolution is done through `apply`.
    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error>;
}


