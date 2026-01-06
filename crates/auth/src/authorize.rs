use std::collections::{HashMap, HashSet};

use serde::Serialize;
use thiserror::Error;

use forgeerp_core::TenantId;

use crate::{Permission, PrincipalId, TenantMembership};

/// A fully resolved principal for authorization decisions.
///
/// Construction of this object is intentionally decoupled from storage and
/// transport: API/workers can derive memberships from claims and a policy source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub principal_id: PrincipalId,
    pub active_tenant_id: TenantId,
    pub membership: TenantMembership,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthzError {
    #[error("tenant mismatch")]
    TenantMismatch,

    #[error("forbidden: missing permission '{0}'")]
    Forbidden(String),
}

/// Command-side authorization contract (checked at the command boundary).
///
/// Implement this on commands that require permissions.
/// The API layer should enforce these requirements before dispatching.
pub trait CommandAuthorization {
    fn required_permissions(&self) -> &[Permission];
}

/// Authorize a principal within its active tenant context.
///
/// - No IO
/// - No panics
/// - No business logic (pure policy check)
pub fn authorize(principal: &Principal, required: &Permission) -> Result<(), AuthzError> {
    if principal.active_tenant_id != principal.membership.tenant_id {
        return Err(AuthzError::TenantMismatch);
    }

    let perms: HashSet<&str> = principal
        .membership
        .permissions
        .iter()
        .map(|p| p.as_str())
        .collect();

    if perms.contains("*") || perms.contains(required.as_str()) {
        Ok(())
    } else {
        Err(AuthzError::Forbidden(required.as_str().to_string()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Authorization Explanation (Audit Trail)
// ─────────────────────────────────────────────────────────────────────────────

/// Detailed explanation of an authorization decision.
///
/// This structure provides transparent, debuggable information about why
/// a request was allowed or denied.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationExplanation {
    /// The permission that was being checked.
    pub required_permission: String,

    /// Whether the authorization was granted.
    pub granted: bool,

    /// Human-readable reason for the decision.
    pub reason: String,

    /// Details about the principal's state.
    pub principal: PrincipalState,

    /// If denied, this explains what was missing.
    pub denial_reason: Option<DenialReason>,
}

/// Current state of the principal being checked.
#[derive(Debug, Clone, Serialize)]
pub struct PrincipalState {
    pub principal_id: PrincipalId,
    pub active_tenant_id: TenantId,
    pub membership_tenant_id: TenantId,
    pub roles: Vec<String>,
    pub effective_permissions: Vec<String>,
    pub has_wildcard: bool,
}

/// Detailed reason why authorization was denied.
#[derive(Debug, Clone, Serialize)]
pub struct DenialReason {
    pub kind: DenialKind,
    pub message: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialKind {
    TenantMismatch,
    MissingPermission,
}

/// Explain why an authorization decision was made (or would be made).
///
/// This function provides detailed, auditable information about authorization
/// decisions. It answers the question: "Why was this request allowed/denied?"
///
/// # Arguments
///
/// - `principal`: The principal whose authorization is being checked
/// - `required`: The permission being requested
/// - `role_permissions`: Function that maps role names to their granted permissions
///
/// # Returns
///
/// A detailed explanation of the authorization decision, including:
/// - Whether access was granted
/// - Why it was granted or denied
/// - What permissions the principal has
/// - Suggestions for fixing denial (if applicable)
pub fn explain_authorization<F>(
    principal: &Principal,
    required: &Permission,
    role_permissions: F,
) -> AuthorizationExplanation
where
    F: Fn(&str) -> Vec<String>,
{
    let required_str = required.as_str();

    // Check tenant mismatch
    if principal.active_tenant_id != principal.membership.tenant_id {
        return AuthorizationExplanation {
            required_permission: required_str.to_string(),
            granted: false,
            reason: format!(
                "Tenant mismatch: principal is active in tenant {} but membership is for tenant {}",
                principal.active_tenant_id, principal.membership.tenant_id
            ),
            principal: PrincipalState {
                principal_id: principal.principal_id,
                active_tenant_id: principal.active_tenant_id,
                membership_tenant_id: principal.membership.tenant_id,
                roles: principal.membership.roles.iter().map(|r| r.as_str().to_string()).collect(),
                effective_permissions: principal
                    .membership
                    .permissions
                    .iter()
                    .map(|p| p.as_str().to_string())
                    .collect(),
                has_wildcard: principal.membership.permissions.iter().any(|p| p.is_wildcard()),
            },
            denial_reason: Some(DenialReason {
                kind: DenialKind::TenantMismatch,
                message: "Principal is authenticated in a different tenant than their membership".to_string(),
                suggestions: vec![
                    "Verify the tenant_id in the JWT token matches the principal's tenant membership".to_string(),
                    "Check that the principal has the correct tenant context for this operation".to_string(),
                ],
            }),
        };
    }

    // Compute effective permissions from roles
    let mut effective_perms: HashSet<String> = HashSet::new();
    for role in &principal.membership.roles {
        for perm in role_permissions(role.as_str()) {
            effective_perms.insert(perm);
        }
    }

    // Add explicit permissions from membership
    for perm in &principal.membership.permissions {
        effective_perms.insert(perm.as_str().to_string());
    }

    let has_wildcard = effective_perms.contains("*");
    let has_required = effective_perms.contains(required_str);

    // Build effective permissions list (sorted for readability)
    let mut effective_perms_list: Vec<String> = effective_perms.into_iter().collect();
    effective_perms_list.sort();

    if has_wildcard || has_required {
        let reason = if has_wildcard {
            format!("Principal has wildcard permission '*' (granted by admin role)")
        } else {
            format!("Principal has explicit permission '{}'", required_str)
        };

        AuthorizationExplanation {
            required_permission: required_str.to_string(),
            granted: true,
            reason,
            principal: PrincipalState {
                principal_id: principal.principal_id,
                active_tenant_id: principal.active_tenant_id,
                membership_tenant_id: principal.membership.tenant_id,
                roles: principal.membership.roles.iter().map(|r| r.as_str().to_string()).collect(),
                effective_permissions: effective_perms_list,
                has_wildcard,
            },
            denial_reason: None,
        }
    } else {
        // Determine which roles would grant this permission
        let mut granting_roles: Vec<String> = Vec::new();
        for role in &principal.membership.roles {
            let role_perms = role_permissions(role.as_str());
            if role_perms.iter().any(|p| p == required_str || p == "*") {
                granting_roles.push(role.as_str().to_string());
            }
        }

        let mut suggestions = vec![
            format!("Assign a role that grants the '{}' permission", required_str),
            format!("Grant the '{}' permission directly to the principal", required_str),
        ];

        if !granting_roles.is_empty() {
            suggestions.insert(
                0,
                format!(
                    "One of the following roles already assigned would grant this permission (check role-permission mapping): {:?}",
                    granting_roles
                ),
            );
        }

        AuthorizationExplanation {
            required_permission: required_str.to_string(),
            granted: false,
            reason: format!(
                "Principal does not have permission '{}'. Current permissions: {:?}",
                required_str, effective_perms_list
            ),
            principal: PrincipalState {
                principal_id: principal.principal_id,
                active_tenant_id: principal.active_tenant_id,
                membership_tenant_id: principal.membership.tenant_id,
                roles: principal.membership.roles.iter().map(|r| r.as_str().to_string()).collect(),
                effective_permissions: effective_perms_list.clone(),
                has_wildcard: false,
            },
            denial_reason: Some(DenialReason {
                kind: DenialKind::MissingPermission,
                message: format!("Missing required permission: '{}'", required_str),
                suggestions,
            }),
        }
    }
}

/// Role definition with its granted permissions (for audit/display).
#[derive(Debug, Clone, Serialize)]
pub struct RoleDefinition {
    pub name: String,
    pub permissions: Vec<String>,
    pub description: Option<String>,
}

/// Permission definition (for audit/display).
#[derive(Debug, Clone, Serialize)]
pub struct PermissionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
}

/// Registry of all available roles and permissions.
///
/// This provides a complete view of the RBAC system for auditing.
pub struct RbacRegistry {
    pub roles: HashMap<String, RoleDefinition>,
    pub permissions: HashMap<String, PermissionDefinition>,
}

impl RbacRegistry {
    /// Create a registry from a role-to-permission mapping function.
    pub fn from_role_mapping<F>(role_permissions: F) -> Self
    where
        F: Fn(&str) -> Vec<String>,
    {
        let mut roles: HashMap<String, RoleDefinition> = HashMap::new();
        let mut permissions: HashMap<String, PermissionDefinition> = HashMap::new();

        // Common roles (you can extend this)
        let known_roles = vec![
            "admin", "manager", "accountant", "salesperson", "warehouse", "user",
        ];

        for role_name in known_roles {
            let perms = role_permissions(role_name);
            roles.insert(
                role_name.to_string(),
                RoleDefinition {
                    name: role_name.to_string(),
                    permissions: perms.clone(),
                    description: role_description(role_name),
                },
            );

            // Collect permissions
            for perm in perms {
                if !permissions.contains_key(&perm) {
                    permissions.insert(
                        perm.clone(),
                        PermissionDefinition {
                            name: perm.clone(),
                            description: permission_description(&perm),
                            category: permission_category(&perm),
                        },
                    );
                }
            }
        }

        Self { roles, permissions }
    }
}

fn role_description(role: &str) -> Option<String> {
    match role {
        "admin" => Some("Full system administrator with all permissions".to_string()),
        "manager" => Some("Business manager with broad operational permissions".to_string()),
        "accountant" => Some("Financial specialist with ledger and invoicing access".to_string()),
        "salesperson" => Some("Sales staff with customer and order management".to_string()),
        "warehouse" => Some("Warehouse staff with inventory and receiving access".to_string()),
        "user" => Some("Basic user with read-only access".to_string()),
        _ => None,
    }
}

fn permission_description(perm: &str) -> Option<String> {
    if perm == "*" {
        return Some("Wildcard permission - grants all permissions".to_string());
    }

    // Parse permission format: "module.action" or "admin.users.action"
    let parts: Vec<&str> = perm.split('.').collect();
    if parts.len() >= 2 {
        let module = parts[0];
        let action = parts[parts.len() - 1];
        let resource = if parts.len() > 2 {
            parts[1..parts.len() - 1].join(".")
        } else {
            module.to_string()
        };

        let action_desc = match action {
            "read" => "View/list",
            "write" => "Create/update/delete",
            "create" => "Create new",
            "list" => "List all",
            _ => action,
        };

        Some(format!("{} {} resources", action_desc, resource))
    } else {
        None
    }
}

fn permission_category(perm: &str) -> Option<String> {
    if perm == "*" {
        return Some("system".to_string());
    }

    let parts: Vec<&str> = perm.split('.').collect();
    if !parts.is_empty() {
        Some(parts[0].to_string())
    } else {
        None
    }
}


