use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;

use forgeerp_auth::Permission;
use forgeerp_core::AggregateId;
use forgeerp_parties::{Party, PartyCommand, PartyId, PartyKind, RegisterParty, SuspendParty, UpdateDetails};

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new()
        .route("/", post(register_supplier).get(list_suppliers))
        .route("/:id", get(get_supplier).patch(update_supplier))
        .route("/:id/suspend", post(suspend_supplier))
}

pub async fn register_supplier(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::RegisterPartyRequest>,
) -> axum::response::Response {
    register_party(services, tenant, principal, PartyKind::Supplier, "suppliers.register", body).await
}

pub async fn update_supplier(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::UpdatePartyRequest>,
) -> axum::response::Response {
    update_party(services, tenant, principal, id, body, PartyKind::Supplier, "suppliers.update").await
}

pub async fn suspend_supplier(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::SuspendPartyRequest>,
) -> axum::response::Response {
    suspend_party(services, tenant, principal, id, body, PartyKind::Supplier, "suppliers.suspend").await
}

pub async fn get_supplier(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    get_party_by_kind(services, tenant, id, PartyKind::Supplier).await
}

pub async fn list_suppliers(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .parties_list(tenant.tenant_id())
        .into_iter()
        .filter(|p| p.kind == PartyKind::Supplier)
        .map(dto::party_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}

async fn register_party(
    services: Arc<AppServices>,
    tenant: crate::context::TenantContext,
    principal: crate::context::PrincipalContext,
    kind: PartyKind,
    perm: &'static str,
    body: dto::RegisterPartyRequest,
) -> axum::response::Response {
    let agg = AggregateId::new();
    let party_id = PartyId::new(agg);

    let cmd = PartyCommand::RegisterParty(RegisterParty {
        tenant_id: tenant.tenant_id(),
        party_id,
        kind,
        name: body.name,
        contact: body.contact,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new(perm)],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Party>(
        tenant.tenant_id(),
        agg,
        "parties.party",
        cmd_auth.inner,
        |_t, aggregate_id| Party::empty(PartyId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": agg.to_string(),
            "kind": "supplier",
            "events_committed": committed.len(),
        })),
    )
        .into_response()
}

async fn update_party(
    services: Arc<AppServices>,
    tenant: crate::context::TenantContext,
    principal: crate::context::PrincipalContext,
    id: String,
    body: dto::UpdatePartyRequest,
    kind: PartyKind,
    perm: &'static str,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid party id"),
    };
    let party_id = PartyId::new(agg);

    if let Some(rm) = services.parties_get(tenant.tenant_id(), &party_id) {
        if rm.kind != kind {
            return errors::json_error(StatusCode::NOT_FOUND, "not_found", "party not found");
        }
    }

    let cmd = PartyCommand::UpdateDetails(UpdateDetails {
        tenant_id: tenant.tenant_id(),
        party_id,
        name: body.name,
        contact: body.contact,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new(perm)],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Party>(
        tenant.tenant_id(),
        agg,
        "parties.party",
        cmd_auth.inner,
        |_t, aggregate_id| Party::empty(PartyId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

async fn suspend_party(
    services: Arc<AppServices>,
    tenant: crate::context::TenantContext,
    principal: crate::context::PrincipalContext,
    id: String,
    body: dto::SuspendPartyRequest,
    kind: PartyKind,
    perm: &'static str,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid party id"),
    };
    let party_id = PartyId::new(agg);

    if let Some(rm) = services.parties_get(tenant.tenant_id(), &party_id) {
        if rm.kind != kind {
            return errors::json_error(StatusCode::NOT_FOUND, "not_found", "party not found");
        }
    }

    let cmd = PartyCommand::SuspendParty(SuspendParty {
        tenant_id: tenant.tenant_id(),
        party_id,
        reason: body.reason,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new(perm)],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Party>(
        tenant.tenant_id(),
        agg,
        "parties.party",
        cmd_auth.inner,
        |_t, aggregate_id| Party::empty(PartyId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

async fn get_party_by_kind(
    services: Arc<AppServices>,
    tenant: crate::context::TenantContext,
    id: String,
    kind: PartyKind,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid party id"),
    };
    let party_id = PartyId::new(agg);
    match services.parties_get(tenant.tenant_id(), &party_id) {
        Some(rm) if rm.kind == kind => (StatusCode::OK, Json(dto::party_to_json(rm))).into_response(),
        _ => errors::json_error(StatusCode::NOT_FOUND, "not_found", "party not found"),
    }
}


