//! HTTP API application wiring (Axum router + service wiring).
//!
//! If you're new to Rust, this folder is structured like:
//! - `services.rs`: infrastructure wiring (event store/bus, projections, dispatcher)
//! - `routes/`: HTTP routes + handlers (one file per domain area)
//! - `dto.rs`: request/response DTOs and JSON mapping helpers
//! - `errors.rs`: consistent error responses

use std::sync::Arc;

use axum::{routing::get, Extension, Router};
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

use crate::middleware;
use forgeerp_infra::projections::DatabaseCredentialStore;
use sqlx::PgPool;

pub mod dto;
pub mod errors;
pub mod routes;
pub mod services;

/// Wrapper for optional database credential store
#[derive(Clone)]
pub struct OptionalDbCredentialStore(pub Option<Arc<DatabaseCredentialStore>>);

/// Build the full HTTP router (public entrypoint used by `main.rs`).
pub async fn build_app(jwt_secret: String) -> Router {
    let jwt_bytes = jwt_secret.clone().into_bytes();
    let jwt = Arc::new(forgeerp_auth::Hs256JwtValidator::new(jwt_bytes.clone()));
    let auth_state = middleware::AuthState { jwt };

    let services = Arc::new(services::build_services().await);
    let replay_jobs = routes::replay::ReplayJobStore::new();

    // Auth stores (credentials, tenants, invites) — shared by public + protected routes.
    let auth_stores = Arc::new(services::AuthStores::new());

    let auth_config = Arc::new(routes::auth::AuthConfig::new(jwt_bytes));

    // Create database credential store if DATABASE_URL is available
    let db_credential_store = if let Ok(database_url) = std::env::var("DATABASE_URL") {
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(20)  // Sufficient for typical workloads
            .acquire_timeout(std::time::Duration::from_secs(5))
            .idle_timeout(Some(std::time::Duration::from_secs(300)))  // 5 min idle timeout
            .connect(&database_url)
            .await {
            Ok(pool) => {
                tracing::info!("✅ Connected to database for credential store (pool size: 20)");
                OptionalDbCredentialStore(Some(Arc::new(DatabaseCredentialStore::new(pool))))
            },
            Err(e) => {
                tracing::warn!("⚠️ Failed to connect to database for credential store: {:?}", e);
                OptionalDbCredentialStore(None)
            }
        }
    } else {
        tracing::info!("ℹ️ DATABASE_URL not set, using in-memory credential store only");
        OptionalDbCredentialStore(None)
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Public routes: /auth/register, /auth/login — no authentication required.
    let public = routes::public_router()
        .layer(Extension(services.clone()))
        .layer(Extension(auth_stores.clone()))
        .layer(Extension(auth_config))
        .layer(Extension(db_credential_store));

    // Protected routes: require auth + tenant context.
    let protected = routes::protected_router()
        .layer(Extension(services))
        .layer(Extension(auth_stores))
        .layer(Extension(replay_jobs))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            middleware::auth_middleware,
        ));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(ServiceBuilder::new())
        .layer(cors)
}
