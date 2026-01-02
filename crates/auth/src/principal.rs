use core::str::FromStr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_core::TenantId;

/// Identity of an authenticated principal (human user, service account, etc).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(Uuid);

impl PrincipalId {
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

impl core::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl From<Uuid> for PrincipalId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<PrincipalId> for Uuid {
    fn from(value: PrincipalId) -> Self {
        value.0
    }
}

impl FromStr for PrincipalId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

/// A principal's membership in a tenant.
///
/// This is an authorization boundary object: it states *which tenant* the
/// principal is acting within and which roles/permissions are granted there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantMembership {
    pub tenant_id: TenantId,
    pub roles: Vec<crate::Role>,
    pub permissions: Vec<crate::Permission>,
}


