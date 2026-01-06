use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;
use forgeerp_sales::SalesOrderId;
use forgeerp_products::ProductId;

/// Invoice identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvoiceId(pub AggregateId);

impl InvoiceId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for InvoiceId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Invoice status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceStatus {
    Open,
    Paid,
    Void,
}

/// Invoice line derived from a sales order line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceLine {
    pub line_no: u32,
    pub sales_order_id: SalesOrderId,
    pub product_id: ProductId,
    pub quantity: i64,
    /// Price in smallest currency unit (e.g., cents).
    pub unit_price: u64,
}

/// Aggregate root: Invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    id: InvoiceId,
    tenant_id: Option<TenantId>,
    status: InvoiceStatus,
    lines: Vec<InvoiceLine>,
    due_date: Option<DateTime<Utc>>,
    total_amount: u64,
    total_paid: u64,
    version: u64,
    created: bool,
}

impl Invoice {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: InvoiceId) -> Self {
        Self {
            id,
            tenant_id: None,
            status: InvoiceStatus::Open,
            lines: Vec::new(),
            due_date: None,
            total_amount: 0,
            total_paid: 0,
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> InvoiceId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn status(&self) -> InvoiceStatus {
        self.status
    }

    pub fn due_date(&self) -> Option<DateTime<Utc>> {
        self.due_date
    }

    pub fn total_amount(&self) -> u64 {
        self.total_amount
    }

    pub fn total_paid(&self) -> u64 {
        self.total_paid
    }

    pub fn outstanding_amount(&self) -> u64 {
        self.total_amount.saturating_sub(self.total_paid)
    }

    pub fn lines(&self) -> &[InvoiceLine] {
        &self.lines
    }

    /// Invariant: cannot pay void invoice.
    pub fn can_accept_payment(&self) -> bool {
        self.status != InvoiceStatus::Void && self.outstanding_amount() > 0
    }
}

impl AggregateRoot for Invoice {
    type Id = InvoiceId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: IssueInvoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueInvoice {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    pub sales_order_id: SalesOrderId,
    pub lines: Vec<InvoiceLine>,
    pub due_date: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Command: RegisterPayment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterPayment {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    /// Payment amount in smallest currency unit.
    pub amount: u64,
    pub occurred_at: DateTime<Utc>,
}

/// Command: VoidInvoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoidInvoice {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    pub reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvoiceCommand {
    IssueInvoice(IssueInvoice),
    RegisterPayment(RegisterPayment),
    VoidInvoice(VoidInvoice),
}

/// Event: InvoiceIssued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceIssued {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    pub sales_order_id: SalesOrderId,
    pub lines: Vec<InvoiceLine>,
    pub due_date: DateTime<Utc>,
    pub total_amount: u64,
    pub occurred_at: DateTime<Utc>,
}

/// Event: PaymentRegistered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentRegistered {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    pub amount: u64,
    pub new_total_paid: u64,
    pub occurred_at: DateTime<Utc>,
}

/// Event: InvoiceVoided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceVoided {
    pub tenant_id: TenantId,
    pub invoice_id: InvoiceId,
    pub reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvoiceEvent {
    InvoiceIssued(InvoiceIssued),
    PaymentRegistered(PaymentRegistered),
    InvoiceVoided(InvoiceVoided),
}

impl Event for InvoiceEvent {
    fn event_type(&self) -> &'static str {
        match self {
            InvoiceEvent::InvoiceIssued(_) => "invoicing.invoice.issued",
            InvoiceEvent::PaymentRegistered(_) => "invoicing.invoice.payment_registered",
            InvoiceEvent::InvoiceVoided(_) => "invoicing.invoice.voided",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            InvoiceEvent::InvoiceIssued(e) => e.occurred_at,
            InvoiceEvent::PaymentRegistered(e) => e.occurred_at,
            InvoiceEvent::InvoiceVoided(e) => e.occurred_at,
        }
    }
}

impl Aggregate for Invoice {
    type Command = InvoiceCommand;
    type Event = InvoiceEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            InvoiceEvent::InvoiceIssued(e) => {
                self.id = e.invoice_id;
                self.tenant_id = Some(e.tenant_id);
                self.lines = e.lines.clone();
                self.due_date = Some(e.due_date);
                self.total_amount = e.total_amount;
                self.total_paid = 0;
                self.status = InvoiceStatus::Open;
                self.created = true;
            }
            InvoiceEvent::PaymentRegistered(e) => {
                self.total_paid = e.new_total_paid;
                if self.total_paid >= self.total_amount {
                    self.status = InvoiceStatus::Paid;
                }
            }
            InvoiceEvent::InvoiceVoided(_) => {
                self.status = InvoiceStatus::Void;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            InvoiceCommand::IssueInvoice(cmd) => self.handle_issue(cmd),
            InvoiceCommand::RegisterPayment(cmd) => self.handle_register_payment(cmd),
            InvoiceCommand::VoidInvoice(cmd) => self.handle_void(cmd),
        }
    }
}

impl Invoice {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_invoice_id(&self, invoice_id: InvoiceId) -> Result<(), DomainError> {
        if self.id != invoice_id {
            return Err(DomainError::invariant("invoice_id mismatch"));
        }
        Ok(())
    }

    fn handle_issue(&self, cmd: &IssueInvoice) -> Result<Vec<InvoiceEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("invoice already exists"));
        }

        if cmd.lines.is_empty() {
            return Err(DomainError::validation(
                "cannot issue invoice without lines",
            ));
        }

        let mut total: u64 = 0;
        for line in &cmd.lines {
            if line.quantity <= 0 {
                return Err(DomainError::validation(
                    "invoice line quantity must be positive",
                ));
            }
            if line.unit_price == 0 {
                return Err(DomainError::validation(
                    "invoice line unit_price must be positive",
                ));
            }
            let line_total = (line.quantity as i128)
                .checked_mul(line.unit_price as i128)
                .ok_or_else(|| DomainError::invariant("invoice line amount overflow"))?;
            if line_total <= 0 {
                return Err(DomainError::invariant(
                    "invoice line total must be positive",
                ));
            }
            total = total
                .checked_add(line_total as u64)
                .ok_or_else(|| DomainError::invariant("invoice total overflow"))?;
        }

        Ok(vec![InvoiceEvent::InvoiceIssued(InvoiceIssued {
            tenant_id: cmd.tenant_id,
            invoice_id: cmd.invoice_id,
            sales_order_id: cmd.sales_order_id,
            lines: cmd.lines.clone(),
            due_date: cmd.due_date,
            total_amount: total,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_register_payment(
        &self,
        cmd: &RegisterPayment,
    ) -> Result<Vec<InvoiceEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_invoice_id(cmd.invoice_id)?;

        if !self.can_accept_payment() {
            return Err(DomainError::invariant(
                "cannot register payment on void or fully paid invoice",
            ));
        }

        if cmd.amount == 0 {
            return Err(DomainError::validation(
                "payment amount must be positive",
            ));
        }

        let new_total_paid = self
            .total_paid
            .checked_add(cmd.amount)
            .ok_or_else(|| DomainError::invariant("payment total overflow"))?;

        if new_total_paid > self.total_amount {
            return Err(DomainError::invariant("cannot overpay invoice"));
        }

        Ok(vec![InvoiceEvent::PaymentRegistered(PaymentRegistered {
            tenant_id: cmd.tenant_id,
            invoice_id: cmd.invoice_id,
            amount: cmd.amount,
            new_total_paid,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_void(&self, cmd: &VoidInvoice) -> Result<Vec<InvoiceEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_invoice_id(cmd.invoice_id)?;

        if self.status == InvoiceStatus::Void {
            return Err(DomainError::conflict("invoice is already void"));
        }

        Ok(vec![InvoiceEvent::InvoiceVoided(InvoiceVoided {
            tenant_id: cmd.tenant_id,
            invoice_id: cmd.invoice_id,
            reason: cmd.reason.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgeerp_core::AggregateId;

    fn test_tenant_id() -> TenantId {
        TenantId::new()
    }

    fn test_invoice_id() -> InvoiceId {
        InvoiceId::new(AggregateId::new())
    }

    fn test_sales_order_id() -> SalesOrderId {
        SalesOrderId::new(AggregateId::new())
    }

    fn test_product_id() -> ProductId {
        ProductId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    fn single_line(order_id: SalesOrderId) -> InvoiceLine {
        InvoiceLine {
            line_no: 1,
            sales_order_id: order_id,
            product_id: test_product_id(),
            quantity: 2,
            unit_price: 100,
        }
    }

    #[test]
    fn issue_invoice_emits_invoice_issued_event() {
        let invoice = Invoice::empty(test_invoice_id());
        let tenant_id = test_tenant_id();
        let invoice_id = test_invoice_id();
        let order_id = test_sales_order_id();

        let line = single_line(order_id);
        let due = test_time();
        let cmd = IssueInvoice {
            tenant_id,
            invoice_id,
            sales_order_id: order_id,
            lines: vec![line.clone()],
            due_date: due,
            occurred_at: test_time(),
        };

        let events = invoice
            .handle(&InvoiceCommand::IssueInvoice(cmd.clone()))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            InvoiceEvent::InvoiceIssued(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.invoice_id, invoice_id);
                assert_eq!(e.sales_order_id, order_id);
                assert_eq!(e.lines.len(), 1);
                assert_eq!(e.due_date, due);
                assert_eq!(e.total_amount, 2 * 100);
            }
            _ => panic!("Expected InvoiceIssued event"),
        }
    }

    #[test]
    fn cannot_pay_void_invoice() {
        let mut invoice = Invoice::empty(test_invoice_id());
        let tenant_id = test_tenant_id();
        let invoice_id = test_invoice_id();
        let order_id = test_sales_order_id();

        let line = single_line(order_id);
        let cmd_issue = IssueInvoice {
            tenant_id,
            invoice_id,
            sales_order_id: order_id,
            lines: vec![line],
            due_date: test_time(),
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::IssueInvoice(cmd_issue))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.status(), InvoiceStatus::Open);

        // Void invoice
        let cmd_void = VoidInvoice {
            tenant_id,
            invoice_id,
            reason: Some("Customer dispute".to_string()),
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::VoidInvoice(cmd_void))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.status(), InvoiceStatus::Void);

        // Try to pay void invoice
        let cmd_pay = RegisterPayment {
            tenant_id,
            invoice_id,
            amount: 50,
            occurred_at: test_time(),
        };
        let err = invoice
            .handle(&InvoiceCommand::RegisterPayment(cmd_pay))
            .unwrap_err();
        match err {
            DomainError::InvariantViolation(msg)
                if msg.contains("cannot register payment on void or fully paid invoice") => {}
            _ => panic!("Expected InvariantViolation for paying void invoice"),
        }
    }

    #[test]
    fn cannot_overpay_invoice() {
        let mut invoice = Invoice::empty(test_invoice_id());
        let tenant_id = test_tenant_id();
        let invoice_id = test_invoice_id();
        let order_id = test_sales_order_id();

        let line = single_line(order_id);
        let cmd_issue = IssueInvoice {
            tenant_id,
            invoice_id,
            sales_order_id: order_id,
            lines: vec![line],
            due_date: test_time(),
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::IssueInvoice(cmd_issue))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.total_amount(), 200);

        // Try to overpay
        let cmd_pay = RegisterPayment {
            tenant_id,
            invoice_id,
            amount: 201,
            occurred_at: test_time(),
        };
        let err = invoice
            .handle(&InvoiceCommand::RegisterPayment(cmd_pay))
            .unwrap_err();
        match err {
            DomainError::InvariantViolation(msg) if msg.contains("cannot overpay invoice") => {}
            _ => panic!("Expected InvariantViolation for overpaying invoice"),
        }
    }

    #[test]
    fn paying_to_total_marks_invoice_paid() {
        let mut invoice = Invoice::empty(test_invoice_id());
        let tenant_id = test_tenant_id();
        let invoice_id = test_invoice_id();
        let order_id = test_sales_order_id();

        let line = single_line(order_id);
        let cmd_issue = IssueInvoice {
            tenant_id,
            invoice_id,
            sales_order_id: order_id,
            lines: vec![line],
            due_date: test_time(),
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::IssueInvoice(cmd_issue))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.status(), InvoiceStatus::Open);
        assert_eq!(invoice.total_amount(), 200);
        assert_eq!(invoice.total_paid(), 0);

        // First partial payment
        let cmd_pay1 = RegisterPayment {
            tenant_id,
            invoice_id,
            amount: 50,
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::RegisterPayment(cmd_pay1))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.total_paid(), 50);
        assert_eq!(invoice.status(), InvoiceStatus::Open);

        // Second payment to reach full amount
        let cmd_pay2 = RegisterPayment {
            tenant_id,
            invoice_id,
            amount: 150,
            occurred_at: test_time(),
        };
        let events = invoice
            .handle(&InvoiceCommand::RegisterPayment(cmd_pay2))
            .unwrap();
        invoice.apply(&events[0]);
        assert_eq!(invoice.total_paid(), 200);
        assert_eq!(invoice.status(), InvoiceStatus::Paid);
    }
}


