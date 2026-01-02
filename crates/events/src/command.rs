use forgeerp_core::AggregateId;

/// A command targets a specific aggregate (command abstraction).
///
/// Commands represent **intent** - a request to perform an action on an aggregate.
/// They are **transient** (not persisted) and are transformed into events (which are persisted).
///
/// ## Command vs Event
///
/// - **Command**: Intent to do something (e.g., "Adjust stock by +10")
/// - **Event**: Fact that something happened (e.g., "StockAdjusted { delta: 10 }")
///
/// Commands are rejected if invalid (validation errors). Events represent accepted changes.
///
/// ## Aggregate Targeting
///
/// Commands must specify which aggregate they target via `target_aggregate_id()`. This enables:
/// - **Routing**: Infrastructure can route commands to the correct aggregate instance
/// - **Isolation**: Each command operates on one aggregate (transaction boundary)
/// - **Concurrency**: Different aggregates can process commands concurrently
///
/// ## Multi-Tenancy
///
/// Multi-tenancy is enforced at the **event level** (envelopes), not at the command level.
/// This keeps commands domain-focused (business logic) while infrastructure handles tenant
/// isolation (enforcement). The tenant context is provided by the infrastructure layer
/// (e.g., from JWT token in HTTP middleware) and attached to events during persistence.
///
/// ## Design Constraints
///
/// Commands must be:
/// - **Cloneable**: Commands may be copied for retries, logging, etc.
/// - **Send + Sync**: Commands cross thread boundaries (workers, async handlers)
/// - **'static**: Commands don't contain borrowed data (must own all data)
///
/// These constraints ensure commands can be safely stored, transmitted, and processed in
/// concurrent, distributed systems.
pub trait Command: Clone + core::fmt::Debug + Send + Sync + 'static {
    fn target_aggregate_id(&self) -> AggregateId;
}


