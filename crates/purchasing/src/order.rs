use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;
use forgeerp_parties::PartyId;
use forgeerp_products::ProductId;

/// Purchase order identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PurchaseOrderId(pub AggregateId);

impl PurchaseOrderId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for PurchaseOrderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Purchase order status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PurchaseOrderStatus {
    Draft,
    Approved,
    Received,
    Closed,
}

/// Purchase order line item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineItem {
    pub line_no: u32,
    pub product_id: ProductId,
    pub quantity: i64,
}

/// Aggregate root: PurchaseOrder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrder {
    id: PurchaseOrderId,
    tenant_id: Option<TenantId>,
    supplier_id: Option<PartyId>,
    status: PurchaseOrderStatus,
    lines: Vec<LineItem>,
    version: u64,
    created: bool,
}

impl PurchaseOrder {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: PurchaseOrderId) -> Self {
        Self {
            id,
            tenant_id: None,
            supplier_id: None,
            status: PurchaseOrderStatus::Draft,
            lines: Vec::new(),
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> PurchaseOrderId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn supplier_id(&self) -> Option<PartyId> {
        self.supplier_id
    }

    pub fn status(&self) -> PurchaseOrderStatus {
        self.status
    }

    pub fn lines(&self) -> &[LineItem] {
        &self.lines
    }
}

impl AggregateRoot for PurchaseOrder {
    type Id = PurchaseOrderId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: CreatePurchaseOrder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePurchaseOrder {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub supplier_id: PartyId,
    pub occurred_at: DateTime<Utc>,
}

/// Command: Approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approve {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Command: AddLine (only allowed in Draft).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddLine {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub product_id: ProductId,
    pub quantity: i64,
    pub occurred_at: DateTime<Utc>,
}

/// Command: ReceiveGoods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveGoods {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurchaseOrderCommand {
    CreatePurchaseOrder(CreatePurchaseOrder),
    AddLine(AddLine),
    Approve(Approve),
    ReceiveGoods(ReceiveGoods),
}

/// Event: PurchaseOrderCreated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderCreated {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub supplier_id: PartyId,
    pub occurred_at: DateTime<Utc>,
}

/// Event: PurchaseOrderApproved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderApproved {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub occurred_at: DateTime<Utc>,
}

/// Event: PurchaseOrderLineAdded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderLineAdded {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub line_no: u32,
    pub product_id: ProductId,
    pub quantity: i64,
    pub occurred_at: DateTime<Utc>,
}

/// Event: GoodsReceived.
///
/// This event integrates with inventory by carrying the product and quantity
/// information that should be reflected in stock. A projection or handler in
/// the infrastructure layer can translate this into `InventoryEvent::StockAdjusted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoodsReceived {
    pub tenant_id: TenantId,
    pub order_id: PurchaseOrderId,
    pub supplier_id: PartyId,
    pub lines: Vec<LineItem>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurchaseOrderEvent {
    PurchaseOrderCreated(PurchaseOrderCreated),
    PurchaseOrderLineAdded(PurchaseOrderLineAdded),
    PurchaseOrderApproved(PurchaseOrderApproved),
    GoodsReceived(GoodsReceived),
}

impl Event for PurchaseOrderEvent {
    fn event_type(&self) -> &'static str {
        match self {
            PurchaseOrderEvent::PurchaseOrderCreated(_) => "purchasing.order.created",
            PurchaseOrderEvent::PurchaseOrderLineAdded(_) => "purchasing.order.line_added",
            PurchaseOrderEvent::PurchaseOrderApproved(_) => "purchasing.order.approved",
            PurchaseOrderEvent::GoodsReceived(_) => "purchasing.order.goods_received",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            PurchaseOrderEvent::PurchaseOrderCreated(e) => e.occurred_at,
            PurchaseOrderEvent::PurchaseOrderLineAdded(e) => e.occurred_at,
            PurchaseOrderEvent::PurchaseOrderApproved(e) => e.occurred_at,
            PurchaseOrderEvent::GoodsReceived(e) => e.occurred_at,
        }
    }
}

impl Aggregate for PurchaseOrder {
    type Command = PurchaseOrderCommand;
    type Event = PurchaseOrderEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            PurchaseOrderEvent::PurchaseOrderCreated(e) => {
                self.id = e.order_id;
                self.tenant_id = Some(e.tenant_id);
                self.supplier_id = Some(e.supplier_id);
                self.status = PurchaseOrderStatus::Draft;
                self.lines.clear();
                self.created = true;
            }
            PurchaseOrderEvent::PurchaseOrderLineAdded(e) => {
                self.lines.push(LineItem {
                    line_no: e.line_no,
                    product_id: e.product_id,
                    quantity: e.quantity,
                });
            }
            PurchaseOrderEvent::PurchaseOrderApproved(_) => {
                self.status = PurchaseOrderStatus::Approved;
            }
            PurchaseOrderEvent::GoodsReceived(e) => {
                // For simplicity, we treat all lines as received at once.
                self.lines = e.lines.clone();
                self.status = PurchaseOrderStatus::Received;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            PurchaseOrderCommand::CreatePurchaseOrder(cmd) => self.handle_create(cmd),
            PurchaseOrderCommand::AddLine(cmd) => self.handle_add_line(cmd),
            PurchaseOrderCommand::Approve(cmd) => self.handle_approve(cmd),
            PurchaseOrderCommand::ReceiveGoods(cmd) => self.handle_receive(cmd),
        }
    }
}

impl PurchaseOrder {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_order_id(&self, order_id: PurchaseOrderId) -> Result<(), DomainError> {
        if self.id != order_id {
            return Err(DomainError::invariant("order_id mismatch"));
        }
        Ok(())
    }

    fn handle_create(
        &self,
        cmd: &CreatePurchaseOrder,
    ) -> Result<Vec<PurchaseOrderEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("purchase order already exists"));
        }

        Ok(vec![PurchaseOrderEvent::PurchaseOrderCreated(
            PurchaseOrderCreated {
                tenant_id: cmd.tenant_id,
                order_id: cmd.order_id,
                supplier_id: cmd.supplier_id,
                occurred_at: cmd.occurred_at,
            },
        )])
    }

    fn handle_approve(&self, cmd: &Approve) -> Result<Vec<PurchaseOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        if self.status != PurchaseOrderStatus::Draft {
            return Err(DomainError::invariant(
                "only draft purchase orders can be approved",
            ));
        }

        if self.lines.is_empty() {
            return Err(DomainError::validation(
                "cannot approve purchase order without lines",
            ));
        }

        Ok(vec![PurchaseOrderEvent::PurchaseOrderApproved(
            PurchaseOrderApproved {
                tenant_id: cmd.tenant_id,
                order_id: cmd.order_id,
                occurred_at: cmd.occurred_at,
            },
        )])
    }

    fn handle_add_line(&self, cmd: &AddLine) -> Result<Vec<PurchaseOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        if self.status != PurchaseOrderStatus::Draft {
            return Err(DomainError::invariant(
                "cannot modify purchase order once approved or received",
            ));
        }

        if cmd.quantity <= 0 {
            return Err(DomainError::validation("quantity must be positive"));
        }

        let next_line_no = (self.lines.len() as u32) + 1;
        Ok(vec![PurchaseOrderEvent::PurchaseOrderLineAdded(
            PurchaseOrderLineAdded {
                tenant_id: cmd.tenant_id,
                order_id: cmd.order_id,
                line_no: next_line_no,
                product_id: cmd.product_id,
                quantity: cmd.quantity,
                occurred_at: cmd.occurred_at,
            },
        )])
    }

    fn handle_receive(
        &self,
        cmd: &ReceiveGoods,
    ) -> Result<Vec<PurchaseOrderEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_order_id(cmd.order_id)?;

        // Invariant: Cannot receive before approval.
        if self.status != PurchaseOrderStatus::Approved {
            return Err(DomainError::invariant(
                "cannot receive goods before purchase order is approved",
            ));
        }

        if self.supplier_id.is_none() {
            return Err(DomainError::invariant("supplier must be set"));
        }

        if self.lines.is_empty() {
            return Err(DomainError::validation(
                "cannot receive goods for empty purchase order",
            ));
        }

        Ok(vec![PurchaseOrderEvent::GoodsReceived(GoodsReceived {
            tenant_id: cmd.tenant_id,
            order_id: cmd.order_id,
            supplier_id: self.supplier_id.unwrap(),
            lines: self.lines.clone(),
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

    fn test_order_id() -> PurchaseOrderId {
        PurchaseOrderId::new(AggregateId::new())
    }

    fn test_supplier_id() -> PartyId {
        PartyId::new(AggregateId::new())
    }

    fn test_product_id() -> ProductId {
        ProductId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn create_purchase_order_emits_purchase_order_created_event() {
        let order = PurchaseOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let supplier_id = test_supplier_id();

        let cmd = CreatePurchaseOrder {
            tenant_id,
            order_id,
            supplier_id,
            occurred_at: test_time(),
        };

        let events = order
            .handle(&PurchaseOrderCommand::CreatePurchaseOrder(cmd.clone()))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PurchaseOrderEvent::PurchaseOrderCreated(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.order_id, order_id);
                assert_eq!(e.supplier_id, supplier_id);
            }
            _ => panic!("Expected PurchaseOrderCreated event"),
        }
    }

    #[test]
    fn approve_moves_status_to_approved() {
        let mut order = PurchaseOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let supplier_id = test_supplier_id();

        let create_cmd = CreatePurchaseOrder {
            tenant_id,
            order_id,
            supplier_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::CreatePurchaseOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), PurchaseOrderStatus::Draft);

        // Add a default line so approval is valid.
        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 10,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);

        let approve_cmd = Approve {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::Approve(approve_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), PurchaseOrderStatus::Approved);
    }

    #[test]
    fn cannot_receive_before_approval() {
        let mut order = PurchaseOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let supplier_id = test_supplier_id();

        let create_cmd = CreatePurchaseOrder {
            tenant_id,
            order_id,
            supplier_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::CreatePurchaseOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);

        // Add a line but do not approve.
        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id: test_product_id(),
            quantity: 10,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);

        let receive_cmd = ReceiveGoods {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let err = order
            .handle(&PurchaseOrderCommand::ReceiveGoods(receive_cmd))
            .unwrap_err();
        match err {
            DomainError::InvariantViolation(msg)
                if msg.contains("cannot receive goods before purchase order is approved") => {}
            _ => panic!("Expected InvariantViolation for receiving before approval"),
        }
    }

    #[test]
    fn receive_after_approval_emits_goods_received() {
        let mut order = PurchaseOrder::empty(test_order_id());
        let tenant_id = test_tenant_id();
        let order_id = test_order_id();
        let supplier_id = test_supplier_id();
        let product_id = test_product_id();

        let create_cmd = CreatePurchaseOrder {
            tenant_id,
            order_id,
            supplier_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::CreatePurchaseOrder(create_cmd))
            .unwrap();
        order.apply(&events[0]);

        let add_cmd = AddLine {
            tenant_id,
            order_id,
            product_id,
            quantity: 10,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::AddLine(add_cmd))
            .unwrap();
        order.apply(&events[0]);

        let approve_cmd = Approve {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::Approve(approve_cmd))
            .unwrap();
        order.apply(&events[0]);
        assert_eq!(order.status(), PurchaseOrderStatus::Approved);

        let receive_cmd = ReceiveGoods {
            tenant_id,
            order_id,
            occurred_at: test_time(),
        };
        let events = order
            .handle(&PurchaseOrderCommand::ReceiveGoods(receive_cmd))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PurchaseOrderEvent::GoodsReceived(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.order_id, order_id);
                assert_eq!(e.supplier_id, supplier_id);
                assert_eq!(e.lines.len(), 1);
                assert_eq!(e.lines[0].product_id, product_id);
                assert_eq!(e.lines[0].quantity, 10);
            }
            _ => panic!("Expected GoodsReceived event"),
        }
    }
}


