//! Aggregate root trait for event-sourced (and non-event-sourced) domain models.
//!
//! This module defines the core aggregate lifecycle in ForgeERP's event-sourced architecture.
//! Aggregates are the consistency boundaries of your domain: they enforce business invariants
//! and maintain transactional consistency within a single aggregate instance.
//!
//! ## Aggregate Lifecycle
//!
//! The event-sourced aggregate lifecycle follows a **command-query separation** pattern:
//!
//! 1. **Load**: Events are loaded from the event store and applied to reconstruct current state
//! 2. **Decide**: A command is handled, producing zero or more events (no state mutation)
//! 3. **Evolve**: Events are persisted and then applied to update the aggregate's version
//!
//! This separation ensures:
//! - **Determinism**: Same events always produce the same state (`apply` is pure)
//! - **Testability**: Commands can be tested without side effects (`handle` doesn't mutate)
//! - **Replayability**: State can be rebuilt from events at any time
//!
//! ## Why Two Traits?
//!
//! `AggregateRoot` is intentionally minimal (just identity + version tracking) to support
//! both event-sourced and traditional aggregates. `Aggregate` adds event-sourcing semantics
//! (`handle` + `apply`) for aggregates that need the full lifecycle.

use crate::error::{DomainError, DomainResult};

/// Aggregate root marker + minimal interface.
///
/// This trait identifies a domain aggregate and provides the minimal metadata needed
/// for optimistic concurrency control and identity management.
///
/// ## Design Rationale
///
/// This is intentionally small so ERP modules can decide how they model state transitions
/// (pure functions, event application, etc.) without bringing in infrastructure concerns.
/// The `version()` method enables optimistic concurrency control at the infrastructure layer
/// without requiring aggregates to know about event stores or persistence mechanisms.
///
/// ## Version Semantics
///
/// The version represents the aggregate's current state revision. For event-sourced aggregates,
/// this typically equals the number of events applied (or the stream's sequence number).
/// The infrastructure layer uses this to:
/// - Detect concurrent modifications (optimistic locking)
/// - Validate event ordering (monotonic sequence numbers)
/// - Enable idempotent command processing
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
///
/// This enum controls how the event store validates aggregate versions during append operations.
/// Optimistic concurrency control prevents lost updates in distributed systems without requiring
/// pessimistic locks (which hurt performance and availability).
///
/// ## Why Optimistic Concurrency?
///
/// Event sourcing uses optimistic locking because:
/// - **Performance**: No locks means high throughput (especially for append-only operations)
/// - **Availability**: Reads never block, only concurrent writes to the same aggregate conflict
/// - **Scalability**: Multiple aggregates can be modified concurrently without contention
///
/// Conflicts are rare in practice because:
/// - Users typically modify different aggregates
/// - UI retries handle transient conflicts gracefully
/// - Commands are idempotent (same command produces same events)
///
/// ## Usage Patterns
///
/// - `ExpectedVersion::Exact(n)`: Normal command execution - ensures no concurrent modifications
/// - `ExpectedVersion::Any`: Idempotent operations, migrations, or when you want to overwrite
///
/// ## Failure Semantics
///
/// When a version mismatch occurs, the operation fails with `DomainError::Conflict`. The caller
/// should reload the aggregate and retry the command (or surface a conflict error to the user).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// Skip version checking (useful for idempotent commands, migrations, etc.).
    ///
    /// Use this when you want to allow overwrites or when idempotency guarantees make
    /// version conflicts harmless (e.g., idempotent command handlers that check "already done").
    Any,
    /// Require the aggregate to be at an exact version.
    ///
    /// This is the default for normal command execution. If the aggregate's version doesn't
    /// match, the append fails with a concurrency conflict, indicating another process
    /// modified the aggregate concurrently.
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

/// Aggregate execution semantics for event-sourced aggregates (pure, deterministic).
///
/// This trait defines the **command-query separation** pattern for event-sourced aggregates:
/// - **Decision logic** (`handle`): Pure function that decides which events to emit given current state + command
/// - **State evolution** (`apply`): Pure function that evolves state deterministically from events
///
/// ## Why Separate `handle` and `apply`?
///
/// This separation is the foundation of event sourcing's benefits:
///
/// 1. **Determinism**: `apply` is a pure function - same events always produce the same state.
///    This enables replay, testing, and debugging by replaying events.
///
/// 2. **Testability**: `handle` doesn't mutate state, so you can call it multiple times with
///    the same input and get the same output. This makes command logic easy to test.
///
/// 3. **Event Sourcing**: By separating decision from mutation, we can:
///    - Store events (decisions) instead of state
///    - Rebuild state by replaying events
///    - Query state from multiple projections
///    - Audit all changes (events are the audit log)
///
/// ## Lifecycle Contract
///
/// The aggregate lifecycle in ForgeERP follows this pattern:
///
/// ```ignore
/// // 1. Load events from store and rehydrate state
/// let mut aggregate = make_aggregate(tenant_id, id);
/// for event in history {
///     aggregate.apply(&event);  // Rebuild current state
/// }
///
/// // 2. Handle command (pure, no mutation)
/// let events = aggregate.handle(&command)?;  // Decide what should happen
///
/// // 3. Persist events to store (infrastructure responsibility)
/// store.append(events, ExpectedVersion::Exact(aggregate.version()))?;
///
/// // 4. Apply events to update version (for next operation)
/// for event in &events {
///     aggregate.apply(event);  // Update version tracking
/// }
/// ```
///
/// ## Implementation Requirements
///
/// - **No IO**: Aggregates must not perform network calls, database queries, or file I/O.
///   All external data should be passed via commands or loaded before command handling.
///
/// - **No side effects**: `handle` should only return events, never mutate state or trigger
///   external actions. Side effects happen in infrastructure (event handlers, projections).
///
/// - **Determinism**: `apply` must be deterministic - same event sequence always produces
///   the same final state. No random numbers, timestamps (except from event), or external state.
///
/// - **Version tracking**: Implementations should increment `version()` in `apply` to track
///   the number of events applied (typically `version += 1` per event).
pub trait Aggregate: AggregateRoot {
    type Command: Clone + core::fmt::Debug;
    type Event: Clone + core::fmt::Debug;
    type Error: core::fmt::Debug;

    /// Evolve in-memory state from a single event.
    ///
    /// This method mutates the aggregate's state based on an event. It must be **deterministic**:
    /// applying the same sequence of events to the same initial state must always produce the
    /// same final state.
    ///
    /// ## Determinism Requirements
    ///
    /// - No random number generation
    /// - No system timestamps (use `event.occurred_at()` if needed)
    /// - No external state lookups (all data must come from the event or current state)
    /// - Same events in same order → same final state
    ///
    /// ## Version Tracking
    ///
    /// Implementations should update the internal version counter (typically increment by 1)
    /// to track how many events have been applied. The infrastructure layer uses this for
    /// optimistic concurrency control.
    ///
    /// ## Usage Pattern
    ///
    /// This is called in two contexts:
    /// 1. **Rehydration**: Loading historical events to rebuild current state
    /// 2. **State update**: After persisting new events, updating the aggregate for the next command
    ///
    /// Both contexts require the same deterministic behavior.
    fn apply(&mut self, event: &Self::Event);

    /// Decide which events to emit given the current state and a command.
    ///
    /// This method implements the aggregate's business logic: it validates the command,
    /// checks invariants, and decides what events should be emitted. **It must not mutate state.**
    ///
    /// ## Purity Contract
    ///
    /// This method is pure (no side effects):
    /// - Takes `&self` (immutable reference)
    /// - Returns events (doesn't mutate aggregate)
    /// - Can be called multiple times with same input → same output
    ///
    /// This enables:
    /// - **Easy testing**: Call `handle` multiple times, verify events match
    /// - **Idempotency**: Retrying commands produces the same events (if state unchanged)
    /// - **Event sourcing**: Events represent decisions, not state mutations
    ///
    /// ## Business Logic Responsibilities
    ///
    /// - **Validation**: Reject invalid commands (e.g., negative quantities)
    /// - **Invariant checking**: Enforce business rules (e.g., stock cannot go negative)
    /// - **Event generation**: Emit events that describe what happened
    ///
    /// ## Return Semantics
    ///
    /// - `Ok(events)`: Command accepted, return events to persist
    /// - `Ok(vec![])`: Command accepted but no events needed (idempotent no-op)
    /// - `Err(error)`: Command rejected (validation error, invariant violation, etc.)
    ///
    /// ## State Mutation
    ///
    /// State evolution happens in `apply()`, not here. The infrastructure layer will:
    /// 1. Call `handle()` to get events
    /// 2. Persist events to the event store
    /// 3. Call `apply()` for each persisted event to update state
    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AggregateId;

    // Test aggregate for verifying aggregate semantics
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestAggregate {
        id: AggregateId,
        value: i32,
        version: u64,
    }

    impl AggregateRoot for TestAggregate {
        type Id = AggregateId;

        fn id(&self) -> &Self::Id {
            &self.id
        }

        fn version(&self) -> u64 {
            self.version
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestCommand {
        Increment(i32),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestEvent {
        ValueChanged(i32),
    }

    impl Aggregate for TestAggregate {
        type Command = TestCommand;
        type Event = TestEvent;
        type Error = DomainError;

        fn apply(&mut self, event: &Self::Event) {
            match event {
                TestEvent::ValueChanged(delta) => {
                    self.value += delta;
                }
            }
            self.version += 1;
        }

        fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
            match command {
                TestCommand::Increment(amount) => {
                    if *amount <= 0 {
                        return Err(DomainError::validation("amount must be positive"));
                    }
                    Ok(vec![TestEvent::ValueChanged(*amount)])
                }
            }
        }
    }

    impl TestAggregate {
        fn new(id: AggregateId) -> Self {
            Self {
                id,
                value: 0,
                version: 0,
            }
        }

        fn value(&self) -> i32 {
            self.value
        }
    }

    #[test]
    fn handle_does_not_mutate_state() {
        let agg = TestAggregate::new(AggregateId::new());
        let initial_value = agg.value();
        let initial_version = agg.version();

        // Call handle multiple times
        let cmd = TestCommand::Increment(5);
        let events1 = agg.handle(&cmd).unwrap();
        let value_after_handle1 = agg.value();
        let version_after_handle1 = agg.version();

        let events2 = agg.handle(&cmd).unwrap();
        let value_after_handle2 = agg.value();
        let version_after_handle2 = agg.version();

        // State should remain unchanged
        assert_eq!(value_after_handle1, initial_value);
        assert_eq!(value_after_handle2, initial_value);
        assert_eq!(version_after_handle1, initial_version);
        assert_eq!(version_after_handle2, initial_version);

        // Events should be identical
        assert_eq!(events1, events2);
    }

    #[test]
    fn apply_is_deterministic() {
        let id = AggregateId::new();
        let event1 = TestEvent::ValueChanged(10);
        let event2 = TestEvent::ValueChanged(5);
        let event3 = TestEvent::ValueChanged(-3);

        // Apply same events to two separate aggregates
        let mut agg1 = TestAggregate::new(id);
        agg1.apply(&event1);
        agg1.apply(&event2);
        agg1.apply(&event3);

        let mut agg2 = TestAggregate::new(id);
        agg2.apply(&event1);
        agg2.apply(&event2);
        agg2.apply(&event3);

        // Both should be in identical state
        assert_eq!(agg1.value(), agg2.value());
        assert_eq!(agg1.version(), agg2.version());
        assert_eq!(agg1.value(), 12); // 10 + 5 - 3
        assert_eq!(agg1.version(), 3);
    }

    #[test]
    fn apply_updates_state() {
        let mut agg = TestAggregate::new(AggregateId::new());
        assert_eq!(agg.value(), 0);
        assert_eq!(agg.version(), 0);

        let event = TestEvent::ValueChanged(7);
        agg.apply(&event);

        assert_eq!(agg.value(), 7);
        assert_eq!(agg.version(), 1);
    }

    #[test]
    fn expected_version_any_always_matches() {
        assert!(ExpectedVersion::Any.matches(0));
        assert!(ExpectedVersion::Any.matches(42));
        assert!(ExpectedVersion::Any.matches(u64::MAX));
    }

    #[test]
    fn expected_version_exact_matches_correctly() {
        assert!(ExpectedVersion::Exact(5).matches(5));
        assert!(!ExpectedVersion::Exact(5).matches(4));
        assert!(!ExpectedVersion::Exact(5).matches(6));
    }

    #[test]
    fn expected_version_check_succeeds_on_match() {
        assert!(ExpectedVersion::Exact(10).check(10).is_ok());
        assert!(ExpectedVersion::Any.check(999).is_ok());
    }

    #[test]
    fn expected_version_check_fails_on_mismatch() {
        let err = ExpectedVersion::Exact(10).check(11).unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for version mismatch"),
        }
    }
}
