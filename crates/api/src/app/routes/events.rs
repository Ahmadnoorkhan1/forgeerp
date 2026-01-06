//! Event inspection endpoints for operator-grade visibility.
//!
//! These endpoints provide read-only access to event streams for debugging,
//! auditing, and operational visibility.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use forgeerp_auth::admin;
use forgeerp_core::AggregateId;
use forgeerp_infra::event_store::{EventFilter, Pagination, StoredEvent};

use crate::app::{errors, services::AppServices};
use crate::app::routes::common::CmdAuth;
use crate::context::{PrincipalContext, TenantContext};

// ─────────────────────────────────────────────────────────────────────────────
// Query Parameters
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EventListQuery {
    pub aggregate_id: Option<String>,
    pub aggregate_type: Option<String>,
    pub event_type: Option<String>,
    pub occurred_after: Option<DateTime<Utc>>,
    pub occurred_before: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new()
        .route("/", get(list_events))
        .route("/aggregates/:id", get(get_aggregate_events))
        .route("/:event_id", get(get_event))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /admin/events?aggregate_id=X&event_type=Y&limit=50&offset=0
/// 
/// List events with optional filters and pagination.
/// 
/// Query parameters:
/// - `aggregate_id`: Filter by aggregate ID (UUID)
/// - `aggregate_type`: Filter by aggregate type (e.g., "inventory.item")
/// - `event_type`: Filter by event type (e.g., "inventory.item.created")
/// - `occurred_after`: Filter events after this timestamp (ISO 8601)
/// - `occurred_before`: Filter events before this timestamp (ISO 8601)
/// - `limit`: Maximum number of events to return (default: 50, max: 1000)
/// - `offset`: Pagination offset (default: 0)
pub async fn list_events(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Query(query): Query<EventListQuery>,
) -> axum::response::Response {
    // Check permission (admin only for event inspection)
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    // Build filter
    let aggregate_id = query.aggregate_id.and_then(|s| {
        s.parse::<uuid::Uuid>()
            .ok()
            .map(|uuid| AggregateId::from_uuid(uuid))
    });

    let filter = EventFilter {
        aggregate_id,
        aggregate_type: query.aggregate_type,
        event_type: query.event_type,
        occurred_after: query.occurred_after,
        occurred_before: query.occurred_before,
    };

    let pagination = Pagination::new(query.limit, query.offset);

    match services.query_events(tenant.tenant_id(), filter, pagination).await {
        Ok(result) => {
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "events": result.events.iter().map(event_to_json).collect::<Vec<_>>(),
                    "total": result.total,
                    "pagination": {
                        "limit": result.pagination.limit,
                        "offset": result.pagination.offset,
                    },
                    "has_more": result.has_more,
                })),
            )
                .into_response()
        }
        Err(e) => errors::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "query_failed",
            format!("Failed to query events: {}", e),
        ),
    }
}

/// GET /admin/events/aggregates/:id?limit=50&offset=0
/// 
/// Get all events for a specific aggregate, ordered by sequence number.
pub async fn get_aggregate_events(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(aggregate_id_str): Path<String>,
    Query(query): Query<EventListQuery>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let aggregate_id: AggregateId = match aggregate_id_str.parse::<uuid::Uuid>() {
        Ok(uuid) => AggregateId::from_uuid(uuid),
        Err(_) => {
            return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid aggregate id");
        }
    };

    let pagination = Pagination::new(query.limit, query.offset);

    match services
        .get_aggregate_events(tenant.tenant_id(), aggregate_id, Some(pagination))
        .await
    {
        Ok(result) => {
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "aggregate_id": aggregate_id_str,
                    "events": result.events.iter().map(event_to_json).collect::<Vec<_>>(),
                    "total": result.total,
                    "pagination": {
                        "limit": result.pagination.limit,
                        "offset": result.pagination.offset,
                    },
                    "has_more": result.has_more,
                })),
            )
                .into_response()
        }
        Err(e) => errors::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "query_failed",
            format!("Failed to query events: {}", e),
        ),
    }
}

/// GET /admin/events/:event_id
/// 
/// Get a single event by its ID.
pub async fn get_event(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
    Path(event_id_str): Path<String>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let event_id = match event_id_str.parse::<uuid::Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid event id");
        }
    };

    match services.get_event_by_id(tenant.tenant_id(), event_id).await {
        Ok(Some(event)) => (StatusCode::OK, Json(event_to_json(&event))).into_response(),
        Ok(None) => errors::json_error(StatusCode::NOT_FOUND, "not_found", "event not found"),
        Err(e) => errors::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "query_failed",
            format!("Failed to query event: {}", e),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn event_to_json(event: &StoredEvent) -> serde_json::Value {
    serde_json::json!({
        "event_id": event.event_id.to_string(),
        "tenant_id": event.tenant_id.to_string(),
        "aggregate_id": event.aggregate_id.to_string(),
        "aggregate_type": event.aggregate_type,
        "sequence_number": event.sequence_number,
        "event_type": event.event_type,
        "event_version": event.event_version,
        "occurred_at": event.occurred_at.to_rfc3339(),
        "payload": event.payload,
    })
}

