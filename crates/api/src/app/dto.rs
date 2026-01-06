use serde::Deserialize;

use forgeerp_accounting::{AccountKind, Account, JournalEntryLine};
use forgeerp_infra::projections::{
    accounting::AccountBalance,
    invoices::InvoiceReadModel,
    parties::PartyReadModel,
    products::ProductReadModel,
    purchasing::PurchaseOrderReadModel,
    sales_orders::SalesOrderReadModel,
};
use forgeerp_parties::PartyKind;

use crate::app::errors;

// -------------------------
// Request DTOs
// -------------------------

#[derive(Debug, Deserialize)]
pub struct CreateItemRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct AdjustStockRequest {
    pub delta: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductRequest {
    pub sku: String,
    pub name: String,
    pub pricing: Option<forgeerp_products::PricingMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterPartyRequest {
    pub name: String,
    pub contact: Option<forgeerp_parties::ContactInfo>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePartyRequest {
    pub name: Option<String>,
    pub contact: Option<forgeerp_parties::ContactInfo>,
}

#[derive(Debug, Deserialize)]
pub struct SuspendPartyRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSalesOrderLineRequest {
    pub product_id: String,
    pub quantity: i64,
    pub unit_price: u64,
}

#[derive(Debug, Deserialize)]
pub struct IssueInvoiceRequest {
    pub sales_order_id: String,
    pub due_date: String, // RFC3339
    pub lines: Vec<CreateSalesOrderLineRequest>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterPaymentRequest {
    pub amount: u64,
}

#[derive(Debug, Deserialize)]
pub struct VoidInvoiceRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PurchaseOrderLineRequest {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreatePurchaseOrderRequest {
    pub supplier_id: String,
    pub lines: Vec<PurchaseOrderLineRequest>,
}

#[derive(Debug, Deserialize)]
pub struct CreateLedgerLineRequest {
    pub account_code: String,
    pub account_name: String,
    pub kind: String,
    pub amount: i64,
    pub is_debit: bool,
}

#[derive(Debug, Deserialize)]
pub struct PostJournalEntryRequest {
    pub description: Option<String>,
    pub lines: Vec<CreateLedgerLineRequest>,
}

// -------------------------
// JSON mapping helpers
// -------------------------

pub fn inventory_to_json(rm: forgeerp_infra::projections::inventory_stock::InventoryReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.item_id.0.to_string(),
        "name": rm.name,
        "quantity": rm.quantity,
    })
}

pub fn product_to_json(rm: ProductReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.product_id.0.to_string(),
        "sku": rm.sku,
        "name": rm.name,
        "status": format!("{:?}", rm.status).to_lowercase(),
        "pricing": {
            "base_price": rm.pricing.base_price,
            "currency": rm.pricing.currency,
        }
    })
}

pub fn party_to_json(rm: PartyReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.party_id.0.to_string(),
        "kind": match rm.kind { PartyKind::Customer => "customer", PartyKind::Supplier => "supplier" },
        "name": rm.name,
        "email": rm.email,
        "phone": rm.phone,
        "status": format!("{:?}", rm.status).to_lowercase(),
    })
}

pub fn sales_order_to_json(rm: SalesOrderReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.order_id.0.to_string(),
        "status": format!("{:?}", rm.status).to_lowercase(),
        "lines": rm.lines.into_iter().map(|l| serde_json::json!({
            "line_no": l.line_no,
            "product_id": l.product_id.0.to_string(),
            "quantity": l.quantity,
            "unit_price": l.unit_price,
        })).collect::<Vec<_>>()
    })
}

pub fn invoice_to_json(rm: InvoiceReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.invoice_id.0.to_string(),
        "sales_order_id": rm.sales_order_id.0.to_string(),
        "status": format!("{:?}", rm.status).to_lowercase(),
        "due_date": rm.due_date.map(|d| d.to_rfc3339()),
        "total_amount": rm.total_amount,
        "total_paid": rm.total_paid,
        "outstanding_amount": rm.total_amount.saturating_sub(rm.total_paid),
        "lines": rm.lines.into_iter().map(|l| serde_json::json!({
            "line_no": l.line_no,
            "product_id": l.product_id.0.to_string(),
            "quantity": l.quantity,
            "unit_price": l.unit_price,
        })).collect::<Vec<_>>()
    })
}

pub fn purchase_order_to_json(rm: PurchaseOrderReadModel) -> serde_json::Value {
    serde_json::json!({
        "id": rm.order_id.0.to_string(),
        "supplier_id": rm.supplier_id.0.to_string(),
        "status": format!("{:?}", rm.status).to_lowercase(),
        "lines": rm.lines.into_iter().map(|l| serde_json::json!({
            "line_no": l.line_no,
            "product_id": l.product_id.0.to_string(),
            "quantity": l.quantity,
        })).collect::<Vec<_>>()
    })
}

pub fn ledger_balance_to_json(b: AccountBalance) -> serde_json::Value {
    serde_json::json!({
        "account_code": b.account_code,
        "account_name": b.account_name,
        "kind": format!("{:?}", b.kind).to_lowercase(),
        "balance": b.balance.to_string(),
    })
}

pub fn to_journal_lines(
    req_lines: Vec<CreateLedgerLineRequest>,
) -> Result<Vec<JournalEntryLine>, axum::response::Response> {
    let mut lines = Vec::with_capacity(req_lines.len());
    for l in req_lines {
        let kind: AccountKind = match errors::parse_account_kind(&l.kind) {
            Ok(k) => k,
            Err(resp) => return Err(resp),
        };
        lines.push(JournalEntryLine {
            account: Account {
                code: l.account_code,
                name: l.account_name,
                kind,
            },
            amount: l.amount,
            is_debit: l.is_debit,
        });
    }
    Ok(lines)
}


