//! `forgeerp-auth` — pure authentication/authorization boundary (zero-trust).
//!
//! This crate is intentionally decoupled from HTTP and storage.

pub mod authorize;
pub mod claims;
pub mod permissions;
pub mod principal;
pub mod roles;
pub mod user;

pub use authorize::{
    explain_authorization, AuthorizationExplanation, CommandAuthorization, DenialKind,
    DenialReason, PermissionDefinition, Principal, PrincipalState, RbacRegistry, RoleDefinition,
    authorize, AuthzError,
};
pub use claims::{Hs256JwtValidator, JwtClaims, JwtValidator, TokenValidationError, validate_claims};
pub use permissions::{admin, Permission};
pub use principal::{PrincipalId, TenantMembership};
pub use roles::Role;
pub use user::{
    ActivateUser, AssignRole, CreateUser, RevokeRole, RoleAssigned, RoleRevoked,
    SuspendUser, User, UserActivated, UserCommand, UserCreated, UserError, UserEvent,
    UserId, UserStatus, UserSuspended,
};


