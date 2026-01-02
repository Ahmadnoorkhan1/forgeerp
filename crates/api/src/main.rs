use axum::{
    Router,
    http::StatusCode,
    routing::get,
    response::IntoResponse,
    Json,
};
use tower::ServiceBuilder;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    forgeerp_observability::init();

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("JWT_SECRET not set; using insecure dev default");
        "dev-secret".to_string()
    });

    let jwt = Arc::new(forgeerp_auth::Hs256JwtValidator::new(jwt_secret.into_bytes()));
    let auth_state = forgeerp_api::middleware::AuthState { jwt };

    // Protected routes: require auth + tenant context.
    let protected = Router::new()
        .route("/whoami", get(whoami))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            forgeerp_api::middleware::auth_middleware,
        ));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(ServiceBuilder::new());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind 0.0.0.0:8080");

    tracing::info!("listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn whoami(
    axum::extract::Extension(tenant): axum::extract::Extension<forgeerp_api::context::TenantContext>,
    axum::extract::Extension(principal): axum::extract::Extension<forgeerp_api::context::PrincipalContext>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "tenant_id": tenant.tenant_id().to_string(),
        "principal_id": principal.principal_id().to_string(),
        "roles": principal.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>(),
    }))
}


