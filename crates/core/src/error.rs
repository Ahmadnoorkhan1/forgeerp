//! Domain error model.

use thiserror::Error;

/// Result type used across the domain layer.
pub type DomainResult<T> = Result<T, DomainError>;

/// Domain-level error.
///
/// Keep this focused on deterministic, business/domain failures (validation,
/// invariants, conflicts). Infrastructure concerns belong elsewhere.
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


