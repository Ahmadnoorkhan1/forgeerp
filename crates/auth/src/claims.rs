use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use forgeerp_core::TenantId;

use crate::{PrincipalId, Role};

/// JWT claims model (transport-agnostic).
///
/// This is the minimal set of claims ForgeERP expects once a token has been
/// decoded/verified by whatever transport/security layer is in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject / principal identifier.
    pub sub: PrincipalId,

    /// Tenant context for the token.
    pub tenant_id: TenantId,

    /// RBAC roles granted within the tenant context.
    pub roles: Vec<Role>,

    /// Issued-at timestamp.
    pub issued_at: DateTime<Utc>,

    /// Expiration timestamp.
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TokenValidationError {
    #[error("token has expired")]
    Expired,

    #[error("token not yet valid (issued_at is in the future)")]
    NotYetValid,

    #[error("invalid token time window (expires_at <= issued_at)")]
    InvalidTimeWindow,
}

/// Deterministically validate JWT claims.
///
/// Note: this validates the *claims* only. Signature verification / decoding is
/// intentionally outside this crate.
pub fn validate_claims(claims: &JwtClaims, now: DateTime<Utc>) -> Result<(), TokenValidationError> {
    if claims.expires_at <= claims.issued_at {
        return Err(TokenValidationError::InvalidTimeWindow);
    }
    if now < claims.issued_at {
        return Err(TokenValidationError::NotYetValid);
    }
    if now >= claims.expires_at {
        return Err(TokenValidationError::Expired);
    }
    Ok(())
}


