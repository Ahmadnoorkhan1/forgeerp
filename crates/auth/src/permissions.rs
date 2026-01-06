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

// ─────────────────────────────────────────────────────────────────────────────
// Admin Permission Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Permission constants for identity management operations.
///
/// These are intentionally stringent to prevent accidental privilege escalation.
pub mod admin {
    use super::Permission;

    /// Permission to create new users within a tenant.
    pub const USER_CREATE: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.create"));

    /// Permission to list users within a tenant.
    pub const USER_LIST: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.list"));

    /// Permission to view a specific user's details.
    pub const USER_READ: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.read"));

    /// Permission to assign roles to users.
    pub const USER_ASSIGN_ROLE: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.assign_role"));

    /// Permission to revoke roles from users.
    pub const USER_REVOKE_ROLE: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.revoke_role"));

    /// Permission to suspend users.
    pub const USER_SUSPEND: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.suspend"));

    /// Permission to activate suspended users.
    pub const USER_ACTIVATE: Permission = Permission(std::borrow::Cow::Borrowed("admin.users.activate"));

    /// All admin user permissions (convenience for super-admin setup).
    pub fn all_user_permissions() -> Vec<Permission> {
        vec![
            USER_CREATE.clone(),
            USER_LIST.clone(),
            USER_READ.clone(),
            USER_ASSIGN_ROLE.clone(),
            USER_REVOKE_ROLE.clone(),
            USER_SUSPEND.clone(),
            USER_ACTIVATE.clone(),
        ]
    }
}


