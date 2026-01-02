//! Event publishing/subscription abstraction (mechanics only).
//!
//! At-least-once delivery is acceptable; consumers must be idempotent.

use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// A subscription to an event stream.
#[derive(Debug)]
pub struct Subscription<M> {
    receiver: Receiver<M>,
}

impl<M> Subscription<M> {
    pub fn new(receiver: Receiver<M>) -> Self {
        Self { receiver }
    }

    /// Block until the next message is available.
    pub fn recv(&self) -> Result<M, std::sync::mpsc::RecvError> {
        self.receiver.recv()
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&self) -> Result<M, std::sync::mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

/// Domain-agnostic event bus.
///
/// This is a lightweight pub/sub contract. It makes no assumptions about
/// storage, transport, or threading model.
pub trait EventBus<M>: Send + Sync {
    type Error: core::fmt::Debug + Send + Sync + 'static;

    fn publish(&self, message: M) -> Result<(), Self::Error>;

    fn subscribe(&self) -> Subscription<M>;
}

impl<M, B> EventBus<M> for Arc<B>
where
    B: EventBus<M> + ?Sized,
{
    type Error = B::Error;

    fn publish(&self, message: M) -> Result<(), Self::Error> {
        (**self).publish(message)
    }

    fn subscribe(&self) -> Subscription<M> {
        (**self).subscribe()
    }
}


