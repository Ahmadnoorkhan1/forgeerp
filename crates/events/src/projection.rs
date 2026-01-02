use crate::{Event, EventEnvelope};

/// A projection builds a read model from an append-only event stream.
///
/// Projections implement the **CQRS read model pattern**: they transform events (write model)
/// into queryable state (read model). This separation enables:
///
/// - **Optimized queries**: Read models can be denormalized for fast queries
/// - **Independent scaling**: Read models can be scaled separately from the write model
/// - **Schema evolution**: Read models can be rebuilt from events when schemas change
/// - **Multiple views**: Different projections can build different views of the same events
///
/// ## Projection Lifecycle
///
/// 1. **Subscribe**: Projection subscribes to events from the event bus (or loads from store)
/// 2. **Apply**: For each event, `apply()` updates the read model
/// 3. **Query**: Read models are queried directly (no event replay needed)
/// 4. **Rebuild**: If needed, projections can be rebuilt from scratch by replaying all events
///
/// ## Idempotency
///
/// Projections must be **idempotent**: applying the same event multiple times should produce
/// the same result. This enables:
/// - **At-least-once delivery**: Events can be delivered multiple times safely
/// - **Replay**: Rebuilding projections by replaying events
/// - **Crash recovery**: Processing events multiple times after a crash is safe
///
/// The `ProjectionRunner` helps with this by tracking sequence numbers and skipping duplicates.
///
/// ## Disposability
///
/// Read models are **disposable**: they can be deleted and rebuilt from events at any time.
/// Events are the source of truth; read models are optimized views. This enables:
/// - **Schema migrations**: Change read model schema, rebuild from events
/// - **Bug fixes**: Fix projection bugs, rebuild read models
/// - **Testing**: Start with empty read models, replay events
///
/// ## Persistence
///
/// This trait doesn't define how read models are stored - that's an infrastructure concern.
/// Implementations might store in:
/// - In-memory HashMap (for tests)
/// - PostgreSQL tables (for production)
/// - Redis (for caching)
/// - Elasticsearch (for search)
///
/// Projections are pure event consumers; persistence is outside this crate.
pub trait Projection {
    type Ev: Event;

    /// Apply a single event to the projection, updating the read model.
    ///
    /// This method processes one event and updates the projection's internal state (read model).
    /// It must be **idempotent**: applying the same event multiple times should produce the same
    /// result (or be a no-op if already processed).
    ///
    /// ## Idempotency Requirements
    ///
    /// Since events can be delivered multiple times (at-least-once delivery), projections must
    /// handle duplicates gracefully. Common strategies:
    ///
    /// - **Sequence number checks**: Skip events with sequence numbers already processed
    /// - **Idempotent operations**: Use upserts, set operations, etc. that are naturally idempotent
    /// - **Event ID deduplication**: Track processed event IDs and skip duplicates
    ///
    /// The `ProjectionRunner` helps by tracking sequence numbers, but projections should still
    /// be designed to be idempotent at the domain level.
    ///
    /// ## Tenant Isolation
    ///
    /// The envelope includes `tenant_id`, which should be used to scope read model updates.
    /// Projections should only update read models for the event's tenant, preventing cross-tenant
    /// data leaks.
    ///
    /// ## Error Handling
    ///
    /// This method doesn't return errors - if an event cannot be processed, the projection should
    /// either:
    /// - Ignore the event (if it's not relevant to this projection)
    /// - Log an error and continue (for resilience)
    /// - Panic (only if the error is unrecoverable and indicates a bug)
    ///
    /// For structured error handling, use `ProjectionRunner::apply()` which returns `ProjectionError`.
    fn apply(&mut self, envelope: &EventEnvelope<Self::Ev>);
}


