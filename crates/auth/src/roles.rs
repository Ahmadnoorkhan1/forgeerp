use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Role identifier used for RBAC.
///
/// Roles are intentionally opaque strings at this layer; mapping roles to
/// permissions can be done by the caller/policy layer (often infra-backed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Role(Cow<'static, str>);

impl Role {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Role {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}


