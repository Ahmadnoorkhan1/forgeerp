//! Value object trait: equality by value, not identity.
//!
//! Value objects are domain objects that have **no identity** - they are defined entirely
//! by their attribute values. Two value objects with the same values are considered equal.

/// Marker trait for value objects.
///
/// Value objects are domain objects that are **immutable** and **compared by value**.
/// They represent concepts where identity doesn't matter - only the values matter.
///
/// ## Value Object vs Entity
///
/// - **Value Object**: No identity (two value objects with same values are equal)
/// - **Entity**: Has identity (two entities with same ID are the same entity)
///
/// Example:
/// - `Money { amount: 100, currency: "USD" }` is a value object
/// - `Customer { id: CustomerId(...), name: "..." }` is an entity
///
/// ## Immutability
///
/// Value objects should be **immutable** - once created, they don't change. To "modify"
/// a value object, create a new one with the new values. This ensures:
/// - **Thread safety**: Immutable objects are safe to share across threads
/// - **Predictability**: Value objects can't be unexpectedly modified
/// - **Value semantics**: Values behave like primitives (can be copied, compared)
///
/// ## Design Constraints
///
/// The trait requires:
/// - **Clone**: Value objects should be cheap to copy (they're values, not references)
/// - **PartialEq**: Value objects are compared by their attribute values
/// - **Debug**: Value objects should be debuggable (helpful for logging, testing)
///
/// ## Usage Pattern
///
/// ```ignore
/// #[derive(Debug, Clone, PartialEq, Eq)]
/// struct Money {
///     amount: i64,
///     currency: String,
/// }
///
/// impl ValueObject for Money {}
///
/// // Two Money objects with same values are equal
/// let m1 = Money { amount: 100, currency: "USD".to_string() };
/// let m2 = Money { amount: 100, currency: "USD".to_string() };
/// assert_eq!(m1, m2);  // Equal by value, not identity
/// ```
pub trait ValueObject: Clone + PartialEq + core::fmt::Debug {}


