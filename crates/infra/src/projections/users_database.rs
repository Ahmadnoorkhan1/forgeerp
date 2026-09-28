//! Database-backed projection for user credentials and read model.
//!
//! This projection consumes auth.user events and persists them to Postgres tables:
//! - `user_credentials`: Stores username -> password_hash + user_id mapping
//! - `users_read_model`: Stores full user profile (email, display_name, roles, status)
//! - `projection_offsets`: Tracks which events have been processed (for resumable projections)

use serde_json::Value as JsonValue;
use sqlx::PgPool;
use thiserror::Error;

use forgeerp_auth::{RoleAssigned, RoleRevoked, UserActivated, UserCreated, UserEvent, UserStatus, UserSuspended};
use forgeerp_core::TenantId;
use forgeerp_events::EventEnvelope;

#[derive(Debug, Error)]
pub enum UsersDatabaseProjectionError {
    #[error("failed to deserialize user event: {0}")]
    Deserialize(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("tenant isolation violation: {0}")]
    TenantIsolation(String),
}

/// Database-backed projection for users.
///
/// Consumes published envelopes (JSON payloads) and writes to:
/// 1. `users_read_model` table - the user profile
/// 2. `user_credentials` table - username -> user_id mapping (created on UserCreated event)
///
/// The projection is idempotent (safe for at-least-once delivery).
pub struct UsersDatabaseProjection {
    pool: PgPool,
    projection_name: String,
}

impl UsersDatabaseProjection {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            projection_name: "users.database".to_string(),
        }
    }

    /// Apply a published envelope into the database.
    ///
    /// - Only processes `auth.user` events
    /// - Idempotent: tracks sequence numbers per (tenant, aggregate)
    /// - Updates `projection_offsets` table to resume after crashes
    pub async fn apply_envelope(&self, envelope: &EventEnvelope<JsonValue>) -> Result<(), UsersDatabaseProjectionError> {
        // Only process auth.user events
        if !envelope.aggregate_type().starts_with("auth.user") {
            return Ok(());
        }

        let tenant_id = envelope.tenant_id();
        let aggregate_id = envelope.aggregate_id();
        let seq = envelope.sequence_number();

        // Check if we've already processed this event (idempotency)
        let cursor: Option<i64> = sqlx::query_scalar(
            "SELECT last_sequence_number FROM projection_offsets 
             WHERE tenant_id = $1 AND aggregate_id = $2 AND projection_name = $3"
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .bind(&self.projection_name)
        .fetch_optional(&self.pool)
        .await?;

        // If we've already processed this or later events, skip
        if let Some(last_seq) = cursor {
            if seq as i64 <= last_seq {
                return Ok(());
            }
        }

        // Deserialize the event
        let event: UserEvent = serde_json::from_value(envelope.payload().clone())
            .map_err(|e| UsersDatabaseProjectionError::Deserialize(e.to_string()))?;

        // Apply the event to the database
        match event {
            UserEvent::Created(e) => self.apply_created(tenant_id, e).await?,
            UserEvent::RoleAssigned(e) => self.apply_role_assigned(tenant_id, e).await?,
            UserEvent::RoleRevoked(e) => self.apply_role_revoked(tenant_id, e).await?,
            UserEvent::Suspended(e) => self.apply_suspended(tenant_id, e).await?,
            UserEvent::Activated(e) => self.apply_activated(tenant_id, e).await?,
        }

        // Update offset to mark this event as processed
        sqlx::query(
            "INSERT INTO projection_offsets (tenant_id, aggregate_id, projection_name, last_sequence_number)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, aggregate_id, projection_name)
             DO UPDATE SET last_sequence_number = $4"
        )
        .bind(tenant_id.as_uuid())
        .bind(aggregate_id.as_uuid())
        .bind(&self.projection_name)
        .bind(seq as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Handle UserCreated event: insert into users_read_model and user_credentials
    async fn apply_created(&self, _tenant_id: TenantId, e: UserCreated) -> Result<(), UsersDatabaseProjectionError> {
        // Convert roles to TEXT[] array (PostgreSQL format)
        let role_strs: Vec<String> = e.initial_roles.iter().map(|r| r.as_str().to_string()).collect();
        
        // Insert into users_read_model
        sqlx::query(
            "INSERT INTO users_read_model (tenant_id, user_id, email, display_name, status, roles, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (tenant_id, user_id)
             DO UPDATE SET 
               email = $3,
               display_name = $4,
               roles = $6,
               updated_at = $8"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .bind(&e.email)
        .bind(&e.display_name)
        .bind("Active")  // Initial status
        .bind(&role_strs)  // TEXT[] array
        .bind(e.occurred_at)
        .bind(e.occurred_at)
        .execute(&self.pool)
        .await?;

        // Note: user_credentials table will be populated separately when credentials are created
        // (this is a separate UserCredentialCreated event or handled directly in auth flow)
        // For now, we just ensure the user exists in users_read_model

        Ok(())
    }

    /// Handle RoleAssigned event: update roles in users_read_model
    async fn apply_role_assigned(&self, _tenant_id: TenantId, e: RoleAssigned) -> Result<(), UsersDatabaseProjectionError> {
        // Get current roles
        let current_roles: Option<Vec<String>> = sqlx::query_scalar(
            "SELECT roles FROM users_read_model WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        let mut roles: Vec<String> = current_roles.unwrap_or_default();

        let role_str = e.role.as_str().to_string();
        if !roles.contains(&role_str) {
            roles.push(role_str);
        }

        sqlx::query(
            "UPDATE users_read_model 
             SET roles = $3, updated_at = $4
             WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .bind(&roles)  // TEXT[] array
        .bind(e.occurred_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Handle RoleRevoked event: update roles in users_read_model
    async fn apply_role_revoked(&self, _tenant_id: TenantId, e: RoleRevoked) -> Result<(), UsersDatabaseProjectionError> {
        // Get current roles
        let current_roles: Option<Vec<String>> = sqlx::query_scalar(
            "SELECT roles FROM users_read_model WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        let mut roles: Vec<String> = current_roles.unwrap_or_default();

        let role_str = e.role.as_str().to_string();
        roles.retain(|r| r != &role_str);

        sqlx::query(
            "UPDATE users_read_model 
             SET roles = $3, updated_at = $4
             WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .bind(&roles)  // TEXT[] array
        .bind(e.occurred_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Handle UserSuspended event: update status in users_read_model
    async fn apply_suspended(&self, _tenant_id: TenantId, e: UserSuspended) -> Result<(), UsersDatabaseProjectionError> {
        sqlx::query(
            "UPDATE users_read_model 
             SET status = $3, updated_at = $4
             WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .bind(UserStatus::Suspended.to_string())
        .bind(e.occurred_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Handle UserActivated event: update status in users_read_model
    async fn apply_activated(&self, _tenant_id: TenantId, e: UserActivated) -> Result<(), UsersDatabaseProjectionError> {
        sqlx::query(
            "UPDATE users_read_model 
             SET status = $3, updated_at = $4
             WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(e.tenant_id.as_uuid())
        .bind(e.user_id.as_uuid())
        .bind(UserStatus::Active.to_string())
        .bind(e.occurred_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
