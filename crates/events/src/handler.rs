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


