//! Entity trait: identity + continuity across state changes.

/// Entity marker + minimal interface.
pub trait Entity {
    /// Strongly-typed entity identifier.
    type Id: Clone + Eq + core::hash::Hash + core::fmt::Debug;

    /// Returns the entity identifier.
    fn id(&self) -> &Self::Id;
}


