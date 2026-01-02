//! Strongly-typed identifiers used across the domain.

use core::str::FromStr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

/// Identifier of a tenant (multi-tenant boundary).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(Uuid);

/// Identifier of a user (actor identity).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(Uuid);

/// Identifier of an aggregate root.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AggregateId(Uuid);

macro_rules! impl_uuid_newtype {
    ($t:ty, $name:literal) => {
        impl $t {
            /// Create a new identifier.
            ///
            /// Uses UUIDv7 (time-ordered). Prefer passing IDs explicitly in tests
            /// for determinism.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl core::fmt::Display for $t {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<Uuid> for $t {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$t> for Uuid {
            fn from(value: $t) -> Self {
                value.0
            }
        }

        impl FromStr for $t {
            type Err = DomainError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid = Uuid::from_str(s)
                    .map_err(|e| DomainError::invalid_id(format!("{}: {}", $name, e)))?;
                Ok(Self(uuid))
            }
        }
    };
}

impl_uuid_newtype!(TenantId, "TenantId");
impl_uuid_newtype!(UserId, "UserId");
impl_uuid_newtype!(AggregateId, "AggregateId");


