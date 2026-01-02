use axum::{
    Router,
    http::StatusCode,
    routing::get,
};
use tower::ServiceBuilder;

#[tokio::main]
async fn main() {
    forgeerp_observability::init();

    let app = Router::new()
        .route("/health", get(health))
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


