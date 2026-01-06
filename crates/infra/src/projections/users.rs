//! Users projection for identity management read models.
//!
//! This projection builds tenant-isolated user read models from auth events.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_auth::{
    Role, RoleAssigned, RoleRevoked, UserActivated, UserCreated, UserEvent, UserId,
    UserStatus, UserSuspended,
};
use forgeerp_core::TenantId;
use forgeerp_events::EventEnvelope;

use crate::read_model::TenantStore;

// ─────────────────────────────────────────────────────────────────────────────
// Read Model
// ─────────────────────────────────────────────────────────────────────────────

/// User read model for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserReadModel {
    pub user_id: UserId,
    pub tenant_id: TenantId,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Effective permissions read model for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePermissions {
    pub user_id: UserId,
    pub tenant_id: TenantId,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Projection
// ─────────────────────────────────────────────────────────────────────────────

/// Projection that maintains user directory per tenant.
pub struct UsersProjection<S> {
    store: S,
}

impl<S> UsersProjection<S>
where
    S: TenantStore<UserId, UserReadModel>,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn apply_envelope(
        &self,
        envelope: &EventEnvelope<serde_json::Value>,
    ) -> Result<(), anyhow::Error> {
        // Only process auth.user events
        if !envelope.aggregate_type().starts_with("auth.user") {
            return Ok(());
        }

        let event: UserEvent = serde_json::from_value(envelope.payload().clone())?;
        let tenant_id = envelope.tenant_id();

        match event {
            UserEvent::Created(e) => self.apply_created(tenant_id, e),
            UserEvent::RoleAssigned(e) => self.apply_role_assigned(tenant_id, e),
            UserEvent::RoleRevoked(e) => self.apply_role_revoked(tenant_id, e),
            UserEvent::Suspended(e) => self.apply_suspended(tenant_id, e),
            UserEvent::Activated(e) => self.apply_activated(tenant_id, e),
        }
    }

    fn apply_created(&self, tenant_id: TenantId, e: UserCreated) -> Result<(), anyhow::Error> {
        let model = UserReadModel {
            user_id: e.user_id,
            tenant_id: e.tenant_id,
            email: e.email,
            display_name: e.display_name,
            roles: e.initial_roles.iter().map(|r| r.as_str().to_string()).collect(),
            status: UserStatus::Active.to_string(),
            created_at: e.occurred_at,
            updated_at: e.occurred_at,
        };
        self.store.upsert(tenant_id, e.user_id, model);
        Ok(())
    }

    fn apply_role_assigned(&self, tenant_id: TenantId, e: RoleAssigned) -> Result<(), anyhow::Error> {
        if let Some(mut model) = self.store.get(tenant_id, &e.user_id) {
            let role_str = e.role.as_str().to_string();
            if !model.roles.contains(&role_str) {
                model.roles.push(role_str);
            }
            model.updated_at = e.occurred_at;
            self.store.upsert(tenant_id, e.user_id, model);
        }
        Ok(())
    }

    fn apply_role_revoked(&self, tenant_id: TenantId, e: RoleRevoked) -> Result<(), anyhow::Error> {
        if let Some(mut model) = self.store.get(tenant_id, &e.user_id) {
            let role_str = e.role.as_str().to_string();
            model.roles.retain(|r| r != &role_str);
            model.updated_at = e.occurred_at;
            self.store.upsert(tenant_id, e.user_id, model);
        }
        Ok(())
    }

    fn apply_suspended(&self, tenant_id: TenantId, e: UserSuspended) -> Result<(), anyhow::Error> {
        if let Some(mut model) = self.store.get(tenant_id, &e.user_id) {
            model.status = UserStatus::Suspended.to_string();
            model.updated_at = e.occurred_at;
            self.store.upsert(tenant_id, e.user_id, model);
        }
        Ok(())
    }

    fn apply_activated(&self, tenant_id: TenantId, e: UserActivated) -> Result<(), anyhow::Error> {
        if let Some(mut model) = self.store.get(tenant_id, &e.user_id) {
            model.status = UserStatus::Active.to_string();
            model.updated_at = e.occurred_at;
            self.store.upsert(tenant_id, e.user_id, model);
        }
        Ok(())
    }

    /// Get a single user by ID.
    pub fn get(&self, tenant_id: TenantId, user_id: &UserId) -> Option<UserReadModel> {
        self.store.get(tenant_id, user_id)
    }

    /// List all users for a tenant.
    pub fn list(&self, tenant_id: TenantId) -> Vec<UserReadModel> {
        self.store.list(tenant_id)
    }

    /// Get a user by email (linear scan).
    pub fn get_by_email(&self, tenant_id: TenantId, email: &str) -> Option<UserReadModel> {
        let normalized = email.trim().to_lowercase();
        self.list(tenant_id)
            .into_iter()
            .find(|u| u.email == normalized)
    }

    /// Compute effective permissions for a user based on role-to-permission mapping.
    ///
    /// The `role_permissions` function maps a role name to its granted permissions.
    pub fn effective_permissions<F>(
        &self,
        tenant_id: TenantId,
        user_id: &UserId,
        role_permissions: F,
    ) -> Option<EffectivePermissions>
    where
        F: Fn(&str) -> Vec<String>,
    {
        let model = self.get(tenant_id, user_id)?;

        let mut all_permissions: HashSet<String> = HashSet::new();
        for role in &model.roles {
            for perm in role_permissions(role) {
                all_permissions.insert(perm);
            }
        }

        Some(EffectivePermissions {
            user_id: model.user_id,
            tenant_id: model.tenant_id,
            roles: model.roles,
            permissions: all_permissions.into_iter().collect(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Default Role-to-Permission Mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Default role-to-permission mapping for ForgeERP.
///
/// This provides a sensible default; production deployments can override this.
pub fn default_role_permissions(role: &str) -> Vec<String> {
    match role {
        "admin" => {
            // Admins get all permissions (wildcard)
            vec!["*".to_string()]
        }
        "manager" => vec![
            // Inventory
            "inventory.read".to_string(),
            "inventory.write".to_string(),
            // Products
            "products.read".to_string(),
            "products.write".to_string(),
            // Sales
            "sales.read".to_string(),
            "sales.write".to_string(),
            // Invoicing
            "invoices.read".to_string(),
            "invoices.write".to_string(),
            // Purchasing
            "purchases.read".to_string(),
            "purchases.write".to_string(),
            // Customers/Suppliers
            "parties.read".to_string(),
            "parties.write".to_string(),
            // Ledger (read-only for managers)
            "ledger.read".to_string(),
            // User management (limited)
            "admin.users.list".to_string(),
            "admin.users.read".to_string(),
        ],
        "accountant" => vec![
            // Read access to most things
            "inventory.read".to_string(),
            "products.read".to_string(),
            "sales.read".to_string(),
            "purchases.read".to_string(),
            "parties.read".to_string(),
            // Full ledger access
            "ledger.read".to_string(),
            "ledger.write".to_string(),
            // Invoicing
            "invoices.read".to_string(),
            "invoices.write".to_string(),
        ],
        "salesperson" => vec![
            // Customers and sales
            "parties.read".to_string(),
            "parties.write".to_string(),
            "products.read".to_string(),
            "sales.read".to_string(),
            "sales.write".to_string(),
            "invoices.read".to_string(),
        ],
        "warehouse" => vec![
            // Inventory operations
            "inventory.read".to_string(),
            "inventory.write".to_string(),
            "products.read".to_string(),
            // Purchasing (goods receipt)
            "purchases.read".to_string(),
            "purchases.write".to_string(),
        ],
        "user" => vec![
            // Basic read access
            "inventory.read".to_string(),
            "products.read".to_string(),
        ],
        _ => vec![], // Unknown roles get no permissions
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_model::InMemoryTenantStore;
    use forgeerp_auth::Role;
    use std::sync::Arc;
    use uuid::Uuid;

    fn make_envelope(
        tenant_id: TenantId,
        user_id: UserId,
        event: UserEvent,
    ) -> EventEnvelope<serde_json::Value> {
        let payload = serde_json::to_value(&event).unwrap();
        EventEnvelope::new(
            Uuid::now_v7(),
            tenant_id,
            forgeerp_core::AggregateId::from_uuid(*user_id.as_uuid()),
            "auth.user",
            1,
            payload,
        )
    }

    #[test]
    fn user_created_projection() {
        let store = Arc::new(InMemoryTenantStore::new());
        let projection = UsersProjection::new(store);

        let tenant_id = TenantId::new();
        let user_id = UserId::new();

        let event = UserEvent::Created(UserCreated {
            tenant_id,
            user_id,
            email: "alice@example.com".to_string(),
            display_name: "Alice Smith".to_string(),
            initial_roles: vec![Role::new("user")],
            occurred_at: Utc::now(),
        });

        let envelope = make_envelope(tenant_id, user_id, event);
        projection.apply_envelope(&envelope).unwrap();

        let user = projection.get(tenant_id, &user_id).unwrap();
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.display_name, "Alice Smith");
        assert_eq!(user.roles, vec!["user"]);
        assert_eq!(user.status, "Active");
    }

    #[test]
    fn role_assignment_updates_projection() {
        let store = Arc::new(InMemoryTenantStore::new());
        let projection = UsersProjection::new(store);

        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let now = Utc::now();

        // Create user
        let create_event = UserEvent::Created(UserCreated {
            tenant_id,
            user_id,
            email: "bob@example.com".to_string(),
            display_name: "Bob".to_string(),
            initial_roles: vec![Role::new("user")],
            occurred_at: now,
        });
        projection
            .apply_envelope(&make_envelope(tenant_id, user_id, create_event))
            .unwrap();

        // Assign role
        let assign_event = UserEvent::RoleAssigned(RoleAssigned {
            tenant_id,
            user_id,
            role: Role::new("manager"),
            occurred_at: now,
        });
        projection
            .apply_envelope(&make_envelope(tenant_id, user_id, assign_event))
            .unwrap();

        let user = projection.get(tenant_id, &user_id).unwrap();
        assert!(user.roles.contains(&"user".to_string()));
        assert!(user.roles.contains(&"manager".to_string()));
    }

    #[test]
    fn suspension_updates_status() {
        let store = Arc::new(InMemoryTenantStore::new());
        let projection = UsersProjection::new(store);

        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let now = Utc::now();

        // Create user
        let create_event = UserEvent::Created(UserCreated {
            tenant_id,
            user_id,
            email: "carol@example.com".to_string(),
            display_name: "Carol".to_string(),
            initial_roles: vec![],
            occurred_at: now,
        });
        projection
            .apply_envelope(&make_envelope(tenant_id, user_id, create_event))
            .unwrap();

        // Suspend user
        let suspend_event = UserEvent::Suspended(UserSuspended {
            tenant_id,
            user_id,
            reason: "Policy violation".to_string(),
            occurred_at: now,
        });
        projection
            .apply_envelope(&make_envelope(tenant_id, user_id, suspend_event))
            .unwrap();

        let user = projection.get(tenant_id, &user_id).unwrap();
        assert_eq!(user.status, "Suspended");
    }

    #[test]
    fn effective_permissions_computed_correctly() {
        let store = Arc::new(InMemoryTenantStore::new());
        let projection = UsersProjection::new(store);

        let tenant_id = TenantId::new();
        let user_id = UserId::new();

        let event = UserEvent::Created(UserCreated {
            tenant_id,
            user_id,
            email: "dave@example.com".to_string(),
            display_name: "Dave".to_string(),
            initial_roles: vec![Role::new("manager"), Role::new("accountant")],
            occurred_at: Utc::now(),
        });

        projection
            .apply_envelope(&make_envelope(tenant_id, user_id, event))
            .unwrap();

        let effective = projection
            .effective_permissions(tenant_id, &user_id, default_role_permissions)
            .unwrap();

        // Manager has ledger.read, accountant has ledger.write
        assert!(effective.permissions.contains(&"ledger.read".to_string()));
        assert!(effective.permissions.contains(&"ledger.write".to_string()));
        // Both should be present
        assert!(effective.roles.contains(&"manager".to_string()));
        assert!(effective.roles.contains(&"accountant".to_string()));
    }

    #[test]
    fn tenant_isolation_enforced() {
        let store = Arc::new(InMemoryTenantStore::new());
        let projection = UsersProjection::new(store);

        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();
        let user_id = UserId::new();

        let event = UserEvent::Created(UserCreated {
            tenant_id: tenant_a,
            user_id,
            email: "eve@example.com".to_string(),
            display_name: "Eve".to_string(),
            initial_roles: vec![],
            occurred_at: Utc::now(),
        });

        projection
            .apply_envelope(&make_envelope(tenant_a, user_id, event))
            .unwrap();

        // User exists in tenant A
        assert!(projection.get(tenant_a, &user_id).is_some());

        // User does NOT exist in tenant B
        assert!(projection.get(tenant_b, &user_id).is_none());

        // List for tenant B is empty
        assert!(projection.list(tenant_b).is_empty());
    }
}

