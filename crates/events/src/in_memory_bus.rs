//! In-memory event bus for tests/dev.

use std::sync::{Mutex, mpsc};

use crate::bus::{EventBus, Subscription};

#[derive(Debug)]
pub enum InMemoryBusError {
    /// Publish failed due to internal lock poisoning.
    Poisoned,
}

/// In-memory pub/sub bus.
///
/// - No IO / no async
/// - Best-effort fan-out
/// - At-least-once acceptable (subscribers must be idempotent)
#[derive(Debug)]
pub struct InMemoryEventBus<M> {
    subscribers: Mutex<Vec<mpsc::Sender<M>>>,
}

impl<M> InMemoryEventBus<M> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<M> Default for InMemoryEventBus<M> {
    fn default() -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
        }
    }
}

impl<M> EventBus<M> for InMemoryEventBus<M>
where
    M: Clone + Send + 'static,
{
    type Error = InMemoryBusError;

    fn publish(&self, message: M) -> Result<(), Self::Error> {
        let mut subs = self.subscribers.lock().map_err(|_| InMemoryBusError::Poisoned)?;

        // Drop any dead subscribers while publishing.
        subs.retain(|tx| tx.send(message.clone()).is_ok());

        Ok(())
    }

    fn subscribe(&self) -> Subscription<M> {
        let (tx, rx) = mpsc::channel();

        // If the lock is poisoned, we still return a subscription;
        // it just won't receive messages until the process restarts.
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.push(tx);
        }

        Subscription::new(rx)
    }
}


