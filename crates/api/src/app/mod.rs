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

use crate::middleware;

pub mod dto;
pub mod errors;
pub mod routes;
pub mod services;

/// Build the full HTTP router (public entrypoint used by `main.rs`).
pub async fn build_app(jwt_secret: String) -> Router {
    let jwt = Arc::new(forgeerp_auth::Hs256JwtValidator::new(jwt_secret.into_bytes()));
    let auth_state = middleware::AuthState { jwt };

    let services = Arc::new(services::build_services().await);
    let replay_jobs = routes::replay::ReplayJobStore::new();

    // Protected routes: require auth + tenant context.
    let protected = routes::router()
        .layer(Extension(services))
        .layer(Extension(replay_jobs))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            middleware::auth_middleware,
        ));

    Router::new()
        .route("/health", get(routes::system::health))
        .merge(protected)
        .layer(ServiceBuilder::new())
}


