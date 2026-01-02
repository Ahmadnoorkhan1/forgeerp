use forgeerp_core::AggregateId;

/// A command targets a specific aggregate.
///
/// Commands are intent; they are not persisted.
/// Multi-tenancy is enforced at the event level (envelopes), not here.
pub trait Command: Clone + core::fmt::Debug + Send + Sync + 'static {
    fn target_aggregate_id(&self) -> AggregateId;
}


