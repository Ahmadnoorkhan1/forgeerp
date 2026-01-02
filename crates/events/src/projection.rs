use crate::{Event, EventEnvelope};

/// A projection builds a read model from an append-only event stream.
///
/// Projections are pure event consumers; persistence is outside this crate.
pub trait Projection {
    type Ev: Event;

    /// Apply a single event to the projection.
    fn apply(&mut self, envelope: &EventEnvelope<Self::Ev>);
}


