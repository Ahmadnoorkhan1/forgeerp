use crate::{Command, Event};

/// Handles a command and emits events (command handler abstraction).
///
/// This trait provides a generic interface for command handling that's independent of
/// the aggregate lifecycle. It's useful for:
///
/// - **Workers**: Background processes that handle commands asynchronously
/// - **Testing**: Simple command handlers for integration tests
/// - **Alternative patterns**: Command handlers that don't use the full aggregate lifecycle
///
/// ## Relationship to Aggregate Trait
///
/// The `Aggregate` trait provides `handle()` which is similar, but integrated with the
/// full event-sourcing lifecycle (version tracking, state management). `CommandHandler`
/// is a simpler, standalone interface for command → events transformation.
///
/// ## Design Philosophy
///
/// This trait is intentionally generic and makes **no storage assumptions**. Errors are
/// domain-specific; therefore the error type is associated. This keeps the trait flexible
/// for different use cases.
pub trait CommandHandler {
    type Cmd: Command;
    type Ev: Event;
    type Error: core::fmt::Debug + Send + Sync + 'static;

    fn handle(&self, command: Self::Cmd) -> Result<Vec<Self::Ev>, Self::Error>;
}

/// Execute an aggregate command deterministically (no IO, no async).
///
/// This function provides a **simple execution pattern** for event-sourced aggregates
/// that combines decision and state evolution in one step. It's useful for:
///
/// - **Testing**: Simple command execution without the full `CommandDispatcher` pipeline
/// - **Inline processing**: Commands that don't need persistence or publication
/// - **Learning**: Understanding the core aggregate lifecycle
///
/// ## Execution Pattern
///
/// This function provides the canonical event-sourced lifecycle:
///
/// 1. **Decide**: Calls `aggregate.handle(command)` to get events (pure, no mutation)
/// 2. **Evolve**: Applies each event to the aggregate via `aggregate.apply(event)`
///
/// Note: This function mutates the aggregate in place. For the full event-sourcing
/// pipeline (with persistence and publication), use `CommandDispatcher::dispatch()`.
///
/// ## Version Tracking
///
/// The aggregate is responsible for maintaining its own version tracking consistently
/// during `apply()`. Typically, each call to `apply()` increments the version by 1.
///
/// ## When to Use
///
/// - **Testing**: Unit tests that don't need event store/bus
/// - **Simple workflows**: Inline command processing without persistence
/// - **Alternative patterns**: Command handlers that manage state directly
///
/// For production use cases, prefer `CommandDispatcher::dispatch()` which provides
/// persistence, publication, tenant isolation, and optimistic concurrency control.
pub fn execute<A>(
    aggregate: &mut A,
    command: &A::Command,
) -> Result<Vec<A::Event>, A::Error>
where
    A: forgeerp_core::Aggregate,
{
    let events = A::handle(aggregate, command)?;
    for ev in &events {
        A::apply(aggregate, ev);
    }
    Ok(events)
}


