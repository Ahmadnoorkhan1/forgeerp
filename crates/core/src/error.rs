//! Domain error model.
//!
//! This module defines the error types used throughout the domain layer. Domain errors
//! represent **business logic failures** - validation errors, invariant violations,
//! conflicts, etc. They are distinct from infrastructure errors (network failures,
//! database errors, etc.).

use thiserror::Error;

/// Result type used across the domain layer.
///
/// This is a convenience alias for `Result<T, DomainError>`, making domain error handling
/// more ergonomic throughout the codebase.
pub type DomainResult<T> = Result<T, DomainError>;

/// Domain-level error (business logic failures).
///
/// This enum represents **deterministic, business/domain failures** - errors that can
/// be detected and handled based on business rules, not infrastructure concerns.
///
/// ## Design Philosophy
///
/// Keep this focused on deterministic, business/domain failures (validation, invariants,
/// conflicts). Infrastructure concerns (network errors, database errors, etc.) belong
/// elsewhere (in infrastructure error types).
///
/// Domain errors should be:
/// - **Deterministic**: Same input always produces the same error (or success)
/// - **Business-focused**: Represent business rule violations, not technical failures
/// - **Actionable**: Callers can handle these errors meaningfully (retry, user feedback, etc.)
///
/// ## Error Categories
///
/// - **Validation**: Invalid input (malformed data, missing required fields)
/// - **InvariantViolation**: Business rule violation (e.g., stock cannot go negative)
/// - **Conflict**: Concurrent modification (optimistic concurrency failure)
/// - **NotFound**: Requested resource doesn't exist
/// - **Unauthorized**: Permission denied (domain-level authorization)
/// - **InvalidId**: Identifier parsing/validation failure
///
/// ## Error Handling
///
/// Domain errors are typically:
/// - **Returned** from domain functions (not thrown/panicked)
/// - **Mapped** to HTTP status codes in the API layer
/// - **Logged** for observability
/// - **Surfaced** to users (validation errors) or handled internally (conflicts, retries)
///
/// ## Clone and PartialEq
///
/// Errors are `Clone` and `PartialEq` to enable:
/// - **Error propagation**: Errors can be cloned when crossing thread boundaries
/// - **Testing**: Errors can be compared in tests (`assert_eq!(result, Err(DomainError::Validation(...)))`)
/// - **Error aggregation**: Multiple errors can be collected and compared
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// A value failed validation (e.g. malformed input).
    #[error("validation failed: {0}")]
    Validation(String),

    /// A domain invariant was violated.
    #[error("invariant violated: {0}")]
    InvariantViolation(String),

    /// An identifier was invalid (e.g. parse failure).
    #[error("invalid identifier: {0}")]
    InvalidId(String),

    /// A requested resource was not found (domain-level).
    #[error("not found")]
    NotFound,

    /// A conflict occurred (e.g. stale version / optimistic concurrency).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Authorization failure at the domain boundary.
    #[error("unauthorized")]
    Unauthorized,
}

impl DomainError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn invariant(msg: impl Into<String>) -> Self {
        Self::InvariantViolation(msg.into())
    }

    pub fn invalid_id(msg: impl Into<String>) -> Self {
        Self::InvalidId(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn not_found() -> Self {
        Self::NotFound
    }
}


