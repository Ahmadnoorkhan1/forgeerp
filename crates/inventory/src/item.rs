use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;

/// Inventory item identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InventoryItemId(pub AggregateId);

impl InventoryItemId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for InventoryItemId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Aggregate root: InventoryItem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    id: InventoryItemId,
    tenant_id: Option<TenantId>,
    name: String,
    stock: i64,
    version: u64,
    created: bool,
}

impl InventoryItem {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: InventoryItemId) -> Self {
        Self {
            id,
            tenant_id: None,
            name: String::new(),
            stock: 0,
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> InventoryItemId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn stock(&self) -> i64 {
        self.stock
    }
}

impl AggregateRoot for InventoryItem {
    type Id = InventoryItemId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: CreateItem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateItem {
    pub tenant_id: TenantId,
    pub item_id: InventoryItemId,
    pub name: String,
    pub occurred_at: DateTime<Utc>,
}

/// Command: AdjustStock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustStock {
    pub tenant_id: TenantId,
    pub item_id: InventoryItemId,
    pub delta: i64,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InventoryCommand {
    CreateItem(CreateItem),
    AdjustStock(AdjustStock),
}

/// Event: ItemCreated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemCreated {
    pub tenant_id: TenantId,
    pub item_id: InventoryItemId,
    pub name: String,
    pub occurred_at: DateTime<Utc>,
}

/// Event: StockAdjusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockAdjusted {
    pub tenant_id: TenantId,
    pub item_id: InventoryItemId,
    pub delta: i64,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InventoryEvent {
    ItemCreated(ItemCreated),
    StockAdjusted(StockAdjusted),
}

impl Event for InventoryEvent {
    fn event_type(&self) -> &'static str {
        match self {
            InventoryEvent::ItemCreated(_) => "inventory.item.created",
            InventoryEvent::StockAdjusted(_) => "inventory.item.stock_adjusted",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            InventoryEvent::ItemCreated(e) => e.occurred_at,
            InventoryEvent::StockAdjusted(e) => e.occurred_at,
        }
    }
}

impl Aggregate for InventoryItem {
    type Command = InventoryCommand;
    type Event = InventoryEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            InventoryEvent::ItemCreated(e) => {
                self.id = e.item_id;
                self.tenant_id = Some(e.tenant_id);
                self.name = e.name.clone();
                self.stock = 0;
                self.created = true;
            }
            InventoryEvent::StockAdjusted(e) => {
                self.stock += e.delta;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            InventoryCommand::CreateItem(cmd) => self.handle_create(cmd),
            InventoryCommand::AdjustStock(cmd) => self.handle_adjust(cmd),
        }
    }
}

impl InventoryItem {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_item_id(&self, item_id: InventoryItemId) -> Result<(), DomainError> {
        if self.id != item_id {
            return Err(DomainError::invariant("item_id mismatch"));
        }
        Ok(())
    }

    fn handle_create(&self, cmd: &CreateItem) -> Result<Vec<InventoryEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("item already exists"));
        }
        if cmd.name.trim().is_empty() {
            return Err(DomainError::validation("name cannot be empty"));
        }
        Ok(vec![InventoryEvent::ItemCreated(ItemCreated {
            tenant_id: cmd.tenant_id,
            item_id: cmd.item_id,
            name: cmd.name.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_adjust(&self, cmd: &AdjustStock) -> Result<Vec<InventoryEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_item_id(cmd.item_id)?;

        if cmd.delta == 0 {
            return Err(DomainError::validation("delta cannot be zero"));
        }

        let new_stock = self.stock + cmd.delta;
        if new_stock < 0 {
            return Err(DomainError::invariant("stock cannot go negative"));
        }

        Ok(vec![InventoryEvent::StockAdjusted(StockAdjusted {
            tenant_id: cmd.tenant_id,
            item_id: cmd.item_id,
            delta: cmd.delta,
            occurred_at: cmd.occurred_at,
        })])
    }
}


