use chrono::{Duration as ChronoDuration, Utc};
use forgeerp_auth::{JwtClaims, PrincipalId, Role};
use forgeerp_core::TenantId;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::StatusCode;
use serde_json::json;

struct TestServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn spawn(jwt_secret: &str) -> Self {
        // Build app (same router as prod), but bind to an ephemeral port.
        let app = forgeerp_api::app::build_app(jwt_secret.to_string());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind ephemeral port");
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self { base_url, handle }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn mint_jwt(jwt_secret: &str, tenant_id: TenantId, roles: Vec<Role>) -> String {
    let now = Utc::now();
    let claims = JwtClaims {
        sub: PrincipalId::new(),
        tenant_id,
        roles,
        issued_at: now,
        expires_at: now + ChronoDuration::minutes(10),
    };

    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .expect("failed to encode jwt")
}

async fn get_item_eventually(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    id: &str,
) -> serde_json::Value {
    // The API is intentionally eventual-consistent (command path vs projection update).
    // Poll briefly until the projection catches up.
    for _ in 0..50 {
        let res = client
            .get(format!("{}/inventory/items/{}", base_url, id))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();

        if res.status() == StatusCode::OK {
            return res.json().await.unwrap();
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    panic!("item did not become visible in projection within timeout");
}

#[tokio::test]
async fn auth_required_for_protected_endpoints() {
    let jwt_secret = "test-secret";
    let srv = TestServer::spawn(jwt_secret).await;

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/whoami", srv.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tenant_context_is_derived_from_token() {
    let jwt_secret = "test-secret";
    let srv = TestServer::spawn(jwt_secret).await;

    let tenant_id = TenantId::new();
    let token = mint_jwt(jwt_secret, tenant_id, vec![Role::new("admin")]);

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/whoami", srv.base_url))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["tenant_id"].as_str().unwrap(), tenant_id.to_string());
    assert!(body["roles"].as_array().unwrap().iter().any(|r| r == "admin"));
}

#[tokio::test]
async fn inventory_lifecycle_create_adjust_query() {
    let jwt_secret = "test-secret";
    let srv = TestServer::spawn(jwt_secret).await;

    let tenant_id = TenantId::new();
    let token = mint_jwt(jwt_secret, tenant_id, vec![Role::new("admin")]);

    let client = reqwest::Client::new();

    // Create
    let res = client
        .post(format!("{}/inventory/items", srv.base_url))
        .bearer_auth(&token)
        .json(&json!({ "name": "Widget" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created: serde_json::Value = res.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // Adjust
    let res = client
        .post(format!("{}/inventory/items/{}/adjust", srv.base_url, id))
        .bearer_auth(&token)
        .json(&json!({ "delta": 10 }))
        .send()
        .await
        .unwrap();
    if res.status() != StatusCode::OK {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        panic!("expected 200 OK from adjust, got {status} body={body}");
    }

    // Query (eventually consistent with projection)
    let item = get_item_eventually(&client, &srv.base_url, &token, &id).await;
    assert_eq!(item["name"], "Widget");
    assert_eq!(item["quantity"], 10);
}

#[tokio::test]
async fn unauthorized_access_blocked_for_commands() {
    let jwt_secret = "test-secret";
    let srv = TestServer::spawn(jwt_secret).await;

    let tenant_id = TenantId::new();
    // Not admin => permission mapping returns empty => forbidden for commands.
    let token = mint_jwt(jwt_secret, tenant_id, vec![Role::new("viewer")]);

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/inventory/items", srv.base_url))
        .bearer_auth(&token)
        .json(&json!({ "name": "Widget" }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tenant_isolation_blocks_cross_tenant_reads_and_writes() {
    let jwt_secret = "test-secret";
    let srv = TestServer::spawn(jwt_secret).await;

    let tenant1 = TenantId::new();
    let tenant2 = TenantId::new();
    let token1 = mint_jwt(jwt_secret, tenant1, vec![Role::new("admin")]);
    let token2 = mint_jwt(jwt_secret, tenant2, vec![Role::new("admin")]);

    let client = reqwest::Client::new();

    // Tenant1 creates an item
    let res = client
        .post(format!("{}/inventory/items", srv.base_url))
        .bearer_auth(&token1)
        .json(&json!({ "name": "Widget" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created: serde_json::Value = res.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // Tenant2 cannot read it (projection lookup is tenant-scoped)
    let res = client
        .get(format!("{}/inventory/items/{}", srv.base_url, id))
        .bearer_auth(&token2)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Tenant2 cannot adjust it either (dispatch happens under tenant2 context)
    let res = client
        .post(format!("{}/inventory/items/{}/adjust", srv.base_url, id))
        .bearer_auth(&token2)
        .json(&json!({ "delta": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}


