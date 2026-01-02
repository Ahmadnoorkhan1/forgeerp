//! Strongly-typed identifiers used across the domain.

use core::str::FromStr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

/// Identifier of a tenant (multi-tenant boundary).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(Uuid);

/// Identifier of a user (actor/principal identity).
///
/// `UserId` represents the identity of a **principal** (user, service account, etc.) who
/// performs actions in the system. This is used for:
///
/// - **Authorization**: Who performed an action (audit trails, permissions)
/// - **Audit logging**: Track which user made changes
/// - **Multi-user scenarios**: Support multiple users per tenant
///
/// Note: In authentication/authorization contexts, this is often called `PrincipalId` to
/// emphasize it can represent any authenticated principal (not just human users).
///
/// ## UUIDv7
///
/// Uses UUIDv7 (time-ordered) for better database index performance and time-based ordering.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(Uuid);

/// Identifier of an aggregate root.
///
/// `AggregateId` uniquely identifies an aggregate root instance. Each aggregate root has
/// a unique ID that persists across all state changes (events).
///
/// ## Aggregate Identity
///
/// In event sourcing, aggregates maintain their identity across all events. The `AggregateId`
/// is used to:
/// - **Load aggregates**: Query events for a specific aggregate
/// - **Route commands**: Direct commands to the correct aggregate instance
/// - **Event streams**: Group events by aggregate (each aggregate has its own event stream)
///
/// ## Event Streams
///
/// In the event store, events are organized into streams, where each stream corresponds to
/// one aggregate instance. The stream key is `(tenant_id, aggregate_id)`, ensuring both
/// tenant isolation and aggregate scoping.
///
/// ## UUIDv7
///
/// Uses UUIDv7 (time-ordered) for better database index performance and time-based ordering.
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


