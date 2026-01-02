//! Value object trait: equality by value, not identity.

/// Marker trait for value objects.
///
/// Value objects should be immutable and compared by their contained values.
pub trait ValueObject: Clone + PartialEq + core::fmt::Debug {}


