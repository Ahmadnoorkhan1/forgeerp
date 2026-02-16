//! Database projection for user credentials (username -> password_hash + user_id mapping).
//!
//! This handles the separate credentials table that stores login credentials.
//! Unlike users_read_model which is built from events, user_credentials is populated
//! directly when a user registers (from the auth register handler).
//!
//! This module mainly provides utilities to ensure credentials are queryable from the database.

use sqlx::PgPool;
use thiserror::Error;

use forgeerp_auth::UserId;
use forgeerp_core::TenantId;

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("credential not found")]
    NotFound,
}

/// Database store for user credentials.
///
/// This is used to verify passwords during login.
/// Credentials are inserted directly when a user registers (via the auth handler),
/// not via event projection, since the password hash must not be stored in events.
pub struct DatabaseCredentialStore {
    pool: PgPool,
}

impl DatabaseCredentialStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the connection pool (for direct queries).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Insert a credential for a user.
    ///
    /// This is called from the auth register handler BEFORE events are published.
    /// The username must be unique per tenant.
    pub async fn insert(
        &self,
        tenant_id: TenantId,
        username: &str,
        password_hash: &str,
        user_id: UserId,
    ) -> Result<(), CredentialError> {
        sqlx::query(
            "INSERT INTO user_credentials (tenant_id, username, password_hash, user_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, NOW(), NOW())
             ON CONFLICT (tenant_id, username)
             DO UPDATE SET password_hash = $3, user_id = $4, updated_at = NOW()"
        )
        .bind(tenant_id.as_uuid())
        .bind(username)
        .bind(password_hash)
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await?;

        tracing::debug!(
            "[DatabaseCredentialStore] Inserted credential for username: {}, user_id: {}",
            username,
            user_id
        );

        Ok(())
    }

    /// Find a credential by username within a tenant.
    pub async fn find(
        &self,
        tenant_id: TenantId,
        username: &str,
    ) -> Result<Option<(String, UserId)>, CredentialError> {
        let row: Option<(String, sqlx::types::Uuid)> = sqlx::query_as(
            "SELECT password_hash, user_id FROM user_credentials 
             WHERE tenant_id = $1 AND username = $2"
        )
        .bind(tenant_id.as_uuid())
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        let result = row.map(|(hash, uid)| (hash, UserId::from_uuid(uid)));

        tracing::debug!(
            "[DatabaseCredentialStore] Looking for username: '{}' | Found: {}",
            username,
            result.is_some()
        );

        Ok(result)
    }

    /// Check if a username exists (cross-tenant, untuk duplicate check saat register).
    /// Lebih efisien dari exists() karena tidak memerlukan tenant_id.
    pub async fn username_exists(&self, username: &str) -> Result<bool, CredentialError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_credentials WHERE username = $1)"
        )
        .bind(username)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    /// Check if a username exists for a tenant.
    pub async fn exists(&self, tenant_id: TenantId, username: &str) -> Result<bool, CredentialError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_credentials WHERE tenant_id = $1 AND username = $2)"
        )
        .bind(tenant_id.as_uuid())
        .bind(username)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    /// Get all credentials for a tenant (mainly for testing/debugging).
    pub async fn find_all(&self, tenant_id: TenantId) -> Result<Vec<(String, UserId)>, CredentialError> {
        let rows: Vec<(String, sqlx::types::Uuid)> = sqlx::query_as(
            "SELECT username, user_id FROM user_credentials WHERE tenant_id = $1"
        )
        .bind(tenant_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(u, uid)| (u, UserId::from_uuid(uid))).collect())
    }

    /// Get user profile from users_read_model by user_id (for login).
    pub async fn get_user_profile(
        &self,
        tenant_id: TenantId,
        user_id: &UserId,
    ) -> Result<Option<(String, String, Vec<String>)>, CredentialError> {
        // Returns (email, status, roles)
        let row: Option<(String, String, Vec<String>)> = sqlx::query_as(
            "SELECT email, status, roles FROM users_read_model 
             WHERE tenant_id = $1 AND user_id = $2"
        )
        .bind(tenant_id.as_uuid())
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    /// Optimized login query: fetch credentials + user profile in single query via JOIN.
    /// Returns (user_id, tenant_id, password_hash, email, status, roles).
    ///
    /// Menggunakan LEFT JOIN — jika users_read_model belum ter-populate (race condition
    /// antara event projection dan register), field email/status/roles akan NULL.
    /// Karena itu field tersebut di-wrap sebagai Option<> agar sqlx tidak error saat
    /// deserialize dan login handler bisa memberikan fallback yang tepat.
    pub async fn get_login_info(
        &self,
        username: &str,
    ) -> Result<Option<(sqlx::types::Uuid, sqlx::types::Uuid, String, Option<String>, Option<String>, Option<Vec<String>>)>, CredentialError> {
        let row: Option<(sqlx::types::Uuid, sqlx::types::Uuid, String, Option<String>, Option<String>, Option<Vec<String>>)> = sqlx::query_as(
            "SELECT 
                uc.user_id, 
                uc.tenant_id, 
                uc.password_hash,
                urm.email,
                urm.status,
                urm.roles
             FROM user_credentials uc
             LEFT JOIN users_read_model urm ON uc.tenant_id = urm.tenant_id AND uc.user_id = urm.user_id
             WHERE uc.username = $1
             LIMIT 1"
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }
}