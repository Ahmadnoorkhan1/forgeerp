use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::Utc;

use forgeerp_auth::JwtValidator;

use crate::context::{PrincipalContext, TenantContext};

#[derive(Clone)]
pub struct AuthState {
    pub jwt: Arc<dyn JwtValidator>,
}

pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_bearer(req.headers())?;

    let claims = state
        .jwt
        .validate(token, Utc::now())
        .map_err(|_e| StatusCode::UNAUTHORIZED)?;

    req.extensions_mut()
        .insert(TenantContext::new(claims.tenant_id));
    req.extensions_mut().insert(PrincipalContext::new(
        claims.sub,
        claims.roles.clone(),
    ));

    Ok(next.run(req).await)
}

fn extract_bearer(headers: &HeaderMap) -> Result<&str, StatusCode> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let header = header.to_str().map_err(|_| StatusCode::UNAUTHORIZED)?;

    let header = header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = header.trim();
    if token.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(token)
}

