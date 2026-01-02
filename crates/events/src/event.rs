use chrono::{DateTime, Utc};

/// A domain-agnostic event trait for event-sourced aggregates.
///
/// This trait defines the minimal interface for domain events in ForgeERP's event-sourced
/// architecture. Events represent **facts that happened** in the domain - they are immutable,
/// versioned, and designed to be append-only.
///
/// ## Event Sourcing Philosophy
///
/// Events are the **source of truth** in event sourcing:
///
/// - **Immutable**: Events represent facts that occurred - they cannot be changed or deleted
/// - **Versioned**: Events have schema versions to support evolution over time
/// - **Append-only**: New events are appended to streams; old events are never modified
/// - **Replayable**: State can be rebuilt by replaying events in order
///
/// This enables powerful capabilities:
/// - **Audit trail**: Complete history of all changes
/// - **Time travel**: Replay events to see state at any point in time
/// - **Schema evolution**: Handle multiple event versions during migrations
/// - **Multiple views**: Different projections can build different views from the same events
///
/// ## Event Versioning
///
/// The `version()` method enables schema evolution. When event schemas change:
///
/// 1. Create a new event variant/version (e.g., `ItemCreatedV2`)
/// 2. Implement both versions in deserialization (backward compatibility)
/// 3. Migrate projections to handle both versions
/// 4. Eventually deprecate old versions
///
/// This allows gradual migrations without downtime.
///
/// ## Design Constraints
///
/// Events must be:
/// - **Serializable**: Events are serialized to JSON for storage/transport
/// - **Cloneable**: Events are copied when building aggregates and projections
/// - **Send + Sync**: Events cross thread boundaries (event bus, projections)
/// - **'static**: Events don't contain borrowed data (must own all data)
///
/// These constraints ensure events can be safely stored, transmitted, and processed in
/// concurrent, distributed systems.
pub trait Event: Clone + core::fmt::Debug + Send + Sync + 'static {
    /// Stable event name/type identifier (e.g., "inventory.item.created").
    ///
    /// This is used for:
    /// - Event routing in the event bus
    /// - Event type filtering in projections
    /// - Schema registry lookups
    /// - Debugging and observability
    ///
    /// The identifier should be:
    /// - **Stable**: Don't change it (breaks deserialization of historical events)
    /// - **Descriptive**: Clearly identify the event type
    /// - **Namespaced**: Use dot notation (e.g., "inventory.item.created") to avoid collisions
    ///
    /// Convention: `{module}.{aggregate}.{action}` (e.g., "inventory.item.stock_adjusted").
    fn event_type(&self) -> &'static str;

    /// Schema version for this event type.
    ///
    /// Version numbers enable schema evolution. When you change an event's structure:
    ///
    /// 1. Increment the version number (e.g., 1 → 2)
    /// 2. Implement deserialization for both versions (backward compatibility)
    /// 3. Migrate projections to handle the new version
    ///
    /// Starting at version 1 is conventional. Versions should only increase (never decrease).
    ///
    /// ## Migration Strategy
    ///
    /// During migrations, you'll have events with multiple versions in the stream:
    /// - Old events: version 1
    /// - New events: version 2
    ///
    /// Code must handle both versions until old events are fully migrated or deprecated.
    fn version(&self) -> u32;

    /// When the event occurred (business time).
    ///
    /// This represents the **domain time** when the event happened (not when it was persisted).
    /// Use this for:
    /// - Business logic that depends on timing
    /// - Time-based queries and analytics
    /// - Audit trails and compliance
    ///
    /// ## Business Time vs System Time
    ///
    /// - **Business time** (`occurred_at`): When the business event happened (from domain)
    /// - **System time**: When the event was persisted (infrastructure concern)
    ///
    /// These can differ in distributed systems (clock skew, retries, etc.). Business time
    /// is what matters for domain logic; system time is for infrastructure (ordering, debugging).
    fn occurred_at(&self) -> DateTime<Utc>;
}


