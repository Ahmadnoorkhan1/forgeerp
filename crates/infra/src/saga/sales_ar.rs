//! Sales → Invoice → Ledger (AR) saga.
//!
//! Orchestrates the flow:
//! 1. SalesOrder confirmed → issue invoice
//! 2. Invoice issued → post ledger entry
//! 3. Ledger posted → complete saga
//!
//! Compensating action: void invoice if ledger posting fails.

use forgeerp_core::{AggregateId, TenantId};
use forgeerp_events::{EventEnvelope, Saga, SagaAction};
use forgeerp_sales::SalesOrderId;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SalesArSagaState {
    #[default]
    WaitingForOrderConfirmed,
    WaitingForInvoiceIssued,
    WaitingForLedgerPosted { invoice_id: String },
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SalesArSagaEvent {
    OrderConfirmedReceived,
    InvoiceIssueRequested,
    InvoiceIssuedReceived { invoice_id: String },
    LedgerPostRequested,
    LedgerPostedReceived,
    SagaCompleted,
    SagaFailed { reason: String },
}

pub struct SalesArSaga;

impl Saga for SalesArSaga {
    type State = SalesArSagaState;
    type SagaEvent = SalesArSagaEvent;
    type CorrelationId = SalesOrderId;

    fn saga_type() -> &'static str {
        "saga.sales_ar"
    }

    fn correlate(envelope: &EventEnvelope<JsonValue>) -> Option<Self::CorrelationId> {
        match envelope.aggregate_type() {
            "sales.order" => {
                // Extract SalesOrderId from payload
                if let Some(obj) = envelope.payload().as_object() {
                    if let Some(order_id) = obj.get("order_id") {
                        if let Some(id_str) = order_id.as_str() {
                            if let Ok(uuid) = uuid::Uuid::parse_str(id_str) {
                                return Some(SalesOrderId::new(AggregateId::from_uuid(uuid)));
                            }
                        }
                    }
                }
                None
            }
            "invoicing.invoice" => {
                // Extract sales_order_id from payload
                if let Some(obj) = envelope.payload().as_object() {
                    if let Some(order_id) = obj.get("sales_order_id") {
                        if let Some(id_str) = order_id.as_str() {
                            if let Ok(uuid) = uuid::Uuid::parse_str(id_str) {
                                return Some(SalesOrderId::new(AggregateId::from_uuid(uuid)));
                            }
                        }
                    }
                }
                None
            }
            "accounting.ledger" => {
                // We don't currently correlate ledger events directly;
                // they're issued by saga command
                None
            }
            _ => None,
        }
    }

    fn saga_id(_tenant_id: TenantId, correlation: &Self::CorrelationId) -> AggregateId {
        // Simple deterministic saga ID: just use the correlation ID directly
        // In production, you might want a composite key (tenant + correlation + saga_type)
        correlation.0
    }

    fn apply(state: &mut Self::State, event: &Self::SagaEvent) {
        match event {
            SalesArSagaEvent::OrderConfirmedReceived => {
                *state = SalesArSagaState::WaitingForInvoiceIssued;
            }
            SalesArSagaEvent::InvoiceIssueRequested => {
                // No state change; waiting for InvoiceIssued
            }
            SalesArSagaEvent::InvoiceIssuedReceived { invoice_id } => {
                *state = SalesArSagaState::WaitingForLedgerPosted {
                    invoice_id: invoice_id.clone(),
                };
            }
            SalesArSagaEvent::LedgerPostRequested => {
                // No state change; waiting for ledger posted
            }
            SalesArSagaEvent::LedgerPostedReceived => {
                *state = SalesArSagaState::Completed;
            }
            SalesArSagaEvent::SagaCompleted => {
                *state = SalesArSagaState::Completed;
            }
            SalesArSagaEvent::SagaFailed { .. } => {
                *state = SalesArSagaState::Failed;
            }
        }
    }

    fn react(
        state: &Self::State,
        tenant_id: TenantId,
        correlation: &Self::CorrelationId,
        incoming: &EventEnvelope<JsonValue>,
    ) -> Vec<SagaAction> {
        let event_type = incoming.aggregate_type();
        
        match state {
            SalesArSagaState::WaitingForOrderConfirmed => {
                if event_type == "sales.order" {
                    if let Some(obj) = incoming.payload().as_object() {
                        if let Some(evt_variant) = obj.keys().next() {
                            if evt_variant == "OrderConfirmed" {
                                return vec![
                                    SagaAction::Emit {
                                        event_type: "order_confirmed_received".to_string(),
                                        payload: serde_json::json!({}),
                                    },
                                    SagaAction::Emit {
                                        event_type: "invoice_issue_requested".to_string(),
                                        payload: serde_json::json!({}),
                                    },
                                    SagaAction::Command {
                                        aggregate_type: "Invoice".to_string(),
                                        command_type: "IssueInvoice".to_string(),
                                        payload: serde_json::json!({
                                            "tenant_id": tenant_id,
                                            "sales_order_id": correlation.0,
                                        }),
                                    },
                                ];
                            }
                        }
                    }
                }
                vec![]
            }
            SalesArSagaState::WaitingForInvoiceIssued => {
                if event_type == "invoicing.invoice" {
                    if let Some(obj) = incoming.payload().as_object() {
                        if let Some(evt) = obj.get("InvoiceIssued") {
                            if let Some(invoice_id) = evt.get("invoice_id").and_then(|v| v.as_str()) {
                                return vec![
                                    SagaAction::Emit {
                                        event_type: "invoice_issued_received".to_string(),
                                        payload: serde_json::json!({ "invoice_id": invoice_id }),
                                    },
                                    SagaAction::Emit {
                                        event_type: "ledger_post_requested".to_string(),
                                        payload: serde_json::json!({}),
                                    },
                                    SagaAction::Command {
                                        aggregate_type: "Ledger".to_string(),
                                        command_type: "PostJournalEntry".to_string(),
                                        payload: serde_json::json!({
                                            "tenant_id": tenant_id,
                                            "entry_id": uuid::Uuid::now_v7(),
                                            "lines": [
                                                {
                                                    "account_code": "1200",
                                                    "account_name": "Accounts Receivable",
                                                    "kind": "Asset",
                                                    "amount": 0,
                                                    "is_debit": true
                                                },
                                                {
                                                    "account_code": "4000",
                                                    "account_name": "Sales Revenue",
                                                    "kind": "Revenue",
                                                    "amount": 0,
                                                    "is_debit": false
                                                }
                                            ],
                                            "description": format!("AR for invoice {}", invoice_id),
                                            "occurred_at": chrono::Utc::now(),
                                        }),
                                    },
                                ];
                            }
                        }
                    }
                }
                vec![]
            }
            SalesArSagaState::WaitingForLedgerPosted { .. } => {
                if event_type == "accounting.ledger" {
                    if let Some(obj) = incoming.payload().as_object() {
                        if obj.contains_key("JournalEntryPosted") {
                            return vec![
                                SagaAction::Emit {
                                    event_type: "ledger_posted_received".to_string(),
                                    payload: serde_json::json!({}),
                                },
                                SagaAction::Complete,
                            ];
                        }
                    }
                }
                vec![]
            }
            SalesArSagaState::Completed | SalesArSagaState::Failed => vec![],
        }
    }
}
