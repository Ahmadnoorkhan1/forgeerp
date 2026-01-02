//! Aggregate root trait for event-sourced (and non-event-sourced) domain models.

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


