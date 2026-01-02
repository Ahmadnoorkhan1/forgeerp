use chrono::{DateTime, Utc};

/// A domain-agnostic event.
///
/// Events are:
/// - **immutable** (treat them as facts)
/// - **versioned** (schema evolution)
/// - designed to be **append-only**
pub trait Event: Clone + core::fmt::Debug + Send + Sync + 'static {
    /// Stable event name/type identifier (e.g. "inventory.item.created").
    fn event_type(&self) -> &'static str;

    /// Schema version for this event type.
    fn version(&self) -> u32;

    /// When the event occurred (business time).
    fn occurred_at(&self) -> DateTime<Utc>;
}


