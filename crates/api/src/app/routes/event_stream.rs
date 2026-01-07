//! Event stream dashboard endpoint for operational visibility.
//!
//! Provides real-time SSE stream of events for debugging and operational monitoring.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use forgeerp_auth::admin;
use forgeerp_events::EventBus;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;
use crate::context::{PrincipalContext, TenantContext};

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn router() -> Router {
    Router::new().route("/events", get(stream_events))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /admin/stream/events
/// 
/// Stream events in real-time via Server-Sent Events (SSE).
/// 
/// Events are filtered to the authenticated tenant and include:
/// - event_id
/// - tenant_id
/// - aggregate_id
/// - aggregate_type
/// - sequence_number
/// - event_type (from payload metadata if available)
/// - occurred_at timestamp
/// - Payload preview (first 500 chars)
pub async fn stream_events(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<PrincipalContext>,
) -> axum::response::Response {
    // Check permission
    let cmd_auth = CmdAuth::<()> {
        inner: (),
        required: vec![admin::USER_READ.clone()], // Admin-only for event streaming
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return (
            StatusCode::FORBIDDEN,
            format!("Forbidden: {}", e),
        )
            .into_response();
    }

    let tenant_id = tenant.tenant_id();

    // Create unbounded channel for streaming SSE events
    let (tx, rx) = unbounded_channel::<Result<SseEvent, std::convert::Infallible>>();

    // Spawn a blocking task that subscribes to the event bus and forwards events into the channel
    let services_clone = services.clone();
    tokio::task::spawn_blocking(move || {
        let mut last_heartbeat = std::time::Instant::now();

        // Subscribe to event bus based on services type
        let subscription = match &*services_clone {
            AppServices::InMemory { event_bus, .. } => {
                event_bus.subscribe()
            }
            #[cfg(feature = "redis")]
            AppServices::Persistent { event_bus, .. } => {
                // For Redis Streams, use a unique consumer group for this stream
                let consumer_name = format!("event-stream-{}", uuid::Uuid::now_v7());
                event_bus.subscribe_with_group("event.stream.dashboard", &consumer_name, Some(tenant_id))
            }
        };

        // Forward events to unbounded channel
        loop {
            match subscription.recv_timeout(Duration::from_millis(1000)) {
                Ok(envelope) => {
                    // Filter by tenant
                    if envelope.tenant_id() != tenant_id {
                        continue;
                    }

                    // Extract event metadata
                    let event_metadata = serde_json::json!({
                        "event_id": envelope.event_id().to_string(),
                        "tenant_id": envelope.tenant_id().to_string(),
                        "aggregate_id": envelope.aggregate_id().to_string(),
                        "aggregate_type": envelope.aggregate_type(),
                        "sequence_number": envelope.sequence_number(),
                        "occurred_at": chrono::Utc::now().to_rfc3339(),
                        "payload_preview": truncate_payload(envelope.payload()),
                    });

                    // Try to extract event_type from payload
                    let event_type = extract_event_type(envelope.payload());
                    let mut event_data = event_metadata;
                    if let Some(evt_type) = event_type {
                        event_data
                            .as_object_mut()
                            .unwrap()
                            .insert("event_type".to_string(), serde_json::Value::String(evt_type));
                    }

                    let json_str = match serde_json::to_string(&event_data) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    let sse_event = SseEvent::default().event("event").data(json_str);

                    if tx.send(Ok(sse_event)).is_err() {
                        break; // Receiver dropped
                    }

                    last_heartbeat = std::time::Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Send heartbeat every 15 seconds to keep connection alive
                    if last_heartbeat.elapsed() > Duration::from_secs(15) {
                        let heartbeat = SseEvent::default().event("heartbeat").data("{}");
                        if tx.send(Ok(heartbeat)).is_err() {
                            break;
                        }
                        last_heartbeat = std::time::Instant::now();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break; // Bus closed
                }
            }
        }
    });

    let stream = UnboundedReceiverStream::new(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Truncate payload to first 500 characters for preview.
fn truncate_payload(payload: &JsonValue) -> String {
    let json_str = payload.to_string();
    if json_str.len() > 500 {
        format!("{}...", &json_str[..500])
    } else {
        json_str
    }
}

/// Extract event_type from payload if it matches known structures.
/// Many domain events have an event_type field or are variants with a name.
fn extract_event_type(payload: &JsonValue) -> Option<String> {
    // Try to get event_type field directly
    if let Some(obj) = payload.as_object() {
        if let Some(evt_type) = obj.get("event_type") {
            if let Some(s) = evt_type.as_str() {
                return Some(s.to_string());
            }
        }
        // Try common variant names (e.g., "ProductCreated", "InvoiceIssued")
        if obj.len() == 1 {
            if let Some((key, _)) = obj.iter().next() {
                // Check if it looks like an event variant (starts with capital letter)
                if key.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    return Some(key.clone());
                }
            }
        }
    }
    None
}


