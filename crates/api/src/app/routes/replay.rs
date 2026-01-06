//! Projection replay endpoints for rebuilding read models.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_auth::admin;
use forgeerp_core::TenantId;
use forgeerp_infra::projections::replay::{ReplayHandle, ReplayProgress};

use crate::app::{errors, services::AppServices};
use crate::app::routes::common::CmdAuth;
use crate::context::{PrincipalContext, TenantContext};

// ─────────────────────────────────────────────────────────────────────────────
// Job Store
// ─────────────────────────────────────────────────────────────────────────────

/// In-memory job store for tracking replay operations.
#[derive(Clone, Default)]
pub struct ReplayJobStore {
    jobs: Arc<RwLock<HashMap<Uuid, ReplayHandle>>>,
}

impl ReplayJobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, job_id: Uuid, handle: ReplayHandle) {
        self.jobs.write().await.insert(job_id, handle);
    }

    pub async fn get(&self, job_id: &Uuid) -> Option<ReplayHandle> {
        self.jobs.read().await.get(job_id).cloned()
    }

    pub async fn list(&self) -> Vec<Uuid> {
        self.jobs.read().await.keys().cloned().collect()
    }

    pub async fn remove(&self, job_id: &Uuid) {
        self.jobs.write().await.remove(job_id);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Request/Response Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ReplayRequest {
    pub projection: Option<String>, // If None, replay all projections
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ReplayResponse {
    pub job_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ReplayStatusResponse {
    pub job_id: String,
    pub progress: ReplayProgress,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new()
        .route("/projections/:projection", post(start_replay))
        .route("/projections", post(start_all_replays))
        .route("/jobs/:job_id", get(get_replay_status))
        .route("/jobs/:job_id", axum::routing::delete(cancel_replay))
        .route("/jobs", get(list_replays))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// POST /admin/replay/projections/:projection
/// 
/// Start replaying a single projection.
pub async fn start_replay(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(job_store): Extension<ReplayJobStore>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(projection_name): Path<String>,
    Query(query): Query<ReplayRequest>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()], // Using admin permission for now
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let dry_run = query.dry_run.unwrap_or(false);
    let job_id = Uuid::now_v7();

    // Map projection name to configuration
    let (aggregate_types, apply_fn, clear_fn) = match projection_name.as_str() {
        "inventory" => (
            vec!["inventory.item".to_string()],
            create_inventory_apply_fn(&services, tenant.tenant_id()),
            create_inventory_clear_fn(&services, tenant.tenant_id()),
        ),
        "products" => (
            vec!["products.product".to_string()],
            create_products_apply_fn(&services, tenant.tenant_id()),
            create_products_clear_fn(&services, tenant.tenant_id()),
        ),
        "parties" => (
            vec!["parties.party".to_string()],
            create_parties_apply_fn(&services, tenant.tenant_id()),
            create_parties_clear_fn(&services, tenant.tenant_id()),
        ),
        "sales" => (
            vec!["sales.order".to_string()],
            create_sales_apply_fn(&services, tenant.tenant_id()),
            create_sales_clear_fn(&services, tenant.tenant_id()),
        ),
        "invoices" => (
            vec!["invoicing.invoice".to_string()],
            create_invoices_apply_fn(&services, tenant.tenant_id()),
            create_invoices_clear_fn(&services, tenant.tenant_id()),
        ),
        "purchases" => (
            vec!["purchasing.order".to_string()],
            create_purchases_apply_fn(&services, tenant.tenant_id()),
            create_purchases_clear_fn(&services, tenant.tenant_id()),
        ),
        _ => {
            return errors::json_error(
                StatusCode::BAD_REQUEST,
                "invalid_projection",
                format!("Unknown projection: {}", projection_name),
            );
        }
    };

    // Start replay - match on services type to get the right event store
    let handle_result = match &*services {
        AppServices::InMemory { event_store, .. } => {
            forgeerp_infra::projections::replay::replay_projection(
                event_store.clone(),
                tenant.tenant_id(),
                aggregate_types,
                apply_fn,
                clear_fn,
                dry_run,
            )
            .await
        }
        #[cfg(feature = "redis")]
        AppServices::Persistent { event_store, .. } => {
            forgeerp_infra::projections::replay::replay_projection(
                event_store.clone(),
                tenant.tenant_id(),
                aggregate_types,
                apply_fn,
                clear_fn,
                dry_run,
            )
            .await
        }
    };

    match handle_result {
        Ok(handle) => {
            job_store.insert(job_id, handle).await;
            (
                StatusCode::ACCEPTED,
                Json(ReplayResponse {
                    job_id: job_id.to_string(),
                    message: if dry_run {
                        format!("Dry-run replay started for projection: {}", projection_name)
                    } else {
                        format!("Replay started for projection: {}", projection_name)
                    },
                }),
            )
                .into_response()
        }
        Err(e) => errors::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "replay_failed",
            format!("Failed to start replay: {}", e),
        ),
    }
}

/// POST /admin/replay/projections
/// 
/// Start replaying all projections for the tenant.
pub async fn start_all_replays(
    Extension(_services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Query(_query): Query<ReplayRequest>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    // For now, return not implemented
    // In a real implementation, we'd start multiple replay jobs
    errors::json_error(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "Replaying all projections is not yet implemented",
    )
}

/// GET /admin/replay/jobs/:job_id
/// 
/// Get the status of a replay job.
pub async fn get_replay_status(
    Extension(job_store): Extension<ReplayJobStore>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(job_id_str): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let job_id = match job_id_str.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid job id");
        }
    };

    match job_store.get(&job_id).await {
        Some(handle) => {
            let progress = handle.progress().await;
            (
                StatusCode::OK,
                Json(ReplayStatusResponse {
                    job_id: job_id.to_string(),
                    progress,
                }),
            )
                .into_response()
        }
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "job not found"),
    }
}

/// DELETE /admin/replay/jobs/:job_id
/// 
/// Cancel a running replay job.
pub async fn cancel_replay(
    Extension(job_store): Extension<ReplayJobStore>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(job_id_str): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let job_id = match job_id_str.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid job id");
        }
    };

    match job_store.get(&job_id).await {
        Some(handle) => {
            handle.cancel();
            // Optionally remove from store after cancellation
            // job_store.remove(&job_id).await;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "job_id": job_id.to_string(),
                    "message": "Replay cancelled"
                })),
            )
                .into_response()
        }
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "job not found"),
    }
}

/// GET /admin/replay/jobs
/// 
/// List all active replay jobs.
pub async fn list_replays(
    Extension(job_store): Extension<ReplayJobStore>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let job_ids = job_store.list().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "jobs": job_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        })),
    )
        .into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

fn create_inventory_apply_fn(
    services: &AppServices,
    _tenant_id: TenantId,
) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { inventory_projection, .. } => {
                inventory_projection
                    .apply_envelope(envelope)
                    .map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { inventory_projection, .. } => {
                inventory_projection
                    .apply_envelope(envelope)
                    .map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_inventory_clear_fn(
    services: &AppServices,
    _tenant_id: TenantId,
) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        // rebuild_from_scratch extracts tenants from envelopes, so we need to pass
        // a dummy envelope with the tenant_id to trigger clearing.
        // Create a minimal dummy envelope just to trigger tenant clearing.
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "inventory.item".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { inventory_projection, .. } => {
                // rebuild_from_scratch will clear tenant state based on envelope tenant_id
                let _ = inventory_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { inventory_projection, .. } => {
                let _ = inventory_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

// Similar functions for other projections - using same pattern as inventory
fn create_products_apply_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { products_projection, .. } => {
                products_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { products_projection, .. } => {
                products_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_products_clear_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "products.product".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { products_projection, .. } => {
                let _ = products_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { products_projection, .. } => {
                let _ = products_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

fn create_parties_apply_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { parties_projection, .. } => {
                parties_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { parties_projection, .. } => {
                parties_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_parties_clear_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "parties.party".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { parties_projection, .. } => {
                let _ = parties_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { parties_projection, .. } => {
                let _ = parties_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

fn create_sales_apply_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { sales_projection, .. } => {
                sales_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { sales_projection, .. } => {
                sales_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_sales_clear_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "sales.order".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { sales_projection, .. } => {
                let _ = sales_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { sales_projection, .. } => {
                let _ = sales_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

fn create_invoices_apply_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { invoices_projection, .. } => {
                invoices_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { invoices_projection, .. } => {
                invoices_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_invoices_clear_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "invoicing.invoice".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { invoices_projection, .. } => {
                let _ = invoices_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { invoices_projection, .. } => {
                let _ = invoices_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

fn create_purchases_apply_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ApplyEnvelopeFn {
    let services_clone = services.clone();
    Arc::new(move |envelope| {
        match &services_clone {
            AppServices::InMemory { purchases_projection, .. } => {
                purchases_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { purchases_projection, .. } => {
                purchases_projection.apply_envelope(envelope).map_err(|e| format!("{}", e))
            }
        }
    })
}

fn create_purchases_clear_fn(services: &AppServices, _tenant_id: TenantId) -> forgeerp_infra::projections::replay::ClearTenantFn {
    let services_clone = services.clone();
    Arc::new(move |tenant_id| {
        use forgeerp_core::AggregateId;
        use forgeerp_events::EventEnvelope;
        use serde_json::json;
        
        let dummy_envelope = EventEnvelope::new(
            uuid::Uuid::now_v7(),
            tenant_id,
            AggregateId::from_uuid(uuid::Uuid::now_v7()),
            "purchasing.order".to_string(),
            1,
            json!({}),
        );
        
        match &services_clone {
            AppServices::InMemory { purchases_projection, .. } => {
                let _ = purchases_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { purchases_projection, .. } => {
                let _ = purchases_projection.rebuild_from_scratch(std::iter::once(dummy_envelope));
            }
        }
    })
}

