use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Permission identifier.
///
/// Permissions are modeled as opaque strings (e.g. "inventory.read").
/// A special wildcard permission `"*"` can be used by policy layers to indicate
/// "allow all" without hardcoding domain permissions into tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Permission(Cow<'static, str>);

impl Permission {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_wildcard(&self) -> bool {
        self.as_str() == "*"
    }
}

impl core::fmt::Display for Permission {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}


