//! User aggregate for identity management (event-sourced).
//!
//! This module implements user lifecycle management with strict tenant isolation
//! and privilege escalation prevention.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_core::{Aggregate, AggregateId, AggregateRoot, DomainError, TenantId};
use forgeerp_events::Event;

use crate::Role;

// ─────────────────────────────────────────────────────────────────────────────
// User ID
// ─────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a user within a tenant.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(Uuid);

impl UserId {
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

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Display for UserId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for UserId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<UserId> for Uuid {
    fn from(value: UserId) -> Self {
        value.0
    }
}

impl From<AggregateId> for UserId {
    fn from(value: AggregateId) -> Self {
        Self(*value.as_uuid())
    }
}

impl From<UserId> for AggregateId {
    fn from(value: UserId) -> Self {
        AggregateId::from_uuid(value.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// User Status
// ─────────────────────────────────────────────────────────────────────────────

/// User account status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UserStatus {
    /// User is active and can authenticate/transact.
    #[default]
    Active,
    /// User is suspended and cannot authenticate.
    Suspended,
}

impl core::fmt::Display for UserStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UserStatus::Active => write!(f, "Active"),
            UserStatus::Suspended => write!(f, "Suspended"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// User Aggregate
// ─────────────────────────────────────────────────────────────────────────────

/// User aggregate for identity management.
///
/// # Invariants
/// - A user belongs to exactly one tenant (tenant_id is immutable after creation).
/// - Roles are tenant-scoped (no cross-tenant role grants).
/// - Suspended users cannot be assigned new roles.
/// - Users cannot escalate their own privileges.
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub tenant_id: Option<TenantId>,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    pub status: UserStatus,
    pub version: u64,
    pub created: bool,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: UserId::new(),
            tenant_id: None,
            email: String::new(),
            display_name: String::new(),
            roles: Vec::new(),
            status: UserStatus::Active,
            version: 0,
            created: false,
        }
    }
}

impl User {
    pub fn new(tenant_id: TenantId, id: UserId) -> Self {
        Self {
            id,
            tenant_id: Some(tenant_id),
            ..Default::default()
        }
    }

    pub fn empty(id: UserId) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_not_suspended(&self) -> Result<(), DomainError> {
        if self.status == UserStatus::Suspended {
            return Err(DomainError::invariant("user is suspended"));
        }
        Ok(())
    }
}

impl AggregateRoot for User {
    type Id = UserId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Command to create a new user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUser {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub email: String,
    pub display_name: String,
    pub initial_roles: Vec<Role>,
    pub occurred_at: DateTime<Utc>,
}

/// Command to assign a role to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignRole {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: Role,
    /// The roles of the actor performing this operation (for escalation check).
    pub actor_roles: Vec<Role>,
    pub occurred_at: DateTime<Utc>,
}

/// Command to revoke a role from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeRole {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: Role,
    pub occurred_at: DateTime<Utc>,
}

/// Command to suspend a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspendUser {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

/// Command to activate a suspended user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateUser {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub occurred_at: DateTime<Utc>,
}

/// All user commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserCommand {
    Create(CreateUser),
    AssignRole(AssignRole),
    RevokeRole(RevokeRole),
    Suspend(SuspendUser),
    Activate(ActivateUser),
}

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// Event emitted when a user is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreated {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub email: String,
    pub display_name: String,
    pub initial_roles: Vec<Role>,
    pub occurred_at: DateTime<Utc>,
}

/// Event emitted when a role is assigned to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssigned {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: Role,
    pub occurred_at: DateTime<Utc>,
}

/// Event emitted when a role is revoked from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRevoked {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: Role,
    pub occurred_at: DateTime<Utc>,
}

/// Event emitted when a user is suspended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSuspended {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

/// Event emitted when a user is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserActivated {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub occurred_at: DateTime<Utc>,
}

/// All user events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserEvent {
    Created(UserCreated),
    RoleAssigned(RoleAssigned),
    RoleRevoked(RoleRevoked),
    Suspended(UserSuspended),
    Activated(UserActivated),
}

impl Event for UserEvent {
    fn event_type(&self) -> &'static str {
        match self {
            UserEvent::Created(_) => "auth.user.created",
            UserEvent::RoleAssigned(_) => "auth.user.role_assigned",
            UserEvent::RoleRevoked(_) => "auth.user.role_revoked",
            UserEvent::Suspended(_) => "auth.user.suspended",
            UserEvent::Activated(_) => "auth.user.activated",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            UserEvent::Created(e) => e.occurred_at,
            UserEvent::RoleAssigned(e) => e.occurred_at,
            UserEvent::RoleRevoked(e) => e.occurred_at,
            UserEvent::Suspended(e) => e.occurred_at,
            UserEvent::Activated(e) => e.occurred_at,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Error
// ─────────────────────────────────────────────────────────────────────────────

/// User aggregate error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    #[error("{0}")]
    Domain(#[from] DomainError),
}

// ─────────────────────────────────────────────────────────────────────────────
// Aggregate Implementation
// ─────────────────────────────────────────────────────────────────────────────

impl Aggregate for User {
    type Command = UserCommand;
    type Event = UserEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            UserEvent::Created(e) => self.apply_created(e),
            UserEvent::RoleAssigned(e) => self.apply_role_assigned(e),
            UserEvent::RoleRevoked(e) => self.apply_role_revoked(e),
            UserEvent::Suspended(e) => self.apply_suspended(e),
            UserEvent::Activated(e) => self.apply_activated(e),
        }
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            UserCommand::Create(cmd) => self.handle_create(cmd),
            UserCommand::AssignRole(cmd) => self.handle_assign_role(cmd),
            UserCommand::RevokeRole(cmd) => self.handle_revoke_role(cmd),
            UserCommand::Suspend(cmd) => self.handle_suspend(cmd),
            UserCommand::Activate(cmd) => self.handle_activate(cmd),
        }
    }
}

impl User {
    // ─────────────────────────────────────────────────────────────────────────
    // Command Handlers
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_create(&self, cmd: &CreateUser) -> Result<Vec<UserEvent>, DomainError> {
        if self.created {
            return Err(DomainError::invariant("user already exists"));
        }

        // Validate email format (basic check)
        if cmd.email.trim().is_empty() || !cmd.email.contains('@') {
            return Err(DomainError::validation("invalid email format"));
        }

        // Validate display name
        if cmd.display_name.trim().is_empty() {
            return Err(DomainError::validation("display name cannot be empty"));
        }

        Ok(vec![UserEvent::Created(UserCreated {
            tenant_id: cmd.tenant_id,
            user_id: cmd.user_id,
            email: cmd.email.trim().to_lowercase(),
            display_name: cmd.display_name.trim().to_string(),
            initial_roles: cmd.initial_roles.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_assign_role(&self, cmd: &AssignRole) -> Result<Vec<UserEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::NotFound);
        }

        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_not_suspended()?;

        // Check if role is already assigned
        if self.roles.iter().any(|r| r.as_str() == cmd.role.as_str()) {
            return Err(DomainError::invariant("role already assigned"));
        }

        // Privilege escalation check: actor cannot assign roles they don't have
        // Exception: actors with "admin" role can assign any role
        let actor_has_admin = cmd.actor_roles.iter().any(|r| r.as_str() == "admin");
        let actor_has_role = cmd
            .actor_roles
            .iter()
            .any(|r| r.as_str() == cmd.role.as_str());

        if !actor_has_admin && !actor_has_role {
            return Err(DomainError::Unauthorized);
        }

        Ok(vec![UserEvent::RoleAssigned(RoleAssigned {
            tenant_id: cmd.tenant_id,
            user_id: cmd.user_id,
            role: cmd.role.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_revoke_role(&self, cmd: &RevokeRole) -> Result<Vec<UserEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::NotFound);
        }

        self.ensure_tenant(cmd.tenant_id)?;

        // Check if role is actually assigned
        if !self.roles.iter().any(|r| r.as_str() == cmd.role.as_str()) {
            return Err(DomainError::invariant("role not assigned"));
        }

        Ok(vec![UserEvent::RoleRevoked(RoleRevoked {
            tenant_id: cmd.tenant_id,
            user_id: cmd.user_id,
            role: cmd.role.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_suspend(&self, cmd: &SuspendUser) -> Result<Vec<UserEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::NotFound);
        }

        self.ensure_tenant(cmd.tenant_id)?;

        if self.status == UserStatus::Suspended {
            return Err(DomainError::invariant("user already suspended"));
        }

        Ok(vec![UserEvent::Suspended(UserSuspended {
            tenant_id: cmd.tenant_id,
            user_id: cmd.user_id,
            reason: cmd.reason.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_activate(&self, cmd: &ActivateUser) -> Result<Vec<UserEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::NotFound);
        }

        self.ensure_tenant(cmd.tenant_id)?;

        if self.status == UserStatus::Active {
            return Err(DomainError::invariant("user already active"));
        }

        Ok(vec![UserEvent::Activated(UserActivated {
            tenant_id: cmd.tenant_id,
            user_id: cmd.user_id,
            occurred_at: cmd.occurred_at,
        })])
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Event Appliers
    // ─────────────────────────────────────────────────────────────────────────

    fn apply_created(&mut self, e: &UserCreated) {
        self.id = e.user_id;
        self.tenant_id = Some(e.tenant_id);
        self.email = e.email.clone();
        self.display_name = e.display_name.clone();
        self.roles = e.initial_roles.clone();
        self.status = UserStatus::Active;
        self.created = true;
    }

    fn apply_role_assigned(&mut self, e: &RoleAssigned) {
        self.roles.push(e.role.clone());
    }

    fn apply_role_revoked(&mut self, e: &RoleRevoked) {
        self.roles.retain(|r| r.as_str() != e.role.as_str());
    }

    fn apply_suspended(&mut self, _e: &UserSuspended) {
        self.status = UserStatus::Suspended;
    }

    fn apply_activated(&mut self, _e: &UserActivated) {
        self.status = UserStatus::Active;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn create_user_success() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let user = User::empty(user_id);

        let cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "alice@example.com".to_string(),
            display_name: "Alice Smith".to_string(),
            initial_roles: vec![Role::new("user")],
            occurred_at: now(),
        });

        let events = user.handle(&cmd).unwrap();
        assert_eq!(events.len(), 1);

        let UserEvent::Created(e) = &events[0] else {
            panic!("expected UserCreated event");
        };

        assert_eq!(e.email, "alice@example.com");
        assert_eq!(e.display_name, "Alice Smith");
        assert_eq!(e.initial_roles.len(), 1);
    }

    #[test]
    fn create_user_invalid_email() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let user = User::empty(user_id);

        let cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "invalid-email".to_string(),
            display_name: "Alice".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });

        let result = user.handle(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn assign_role_success() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create the user first
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "bob@example.com".to_string(),
            display_name: "Bob".to_string(),
            initial_roles: vec![Role::new("user")],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        // Assign role (actor is admin)
        let assign_cmd = UserCommand::AssignRole(AssignRole {
            tenant_id,
            user_id,
            role: Role::new("manager"),
            actor_roles: vec![Role::new("admin")],
            occurred_at: now(),
        });

        let events = user.handle(&assign_cmd).unwrap();
        assert_eq!(events.len(), 1);

        let UserEvent::RoleAssigned(e) = &events[0] else {
            panic!("expected RoleAssigned event");
        };
        assert_eq!(e.role.as_str(), "manager");
    }

    #[test]
    fn assign_role_privilege_escalation_blocked() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create the user
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "carol@example.com".to_string(),
            display_name: "Carol".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        // Attempt to assign "admin" role by a non-admin actor
        let assign_cmd = UserCommand::AssignRole(AssignRole {
            tenant_id,
            user_id,
            role: Role::new("admin"),
            actor_roles: vec![Role::new("user")], // Actor only has "user" role
            occurred_at: now(),
        });

        let result = user.handle(&assign_cmd);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::Unauthorized));
    }

    #[test]
    fn suspend_user_success() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create the user
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "dave@example.com".to_string(),
            display_name: "Dave".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        // Suspend
        let suspend_cmd = UserCommand::Suspend(SuspendUser {
            tenant_id,
            user_id,
            reason: "Policy violation".to_string(),
            occurred_at: now(),
        });

        let events = user.handle(&suspend_cmd).unwrap();
        assert_eq!(events.len(), 1);

        for event in events {
            user.apply(&event);
        }

        assert_eq!(user.status, UserStatus::Suspended);
    }

    #[test]
    fn cannot_assign_role_to_suspended_user() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create and suspend user
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "eve@example.com".to_string(),
            display_name: "Eve".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        let suspend_cmd = UserCommand::Suspend(SuspendUser {
            tenant_id,
            user_id,
            reason: "Test".to_string(),
            occurred_at: now(),
        });
        for event in user.handle(&suspend_cmd).unwrap() {
            user.apply(&event);
        }

        // Attempt to assign role to suspended user
        let assign_cmd = UserCommand::AssignRole(AssignRole {
            tenant_id,
            user_id,
            role: Role::new("manager"),
            actor_roles: vec![Role::new("admin")],
            occurred_at: now(),
        });

        let result = user.handle(&assign_cmd);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("suspended"));
    }

    #[test]
    fn tenant_isolation_enforced() {
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create user in tenant A
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id: tenant_a,
            user_id,
            email: "frank@example.com".to_string(),
            display_name: "Frank".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        // Attempt to assign role from tenant B
        let assign_cmd = UserCommand::AssignRole(AssignRole {
            tenant_id: tenant_b, // Different tenant!
            user_id,
            role: Role::new("admin"),
            actor_roles: vec![Role::new("admin")],
            occurred_at: now(),
        });

        let result = user.handle(&assign_cmd);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("tenant"));
    }

    #[test]
    fn activate_suspended_user() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create, suspend, then activate
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "grace@example.com".to_string(),
            display_name: "Grace".to_string(),
            initial_roles: vec![],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        let suspend_cmd = UserCommand::Suspend(SuspendUser {
            tenant_id,
            user_id,
            reason: "Test".to_string(),
            occurred_at: now(),
        });
        for event in user.handle(&suspend_cmd).unwrap() {
            user.apply(&event);
        }

        assert_eq!(user.status, UserStatus::Suspended);

        let activate_cmd = UserCommand::Activate(ActivateUser {
            tenant_id,
            user_id,
            occurred_at: now(),
        });
        for event in user.handle(&activate_cmd).unwrap() {
            user.apply(&event);
        }

        assert_eq!(user.status, UserStatus::Active);
    }

    #[test]
    fn revoke_role_success() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let mut user = User::empty(user_id);

        // Create user with a role
        let create_cmd = UserCommand::Create(CreateUser {
            tenant_id,
            user_id,
            email: "henry@example.com".to_string(),
            display_name: "Henry".to_string(),
            initial_roles: vec![Role::new("manager")],
            occurred_at: now(),
        });
        for event in user.handle(&create_cmd).unwrap() {
            user.apply(&event);
        }

        assert!(user.roles.iter().any(|r| r.as_str() == "manager"));

        // Revoke the role
        let revoke_cmd = UserCommand::RevokeRole(RevokeRole {
            tenant_id,
            user_id,
            role: Role::new("manager"),
            occurred_at: now(),
        });
        for event in user.handle(&revoke_cmd).unwrap() {
            user.apply(&event);
        }

        assert!(!user.roles.iter().any(|r| r.as_str() == "manager"));
    }
}
