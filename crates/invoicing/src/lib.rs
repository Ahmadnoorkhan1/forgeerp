//! Invoicing domain module (event-sourced).
//!
//! This crate contains business rules for invoices and accounts receivable,
//! implemented purely as deterministic domain logic (no IO, no HTTP, no storage).

pub mod invoice;

pub use invoice::{
    Invoice, InvoiceCommand, InvoiceEvent, InvoiceId, InvoiceIssued, InvoiceLine, InvoiceStatus,
    InvoiceVoided, IssueInvoice, PaymentRegistered, RegisterPayment, VoidInvoice,
};


