use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;

use forgeerp_accounting::{JournalCommand, Ledger, LedgerId, PostJournalEntry};
use forgeerp_auth::Permission;

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new()
        .route("/balances", get(list_ledger_balances))
        .route("/balances/:code", get(get_ledger_balance))
        .route("/journal", post(post_journal_entry))
}

pub async fn list_ledger_balances(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .ledger_balances_list(tenant.tenant_id())
        .into_iter()
        .map(dto::ledger_balance_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}

pub async fn get_ledger_balance(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(code): Path<String>,
) -> axum::response::Response {
    match services.ledger_balance_get(tenant.tenant_id(), &code) {
        Some(b) => (StatusCode::OK, Json(dto::ledger_balance_to_json(b))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "account not found"),
    }
}

pub async fn post_journal_entry(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::PostJournalEntryRequest>,
) -> axum::response::Response {
    if body.lines.is_empty() {
        return errors::json_error(StatusCode::BAD_REQUEST, "validation", "journal entry must have lines");
    }

    let lines = match dto::to_journal_lines(body.lines) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let ledger_agg = services.default_ledger_id();
    let ledger_id = LedgerId::new(ledger_agg);

    let cmd = JournalCommand::PostJournalEntry(PostJournalEntry {
        tenant_id: tenant.tenant_id(),
        ledger_id,
        entry_id: uuid::Uuid::now_v7(),
        lines,
        occurred_at: Utc::now(),
        description: body.description,
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("ledger.post")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Ledger>(
        tenant.tenant_id(),
        ledger_agg,
        "accounting.ledger",
        cmd_auth.inner,
        |_t, aggregate_id| Ledger::empty(LedgerId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({"ledger_id": ledger_agg.to_string(), "events_committed": committed.len()})),
    )
        .into_response()
}


