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
use forgeerp_invoicing::{
    Invoice, InvoiceCommand, InvoiceId, InvoiceLine, IssueInvoice, RegisterPayment, VoidInvoice,
};
use forgeerp_products::ProductId;
use forgeerp_sales::SalesOrderId;

use crate::app::{dto, errors};
use crate::app::routes::common::CmdAuth;
use crate::app::services::AppServices;

pub fn router() -> Router {
    Router::new()
        .route("/", post(issue_invoice).get(list_invoices))
        .route("/:id", get(get_invoice))
        .route("/:id/payments", post(register_invoice_payment))
        .route("/:id/void", post(void_invoice))
}

pub async fn issue_invoice(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Json(body): Json<dto::IssueInvoiceRequest>,
) -> axum::response::Response {
    let sales_order_agg: AggregateId = match body.sales_order_id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid sales_order_id"),
    };
    let sales_order_id = SalesOrderId::new(sales_order_agg);

    let due_date = match chrono::DateTime::parse_from_rfc3339(&body.due_date) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_due_date", "due_date must be RFC3339"),
    };

    let invoice_agg = AggregateId::new();
    let invoice_id = InvoiceId::new(invoice_agg);

    let mut lines: Vec<InvoiceLine> = Vec::new();
    for (idx, l) in body.lines.into_iter().enumerate() {
        let prod_agg: AggregateId = match l.product_id.parse() {
            Ok(v) => v,
            Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid product id"),
        };
        lines.push(InvoiceLine {
            line_no: (idx as u32) + 1,
            sales_order_id,
            product_id: ProductId::new(prod_agg),
            quantity: l.quantity,
            unit_price: l.unit_price,
        });
    }

    let cmd = InvoiceCommand::IssueInvoice(IssueInvoice {
        tenant_id: tenant.tenant_id(),
        invoice_id,
        sales_order_id,
        lines,
        due_date,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("invoices.issue")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Invoice>(
        tenant.tenant_id(),
        invoice_agg,
        "invoicing.invoice",
        cmd_auth.inner,
        |_t, aggregate_id| Invoice::empty(InvoiceId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::CREATED, Json(serde_json::json!({"id": invoice_agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn register_invoice_payment(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::RegisterPaymentRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid invoice id"),
    };
    let invoice_id = InvoiceId::new(agg);

    let cmd = InvoiceCommand::RegisterPayment(RegisterPayment {
        tenant_id: tenant.tenant_id(),
        invoice_id,
        amount: body.amount,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("invoices.pay")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Invoice>(
        tenant.tenant_id(),
        agg,
        "invoicing.invoice",
        cmd_auth.inner,
        |_t, aggregate_id| Invoice::empty(InvoiceId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn void_invoice(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Extension(principal): Extension<crate::context::PrincipalContext>,
    Path(id): Path<String>,
    Json(body): Json<dto::VoidInvoiceRequest>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid invoice id"),
    };
    let invoice_id = InvoiceId::new(agg);

    let cmd = InvoiceCommand::VoidInvoice(VoidInvoice {
        tenant_id: tenant.tenant_id(),
        invoice_id,
        reason: body.reason,
        occurred_at: Utc::now(),
    });

    let cmd_auth = CmdAuth {
        inner: cmd,
        required: vec![Permission::new("invoices.void")],
    };
    if let Err(e) = crate::authz::authorize_command(&tenant, &principal, &cmd_auth) {
        return errors::json_error(StatusCode::FORBIDDEN, "forbidden", e.to_string());
    }

    let committed = match services.dispatch::<Invoice>(
        tenant.tenant_id(),
        agg,
        "invoicing.invoice",
        cmd_auth.inner,
        |_t, aggregate_id| Invoice::empty(InvoiceId::new(aggregate_id)),
    ) {
        Ok(c) => c,
        Err(e) => return errors::dispatch_error_to_response(e),
    };

    (StatusCode::OK, Json(serde_json::json!({"id": agg.to_string(), "events_committed": committed.len()}))).into_response()
}

pub async fn get_invoice(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let agg: AggregateId = match id.parse() {
        Ok(v) => v,
        Err(_) => return errors::json_error(StatusCode::BAD_REQUEST, "invalid_id", "invalid invoice id"),
    };
    let invoice_id = InvoiceId::new(agg);
    match services.invoices_get(tenant.tenant_id(), &invoice_id) {
        Some(rm) => (StatusCode::OK, Json(dto::invoice_to_json(rm))).into_response(),
        None => errors::json_error(StatusCode::NOT_FOUND, "not_found", "invoice not found"),
    }
}

pub async fn list_invoices(
    Extension(services): Extension<Arc<AppServices>>,
    Extension(tenant): Extension<crate::context::TenantContext>,
) -> axum::response::Response {
    let items = services
        .invoices_list(tenant.tenant_id())
        .into_iter()
        .map(dto::invoice_to_json)
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!({ "items": items }))).into_response()
}


