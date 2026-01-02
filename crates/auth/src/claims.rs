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
    #[serde(rename = "iat", with = "chrono::serde::ts_seconds")]
    pub issued_at: DateTime<Utc>,

    /// Expiration timestamp.
    #[serde(rename = "exp", with = "chrono::serde::ts_seconds")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TokenValidationError {
    #[error("missing token")]
    MissingToken,

    #[error("invalid token format")]
    InvalidFormat,

    #[error("invalid token: {0}")]
    InvalidToken(String),

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

/// JWT validator abstraction (keeps API decoupled from token decoding).
pub trait JwtValidator: Send + Sync {
    fn validate(&self, token: &str, now: DateTime<Utc>) -> Result<JwtClaims, TokenValidationError>;
}

/// Minimal HS256 validator (signature verification + claims validation).
#[derive(Debug, Clone)]
pub struct Hs256JwtValidator {
    secret: Vec<u8>,
}

impl Hs256JwtValidator {
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
        }
    }
}

impl JwtValidator for Hs256JwtValidator {
    fn validate(&self, token: &str, now: DateTime<Utc>) -> Result<JwtClaims, TokenValidationError> {
        if token.trim().is_empty() {
            return Err(TokenValidationError::MissingToken);
        }

        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        // We validate exp/iat deterministically ourselves.
        validation.validate_exp = false;
        validation.validate_nbf = false;

        let decoded = jsonwebtoken::decode::<JwtClaims>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(&self.secret),
            &validation,
        )
        .map_err(|e| TokenValidationError::InvalidToken(e.to_string()))?;

        validate_claims(&decoded.claims, now)?;
        Ok(decoded.claims)
    }
}


