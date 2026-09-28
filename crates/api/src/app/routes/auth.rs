//! Authentication endpoints: register + login.
//!
//! # Tenant Strategy: AUTO-CREATE on Register
//!
//! When a user registers without an invite_token, a new Tenant is
//! auto-created and the user becomes its admin.
//!
//! To join an existing tenant, a tenant admin generates an invite
//! (POST /admin/invites → token). The invitee includes that token
//! in their register request.

use axum::{
    extract::{State, Extension},
    response::IntoResponse,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};

//use crate::app::services::AppServices;

use forgeerp_auth::{CreateUser, JwtClaims, PrincipalId, Role, User, UserCommand, UserId};
use forgeerp_core::{AggregateId, TenantId};

use crate::app::{errors, services::{AppServices, AuthStores, StoredCredential, StoredInvite}, OptionalDbCredentialStore};

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    /// Optional: if present, join this tenant instead of creating one.
    pub invite_token: Option<String>,
}

/// Login request payload.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response with JWT token.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
    pub tenant: TenantInfo,
}

/// User information returned after login.
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    pub display_name: String,
    pub tenant_id: String,
}

#[derive(Debug, Serialize)]
pub struct TenantInfo {
    pub id: String,
    pub name: String,
}

// ─── Auth Config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub jwt_secret: Vec<u8>,
    pub token_expiry_hours: i64,
}

impl AuthConfig {
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            jwt_secret: secret.into(),
            token_expiry_hours: 24,
        }
    }
}

/// Error response for authentication failures.
#[derive(Debug, Serialize)]
pub struct AuthError {
    pub error: String,
}

// ─── Router ──────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// POST /auth/register
pub async fn register(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(auth_stores): Extension<Arc<AuthStores>>,
    Extension(auth_config): Extension<Arc<AuthConfig>>,
    Extension(db_credential_store): Extension<OptionalDbCredentialStore>,
    Json(body): Json<RegisterRequest>,
) -> impl IntoResponse {
    // Validate
    if body.username.trim().is_empty() {
        return errors::json_error(StatusCode::BAD_REQUEST, "validation", "username is required");
    }
    if !body.email.contains('@') {
        return errors::json_error(StatusCode::BAD_REQUEST, "validation", "invalid email format");
    }
    if body.password.len() < 6 {
        return errors::json_error(StatusCode::BAD_REQUEST, "validation", "password must be at least 6 characters");
    }

    // Duplicate username check — cek in-memory store terlebih dahulu (cepat, O(1))
    if auth_stores.credentials.find(body.username.trim()).is_some() {
        return errors::json_error(StatusCode::CONFLICT, "conflict", "username already taken");
    }

    // Jika DB tersedia, cek juga di DB sebelum bcrypt::hash yang mahal.
    // Ini penting karena in-memory store kosong setiap restart (USE_PERSISTENT_STORES=true),
    // sehingga user yang sudah ada di DB tidak terdeteksi oleh check di atas.
    if let Some(db_store) = &db_credential_store.0 {
        match db_store.username_exists(body.username.trim()).await {
            Ok(true) => return errors::json_error(StatusCode::CONFLICT, "conflict", "username already taken"),
            Ok(false) => {}
            Err(e) => {
                tracing::warn!("[REGISTER] Failed to check username uniqueness in DB: {:?}", e);
                // Lanjut; DB insert akan gagal dengan constraint error jika duplikat
            }
        }
    }

    // Hash password — bcrypt adalah CPU-intensive (~300-500ms per DEFAULT_COST=12).
    // Offload ke blocking thread pool agar tidak memblokir Tokio async worker thread.
    // CATATAN: bcrypt cost dapat dikurangi di environment dev via BCRYPT_COST env var
    // (misal: BCRYPT_COST=4 untuk testing), tapi jangan kurang dari 10 untuk production.
    let bcrypt_cost = std::env::var("BCRYPT_COST")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(bcrypt::DEFAULT_COST);
    let password_to_hash = body.password.clone();
    let password_hash = match tokio::task::spawn_blocking(move || {
        bcrypt::hash(&password_to_hash, bcrypt_cost)
    })
    .await
    {
        Ok(Ok(h)) => h,
        _ => return errors::json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "failed to process credentials"),
    };

    // Determine tenant + roles
    let (tenant_id, tenant_name, initial_roles) = if let Some(ref token) = body.invite_token {
        match auth_stores.invites.consume(token) {
            Some((tid, tname)) => (tid, tname, vec![Role::new("user")]),
            None => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_invite", "invite token is invalid or expired"),
        }
    } else {
        let tid = TenantId::new();
        let tname = format!("{}'s Workspace", body.username.trim());
        auth_stores.tenants.insert(tid, tname.clone());
        (tid, tname, vec![Role::new("admin"), Role::new("user")])
    };

    // Create user via event sourcing
    let agg_id = AggregateId::new();
    let user_id = UserId::from(agg_id);

    let cmd = UserCommand::Create(CreateUser {
        tenant_id,
        user_id,
        email: body.email.trim().to_lowercase(),
        display_name: body.username.trim().to_string(),
        initial_roles: initial_roles.clone(),
        occurred_at: Utc::now(),
    });

    // Gunakan dispatch_async agar tidak memanggil block_in_place() dari async context.
    // dispatch() sync → PostgresEventStore::block_in_place() menahan koneksi DB →
    // pool exhaustion → semua query berikutnya (login) stuck antri menit-menit.
    if let Err(e) = services.dispatch_async::<User>(
        tenant_id,
        agg_id,
        "auth.user",
        cmd,
        |t, aggregate_id| User::new(t, UserId::from(aggregate_id)),
    ).await {
        tracing::error!("create user dispatch failed: {:?}", e);
        return errors::json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "failed to create user");
    }

    // Initialize user in users_read_model immediately (projection will update later)
    // This ensures foreign key constraint is satisfied when storing credentials
    if let Some(db_store) = &db_credential_store.0 {
        let role_strs: Vec<String> = initial_roles.iter().map(|r| r.as_str().to_string()).collect();

        // Insert into users_read_model using sqlx directly
        if let Err(e) = sqlx::query(
            "INSERT INTO users_read_model (tenant_id, user_id, email, display_name, status, roles, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
             ON CONFLICT (tenant_id, user_id) DO NOTHING"
        )
        .bind(tenant_id.as_uuid())
        .bind(user_id.as_uuid())
        .bind(body.email.trim().to_lowercase())
        .bind(body.username.trim())
        .bind("Active")
        .bind(&role_strs)
        .execute(db_store.pool())
        .await {
            tracing::warn!("[REGISTER] Failed to pre-populate users_read_model: {:?}", e);
            // Don't fail registration if this fails - projection will create it
        }
    }

    // Store credentials
    tracing::info!("[REGISTER] Attempting to store credentials for username: {}", body.username.trim());
    tracing::info!("[REGISTER] auth_stores ptr: {:p}", &auth_stores.credentials);

    auth_stores.credentials.insert(StoredCredential {
        username: body.username.trim().to_string(),
        password_hash: password_hash.clone(),
        user_id,
        tenant_id,
    });

    // Also store in database if available
    if let Some(db_store) = &db_credential_store.0 {
        if let Err(e) = db_store.insert(
            tenant_id,
            body.username.trim(),
            &password_hash,
            user_id,
        ).await {
            tracing::error!("[REGISTER] Failed to store credentials in database: {:?}", e);
            // Don't fail the registration if database storage fails (graceful degradation)
        } else {
            tracing::info!("[REGISTER] ✅ Credentials stored in database successfully");
        }
    }

    tracing::info!("[REGISTER] ✅ Credentials stored successfully");

    let token = match issue_token(&auth_config, user_id, tenant_id, &initial_roles) {
        Ok(t) => t,
        Err(_) => return errors::json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "failed to issue token"),
    };

    (StatusCode::CREATED, Json(LoginResponse {
        token,
        user: UserInfo {
            id: user_id.to_string(),
            username: body.username.trim().to_string(),
            display_name: body.username.trim().to_string(),
            tenant_id: tenant_id.to_string(),
            email: body.email.trim().to_lowercase(),
            roles: initial_roles.iter().map(|r| r.as_str().to_string()).collect(),
        },
        tenant: TenantInfo {
            id: tenant_id.to_string(),
            name: tenant_name,
        },
    })).into_response()
}

/// Login endpoint - validates credentials and returns JWT token.
/// POST /auth/login
pub async fn login(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(auth_stores): Extension<Arc<AuthStores>>,
    Extension(auth_config): Extension<Arc<AuthConfig>>,
    Extension(db_credential_store): Extension<OptionalDbCredentialStore>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    tracing::info!("[LOGIN] Attempting login for username: {}", body.username.trim());

    // Try database first with optimized query (fetch credentials + profile in single query)
    let (creds_user_id, creds_tenant_id, password_hash, email, status, roles_vec) = 
        if let Some(db_store) = &db_credential_store.0 {
            match db_store.get_login_info(body.username.trim()).await {
                Ok(Some((user_uuid, tenant_uuid, hash, opt_email, opt_status, opt_roles))) => {
                    // LEFT JOIN: jika users_read_model belum ter-populate, gunakan fallback values.
                    // Ini terjadi ketika login dipanggil sangat cepat setelah register sebelum
                    // event projection selesai menulis ke users_read_model.
                    let email   = opt_email.unwrap_or_else(|| body.username.trim().to_lowercase());
                    let status  = opt_status.unwrap_or_else(|| "Active".to_string());
                    let roles   = opt_roles.unwrap_or_else(|| vec!["user".to_string()]);
                    tracing::info!("[LOGIN] ✅ Credentials + profile found in database for username: {}", body.username.trim());
                    (
                        UserId::from_uuid(user_uuid),
                        TenantId::from_uuid(tenant_uuid),
                        hash,
                        email,
                        status,
                        roles,
                    )
                },
                Ok(None) => {
                    tracing::warn!("[LOGIN] ❌ Credentials NOT found in database, trying in-memory");
                    match auth_stores.credentials.find(body.username.trim()) {
                        Some(c) => {
                            tracing::info!("[LOGIN] ✅ Credentials found in in-memory store");
                            match services.users_get(c.tenant_id, &c.user_id) {
                                Some(u) => (c.user_id, c.tenant_id, c.password_hash, u.email.clone(), u.status.clone(), u.roles.clone()),
                                None => {
                                    tracing::warn!("[LOGIN] ⚠️ User profile not found in memory");
                                    return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                                }
                            }
                        },
                        None => {
                            tracing::warn!("[LOGIN] ❌ Credentials NOT found");
                            return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!("[LOGIN] ⚠️ Failed to query database: {:?}, trying in-memory", e);
                    match auth_stores.credentials.find(body.username.trim()) {
                        Some(c) => {
                            tracing::info!("[LOGIN] ✅ Credentials found in in-memory store");
                            match services.users_get(c.tenant_id, &c.user_id) {
                                Some(u) => (c.user_id, c.tenant_id, c.password_hash, u.email.clone(), u.status.clone(), u.roles.clone()),
                                None => {
                                    return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                                }
                            }
                        },
                        None => {
                            return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                        }
                    }
                }
            }
        } else {
            match auth_stores.credentials.find(body.username.trim()) {
                Some(c) => {
                    tracing::info!("[LOGIN] ✅ Credentials found in in-memory store");
                    match services.users_get(c.tenant_id, &c.user_id) {
                        Some(u) => (c.user_id, c.tenant_id, c.password_hash, u.email.clone(), u.status.clone(), u.roles.clone()),
                        None => {
                            tracing::warn!("[LOGIN] ⚠️ User profile not found");
                            return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                        }
                    }
                },
                None => {
                    tracing::warn!("[LOGIN] ❌ Credentials NOT found for username: {}", body.username.trim());
                    return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
                }
            }
        };

    // bcrypt::verify adalah CPU-intensive; offload ke blocking thread pool
    // agar tidak memblokir Tokio async worker thread.
    let password_input = body.password.clone();
    let hash_for_verify = password_hash.clone();
    let password_valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password_input, &hash_for_verify).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !password_valid {
        tracing::warn!("[LOGIN] ❌ Password verification failed for username: {}", body.username.trim());
        return errors::json_error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid credentials");
    }

    tracing::info!("[LOGIN] ✅ Password verified for username: {}", body.username.trim());

    // At this point, we have all data from either database (preferred) or in-memory

    if status == "Suspended" {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", "account is suspended");
    }

    let roles: Vec<Role> = roles_vec.iter().map(|r| Role::new(r.clone())).collect();

    let token = match issue_token(&auth_config, creds_user_id, creds_tenant_id, &roles) {
        Ok(t) => t,
        Err(_) => return errors::json_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "failed to issue token"),
    };

    let tenant_name = auth_stores.tenants.get_name(creds_tenant_id)
        .unwrap_or_else(|| "Workspace".to_string());

    (StatusCode::OK, Json(LoginResponse {
        token,
        user: UserInfo {
            id: creds_user_id.to_string(),
            username: body.username.trim().to_string(),
            email,
            roles: roles_vec,
            display_name: body.username.trim().to_string(),
            tenant_id: creds_tenant_id.to_string(),
        },
        tenant: TenantInfo {
            id: creds_tenant_id.to_string(),
            name: tenant_name,
        },
    })).into_response()
}

/// Logout endpoint - client should clear token.
async fn logout() -> StatusCode {
    // In a real implementation with server-side sessions,
    // you would invalidate the session here.
    // With JWT tokens, the client simply deletes the token.
    StatusCode::NO_CONTENT
}

// ─── JWT helper ──────────────────────────────────────────────────────────────

fn issue_token(
    config: &AuthConfig,
    user_id: UserId,
    tenant_id: TenantId,
    roles: &[Role],
) -> anyhow::Result<String> {
    let now = Utc::now();
    let claims = JwtClaims {
        sub: PrincipalId::from(*user_id.as_uuid()),
        tenant_id,
        roles: roles.to_vec(),
        issued_at: now,
        expires_at: now + Duration::hours(config.token_expiry_hours),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&config.jwt_secret),
    )?;
    Ok(token)
}