use crate::{Command, Event};

/// Handles a command and emits events.
///
/// This trait is intentionally generic and makes **no storage assumptions**.
/// Errors are domain-specific; therefore the error type is associated.
pub trait CommandHandler {
    type Cmd: Command;
    type Ev: Event;
    type Error: core::fmt::Debug + Send + Sync + 'static;

    fn handle(&self, command: Self::Cmd) -> Result<Vec<Self::Ev>, Self::Error>;
}

/// Execute an aggregate command deterministically (no IO, no async).
///
/// This provides the canonical event-sourced lifecycle:
/// 1) **Decide** events with `handle(&self, cmd)` (no mutation)
/// 2) **Evolve** state by applying each event via `apply(&mut self, ev)`
///
/// The aggregate is responsible for maintaining its own version tracking
/// consistently during `apply`.
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


