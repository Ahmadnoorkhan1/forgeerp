//! Event publishing/subscription abstraction (mechanics only).
//!
//! This module provides the **event bus pattern** - a pub/sub mechanism for distributing
//! events to multiple consumers (projections, handlers, workers, etc.).
//!
//! ## Design Philosophy
//!
//! The event bus is intentionally **lightweight** and makes minimal assumptions:
//!
//! - **Transport-agnostic**: Works with in-memory channels, Redis pub/sub, message queues, etc.
//! - **At-least-once delivery**: Events may be delivered multiple times; consumers must be idempotent
//! - **No ordering guarantees**: Events may arrive out of order (unless implementation provides ordering)
//! - **No persistence**: Bus is for distribution, not storage (event store is source of truth)
//!
//! ## Why At-Least-Once?
//!
//! At-least-once delivery is acceptable because:
//! - **Event sourcing**: Events are stored in the event store first (before publishing)
//! - **Idempotent consumers**: Projections and handlers are designed to handle duplicates
//! - **Simplicity**: At-least-once is easier to implement than exactly-once
//! - **Recovery**: Consumers can reprocess events if needed (event store is source of truth)
//!
//! Consumers must be idempotent - processing the same event multiple times should produce
//! the same result (or be a no-op).

use std::sync::mpsc::Receiver;
use std::time::Duration;
use std::sync::Arc;

/// A subscription to an event stream.
///
/// A subscription provides a way to receive events from an event bus. Each subscription
/// gets a copy of all events published to the bus (broadcast semantics).
///
/// ## Usage Pattern
///
/// ```ignore
/// let bus: Arc<dyn EventBus<EventEnvelope>> = ...;
/// let subscription = bus.subscribe();
///
/// loop {
///     match subscription.recv_timeout(Duration::from_secs(1)) {
///         Ok(event) => process(event)?,
///         Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,  // Check for shutdown
///         Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,  // Bus closed
///     }
/// }
/// ```
///
/// ## Thread Safety
///
/// Subscriptions are designed for single-threaded consumption. Each subscription should
/// be used by one thread (or use a mutex/channel to distribute events to multiple threads).
///
/// ## Message Ordering
///
/// Messages are received in the order they were published by the bus implementation.
/// However, if events are published concurrently, ordering between different publishers
/// is not guaranteed (unless the bus implementation provides ordering guarantees).
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

    /// Block for up to `timeout` waiting for a message.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<M, std::sync::mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

/// Domain-agnostic event bus (pub/sub abstraction).
///
/// An `EventBus` provides a publish-subscribe mechanism for distributing events to multiple
/// consumers. It's the **transport layer** for events after they've been persisted to the
/// event store.
///
/// ## Architecture Role
///
/// The event bus sits between the event store and event consumers:
///
/// ```text
/// Command → Event Store (append events) → Event Bus (publish) → Consumers
///                                                                    ├─ Projections
///                                                                    ├─ Handlers
///                                                                    └─ Workers
/// ```
///
/// Events are **stored first** (in the event store), then **published** (via the bus).
/// This ensures events are never lost - if publication fails, events are still in the store
/// and can be republished.
///
/// ## Design Principles
///
/// - **Lightweight contract**: Minimal interface, no assumptions about implementation
/// - **Transport-agnostic**: Works with in-memory channels, Redis, message queues, etc.
/// - **No storage assumptions**: Bus is for distribution, not persistence (event store handles storage)
/// - **Broadcast semantics**: Each subscriber gets a copy of all published events
///
/// ## Delivery Guarantees
///
/// The bus provides **at-least-once delivery**:
/// - Events may be delivered multiple times (network retries, crashes, etc.)
/// - Events may be delivered out of order (unless implementation provides ordering)
/// - Consumers must be idempotent (handle duplicates safely)
///
/// ## Error Handling
///
/// `publish()` can fail (e.g., bus is full, network error). Failures are surfaced to the
/// caller (typically `CommandDispatcher`) which may retry or handle the error. Since events
/// are already persisted, retrying publication is safe.
///
/// ## Thread Safety
///
/// The trait requires `Send + Sync`, meaning implementations must be safe to share across
/// threads. Multiple threads can publish events concurrently.
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


