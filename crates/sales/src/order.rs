use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;
use forgeerp_products::ProductId;

/// Sales order identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SalesOrderId(pub AggregateId);

impl SalesOrderId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for SalesOrderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Sales order status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SalesOrderStatus {
    Draft,
    Confirmed,
    Invoiced,
    Closed,
}

/// Order line: product, quantity, unit price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderLine {
    pub line_no: u32,
    pub product_id: ProductId,
    pub quantity: i64,
    /// Price in smallest currency unit (e.g., cents).
    pub unit_price: u64,
}

/// Aggregate root: SalesOrder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesOrder {
    id: SalesOrderId,
    tenant_id: Option<TenantId>,
    status: SalesOrderStatus,
    lines: Vec<OrderLine>,
    version: u64,
    created: bool,
}

impl SalesOrder {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: SalesOrderId) -> Self {
        Self {
            id,
            tenant_id: None,
            status: SalesOrderStatus::Draft,
            lines: Vec::new(),
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> SalesOrderId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn status(&self) -> SalesOrderStatus {
        self.status
    }

    pub fn lines(&self) -> &[OrderLine] {
        &self.lines
    }

    pub fn is_modifiable(&self) -> bool {
        matches!(self.status, SalesOrderStatus::Draft)
    }

    pub fn is_invoice_allowed(&self) -> bool {
        matches!(self.status, SalesOrderStatus::Confirmed)
    }
}

impl AggregateRoot for SalesOrder {
    type Id = SalesOrderId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: CreateSalesOrder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSalesOrder {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Command: AddLine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddLine {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub product_id: ProductId,
    pub quantity: i64,
    pub unit_price: u64,
    pub occurred_at: DateTime<Utc>,
}

/// Command: ConfirmOrder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmOrder {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Command: MarkInvoiced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkInvoiced {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SalesOrderCommand {
    CreateSalesOrder(CreateSalesOrder),
    AddLine(AddLine),
    ConfirmOrder(ConfirmOrder),
    MarkInvoiced(MarkInvoiced),
}

/// Event: SalesOrderCreated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SalesOrderCreated {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Event: LineAdded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineAdded {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub line_no: u32,
    pub product_id: ProductId,
    pub quantity: i64,
    pub unit_price: u64,
    pub occurred_at: DateTime<Utc>,
}

/// Event: OrderConfirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderConfirmed {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Event: OrderInvoiced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderInvoiced {
    pub tenant_id: TenantId,
    pub order_id: SalesOrderId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SalesOrderEvent {
    SalesOrderCreated(SalesOrderCreated),
    LineAdded(LineAdded),
    OrderConfirmed(OrderConfirmed),
    OrderInvoiced(OrderInvoiced),
}

impl Event for SalesOrderEvent {
    fn event_type(&self) -> &'static str {
        match self {
            SalesOrderEvent::SalesOrderCreated(_) => "sales.order.created",
            SalesOrderEvent::LineAdded(_) => "sales.order.line_added",
            SalesOrderEvent::OrderConfirmed(_) => "sales.order.confirmed",
            SalesOrderEvent::OrderInvoiced(_) => "sales.order.invoiced",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            SalesOrderEvent::SalesOrderCreated(e) => e.occurred_at,
            SalesOrderEvent::LineAdded(e) => e.occurred_at,
            SalesOrderEvent::OrderConfirmed(e) => e.occurred_at,
            SalesOrderEvent::OrderInvoiced(e) => e.occurred_at,
        }
    }
}

impl Aggregate for SalesOrder {
    type Command = SalesOrderCommand;
    type Event = SalesOrderEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            SalesOrderEvent::SalesOrderCreated(e) => {
                self.id = e.order_id;
                self.tenant_id = Some(e.tenant_id);
                self.status = SalesOrderStatus::Draft;
                self.lines.clear();
                self.created = true;
            }
            SalesOrderEvent::LineAdded(e) => {
                let line = OrderLine {
                    line_no: e.line_no,
                    product_id: e.product_id,
                    quantity: e.quantity,
                    unit_price: e.unit_price,
                };
                self.lines.push(line);
            }
            SalesOrderEvent::OrderConfirmed(_) => {
                self.status = SalesOrderStatus::Confirmed;
            }
            SalesOrderEvent::OrderInvoiced(_) => {
                self.status = SalesOrderStatus::Invoiced;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            SalesOrderCommand::CreateSalesOrder(cmd) => self.handle_create(cmd),
            SalesOrderCommand::AddLine(cmd) => self.handle_add_line(cmd),
            SalesOrderCommand::ConfirmOrder(cmd) => self.handle_confirm(cmd),
            SalesOrderCommand::MarkInvoiced(cmd) => self.handle_mark_invoiced(cmd),
        }
    }
}

impl SalesOrder {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_order_id(&self, order_id: SalesOrderId) -> Result<(), DomainError> {
        if self.id != order_id {
            return Err(DomainError::invariant("order_id mismatch"));
        }
        Ok(())
    }

    fn handle_create(
        &self,
        cmd: &CreateSalesOrder,
    ) -> Result<Vec<SalesOrderEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("sales order already exists"));
        }

        Ok(vec![SalesOrderEvent::SalesOrderCreated(SalesOrderCreated {
            tenant_id: cmd.tenant_id,
            order_id: cmd.order_id,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_add_line(&self, cmd: &AddLine) -> Result<Vec<SalesOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        if !self.is_modifiable() {
            return Err(DomainError::invariant(
                "cannot modify order once it is confirmed or invoiced",
            ));
        }

        if cmd.quantity <= 0 {
            return Err(DomainError::validation("quantity must be positive"));
        }

        if cmd.unit_price == 0 {
            return Err(DomainError::validation("unit_price must be positive"));
        }

        let next_line_no = (self.lines.len() as u32) + 1;

        Ok(vec![SalesOrderEvent::LineAdded(LineAdded {
            tenant_id: cmd.tenant_id,
            order_id: cmd.order_id,
            line_no: next_line_no,
            product_id: cmd.product_id,
            quantity: cmd.quantity,
            unit_price: cmd.unit_price,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_confirm(
        &self,
        cmd: &ConfirmOrder,
    ) -> Result<Vec<SalesOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        if self.status != SalesOrderStatus::Draft {
            return Err(DomainError::invariant(
                "only draft orders can be confirmed",
            ));
        }

        if self.lines.is_empty() {
            return Err(DomainError::validation(
                "cannot confirm order without lines",
            ));
        }

        Ok(vec![SalesOrderEvent::OrderConfirmed(OrderConfirmed {
            tenant_id: cmd.tenant_id,
            order_id: cmd.order_id,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_mark_invoiced(
        &self,
        cmd: &MarkInvoiced,
    ) -> Result<Vec<SalesOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        if !self.is_invoice_allowed() {
            return Err(DomainError::invariant(
                "cannot invoice order that is not confirmed",
            ));
        }

        Ok(vec![SalesOrderEvent::OrderInvoiced(OrderInvoiced {
            tenant_id: cmd.tenant_id,
            order_id: cmd.order_id,
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

    fn test_order_id() -> SalesOrderId {
        SalesOrderId::new(AggregateId::new())
    }

    fn test_product_id() -> ProductId {
        ProductId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn create_sales_order_emits_sales_order_created_event() {
        let order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };

        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(cmd.clone()))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            SalesOrderEvent::SalesOrderCreated(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.order_id, order_id);
            }
            _ => panic!("Expected SalesOrderCreated event"),
        }
    }

    #[test]
    fn add_line_emits_line_added_event_in_draft() {
        let mut order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);

        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 2,
            unit_price: 100,
            occurred_at: test_time(),
        };

        let events = order
            .handle(&SalesOrderCommand::AddLine(add_cmd.clone()))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            SalesOrderEvent::LineAdded(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.order_id, order_id);
                assert_eq!(e.quantity, 2);
                assert_eq!(e.unit_price, 100);
                assert_eq!(e.line_no, 1);
            }
            _ => panic!("Expected LineAdded event"),
        }
    }

    #[test]
    fn cannot_modify_confirmed_order() {
        let mut order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        // Create
        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);

        // Add a line
        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 1,
            unit_price: 100,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);

        // Confirm
        let confirm_cmd = ConfirmOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::ConfirmOrder(confirm_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), SalesOrderStatus::Confirmed);

        // Try to add another line after confirmation
        let add_cmd_after = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 1,
            unit_price: 100,
            occurred_at: test_time(),
        };
        let err = order
            .handle(&SalesOrderCommand::AddLine(add_cmd_after))
            .unwrap_err();
        match err {
            DomainError::InvariantViolation(msg)
                if msg.contains("cannot modify order once it is confirmed or invoiced") => {}
            _ => panic!("Expected InvariantViolation for modifying confirmed order"),
        }
    }

    #[test]
    fn cannot_invoice_unconfirmed_order() {
        let mut order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        // Create
        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);

        // Attempt to invoice while still Draft
        let invoice_cmd = MarkInvoiced {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let err = order
            .handle(&SalesOrderCommand::MarkInvoiced(invoice_cmd))
            .unwrap_err();
        match err {
            DomainError::InvariantViolation(msg)
                if msg.contains("cannot invoice order that is not confirmed") => {}
            _ => panic!("Expected InvariantViolation for invoicing unconfirmed order"),
        }
    }

    #[test]
    fn full_lifecycle_draft_to_confirmed_to_invoiced() {
        let mut order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        // Create
        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), SalesOrderStatus::Draft);

        // Add line
        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 2,
            unit_price: 100,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.lines().len(), 1);

        // Confirm
        let confirm_cmd = ConfirmOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::ConfirmOrder(confirm_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), SalesOrderStatus::Confirmed);

        // Invoice
        let invoice_cmd = MarkInvoiced {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::MarkInvoiced(invoice_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), SalesOrderStatus::Invoiced);
    }

    #[test]
    fn version_increments_on_apply() {
        let mut order = SalesOrder::empty(test_order_id());
        assert_eq!(order.version(), 0);

        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.version(), 1);

        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 1,
            unit_price: 100,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.version(), 2);
    }

    #[test]
    fn handle_does_not_mutate_state() {
        let mut order = SalesOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();

        let create_cmd = CreateSalesOrder {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&SalesOrderCommand::CreateSalesOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);
        let initial_version = order.version();
        let initial_status = order.status();
        let initial_line_count = order.lines().len();

        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 1,
            unit_price: 100,
            occurred_at: test_time(),
        };

        let events1 = order
            .handle(&SalesOrderCommand::AddLine(add_cmd.clone()))
            .unwrap();
        let version_after_handle1 = order.version();
        let status_after_handle1 = order.status();
        let lines_after_handle1 = order.lines().len();

        let events2 = order
            .handle(&SalesOrderCommand::AddLine(add_cmd.clone()))
            .unwrap();
        let version_after_handle2 = order.version();
        let status_after_handle2 = order.status();
        let lines_after_handle2 = order.lines().len();

        assert_eq!(version_after_handle1, initial_version);
        assert_eq!(version_after_handle2, initial_version);
        assert_eq!(status_after_handle1, initial_status);
        assert_eq!(status_after_handle2, initial_status);
        assert_eq!(lines_after_handle1, initial_line_count);
        assert_eq!(lines_after_handle2, initial_line_count);

        assert_eq!(events1, events2);
    }

    #[test]
    fn apply_is_deterministic() {
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let product_id = test_product_id();

        let event1 = SalesOrderEvent::SalesOrderCreated(SalesOrderCreated {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        });
        let event2 = SalesOrderEvent::LineAdded(LineAdded {
            tenant_id,
            order_id,
            line_no: 1,
            product_id,
            quantity: 2,
            unit_price: 100,
            occurred_at: test_time(),
        });
        let event3 = SalesOrderEvent::OrderConfirmed(OrderConfirmed {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        });

        let mut order1 = SalesOrder::empty(order_id);
        order1.apply(&event1);
        order1.apply(&event2);
        order1.apply(&event3);

        let mut order2 = SalesOrder::empty(order_id);
        order2.apply(&event1);
        order2.apply(&event2);
        order2.apply(&event3);

        assert_eq!(order1.version(), order2.version());
        assert_eq!(order1.status(), order2.status());
        assert_eq!(order1.lines(), order2.lines());
        assert_eq!(order1.tenant_id(), order2.tenant_id());
        assert_eq!(order1.status(), SalesOrderStatus::Confirmed);
    }
}


