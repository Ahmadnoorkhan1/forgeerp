use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

use crate::models::{InventoryItem, SessionTenant, SessionUser};

const BASE: &str = "http://localhost:8080";

// ─── Auth request / response DTOs ────────────────────────────────────────────

#[derive(Serialize)]
pub struct RegisterRequest {
    pub username:     String,
    pub email:        String,
    pub password:     String,
    pub invite_token: Option<String>,
}

// ─── Inventory request DTOs ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CreateItemRequest {
    pub tenant_id: String,
    pub item_id:   String,
    pub name:      String,
}

#[derive(Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct AuthResponse {
    pub token:  String,
    pub user:   AuthUser,
    pub tenant: AuthTenant,
}

#[derive(Deserialize)]
pub struct AuthUser {
    pub id:       String,
    pub username: String,
    pub email:    String,
    pub roles:    Vec<String>,
}

#[derive(Deserialize)]
pub struct AuthTenant {
    pub id:   String,
    pub name: String,
}

impl From<AuthResponse> for (String, SessionUser, SessionTenant) {
    fn from(r: AuthResponse) -> Self {
        (
            r.token,
            SessionUser {
                id:       r.user.id,
                username: r.user.username,
                email:    r.user.email,
                roles:    r.user.roles,
            },
            SessionTenant { id: r.tenant.id, name: r.tenant.name },
        )
    }
}

// ─── Inventory response ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InventoryListResponse {
    items: Vec<InventoryItemRaw>,
}

#[derive(Deserialize)]
struct InventoryItemRaw {
    id:       String,
    name:     String,
    quantity: i64,
}

// ─── Auth calls ───────────────────────────────────────────────────────────────

pub async fn register(req: RegisterRequest) -> Result<AuthResponse, String> {
    json_post(&format!("{}/auth/register", BASE), &req, None).await
}

pub async fn login(req: LoginRequest) -> Result<AuthResponse, String> {
    json_post(&format!("{}/auth/login", BASE), &req, None).await
}

// ─── Inventory calls ──────────────────────────────────────────────────────────

pub async fn list_inventory(token: &str) -> Result<Vec<InventoryItem>, String> {
    let resp = authed_get(&format!("{}/inventory/items", BASE), token).await?;
    let raw: InventoryListResponse = resp;
    Ok(raw.items
        .into_iter()
        .map(|i| InventoryItem { id: i.id, name: i.name, quantity: i.quantity })
        .collect())
}

pub async fn adjust_stock(item_id: &str, delta: i64, token: &str) -> Result<(), String> {
    let body = serde_json::json!({ "delta": delta });
    let url = format!("{}/inventory/items/{}/adjust", BASE, item_id);
    let _: serde_json::Value = json_post(&url, &body, Some(token)).await?;
    Ok(())
}

pub async fn create_item(req: CreateItemRequest, token: &str) -> Result<(), String> {
    let _: serde_json::Value = json_post(&format!("{}/inventory/items", BASE), &req, Some(token)).await?;
    Ok(())
}

pub async fn health_check() -> bool {
    Request::get(&format!("{}/health", BASE))
        .send()
        .await
        .map(|r| r.ok())
        .unwrap_or(false)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn json_post<B, R>(url: &str, body: &B, token: Option<&str>) -> Result<R, String>
where
    B: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    let body_str = serde_json::to_string(body).map_err(|e| e.to_string())?;
    let mut req = Request::post(url).header("Content-Type", "application/json");
    if let Some(tok) = token {
        req = req.header("Authorization", &format!("Bearer {}", tok));
    }
    let resp = req
        .body(body_str)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status() == 401 {
        return Err("SESSION_EXPIRED".to_string());
    }
    if resp.ok() {
        resp.json::<R>().await.map_err(|e| e.to_string())
    } else {
        let v: serde_json::Value = resp.json().await.unwrap_or_default();
        Err(v["message"].as_str().unwrap_or("Request failed").to_string())
    }
}

async fn authed_get<R>(url: &str, token: &str) -> Result<R, String>
where
    R: serde::de::DeserializeOwned,
{
    let resp = Request::get(url)
        .header("Authorization", &format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status() == 401 {
        return Err("SESSION_EXPIRED".to_string());
    }
    if resp.ok() {
        resp.json::<R>().await.map_err(|e| e.to_string())
    } else {
        let v: serde_json::Value = resp.json().await.unwrap_or_default();
        Err(v["message"].as_str().unwrap_or("Request failed").to_string())
    }
}
