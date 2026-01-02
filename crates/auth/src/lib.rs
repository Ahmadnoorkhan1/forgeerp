//! `forgeerp-auth` — pure authentication/authorization boundary (zero-trust).
//!
//! This crate is intentionally decoupled from HTTP and storage.

pub mod authorize;
pub mod claims;
pub mod permissions;
pub mod principal;
pub mod roles;

pub use authorize::{authorize, AuthzError, Principal};
pub use claims::{JwtClaims, TokenValidationError, validate_claims};
pub use permissions::Permission;
pub use principal::{PrincipalId, TenantMembership};
pub use roles::Role;


