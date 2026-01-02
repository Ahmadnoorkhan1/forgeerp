//! Entity trait: identity + continuity across state changes.
//!
//! Entities are domain objects that have **identity** - they are distinguished by their
//! ID, not by their attribute values. Two entities with the same ID are the same entity,
//! even if their attributes differ.

/// Entity marker + minimal interface.
///
/// Entities are domain objects that have a **stable identity** - they are identified by
/// their ID, not by their attribute values. This is the foundation of DDD's Entity pattern.
///
/// ## Entity vs Value Object
///
/// - **Entity**: Has identity (two entities with same ID are the same entity, even if attributes differ)
/// - **Value Object**: Has no identity (two value objects with same values are equal)
///
/// Example:
/// - `Customer` is an entity (identified by `CustomerId`)
/// - `Money` is a value object (two `Money { amount: 100, currency: "USD" }` are equal)
///
/// ## Identity Continuity
///
/// Entities maintain their identity across state changes. An entity's ID doesn't change
/// when its attributes change. This enables:
/// - **State tracking**: Track the same entity over time
/// - **Aggregate roots**: Entities can be aggregate roots (consistency boundaries)
/// - **Event sourcing**: Events reference entities by ID
///
/// ## Relationship to AggregateRoot
///
/// `AggregateRoot` extends `Entity` with version tracking for event sourcing. All aggregate
/// roots are entities, but not all entities are aggregate roots (some entities are part of
/// an aggregate, not the root).
pub trait Entity {
    /// Strongly-typed entity identifier.
    type Id: Clone + Eq + core::hash::Hash + core::fmt::Debug;

    /// Returns the entity identifier.
    fn id(&self) -> &Self::Id;
}


