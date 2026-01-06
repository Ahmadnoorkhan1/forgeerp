use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;

/// Product identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductId(pub AggregateId);

impl ProductId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for ProductId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Product status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductStatus {
    Draft,
    Active,
    Archived,
}

/// Optional pricing metadata (no accounting yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingMetadata {
    pub base_price: Option<u64>, // Price in smallest currency unit (e.g., cents)
    pub currency: Option<String>, // ISO currency code (e.g., "USD", "EUR")
}

impl Default for PricingMetadata {
    fn default() -> Self {
        Self {
            base_price: None,
            currency: None,
        }
    }
}

/// Aggregate root: Product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    id: ProductId,
    tenant_id: Option<TenantId>,
    sku: String,
    name: String,
    status: ProductStatus,
    pricing: PricingMetadata,
    version: u64,
    created: bool,
}

impl Product {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: ProductId) -> Self {
        Self {
            id,
            tenant_id: None,
            sku: String::new(),
            name: String::new(),
            status: ProductStatus::Draft,
            pricing: PricingMetadata::default(),
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> ProductId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn sku(&self) -> &str {
        &self.sku
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn status(&self) -> ProductStatus {
        self.status
    }

    pub fn pricing(&self) -> &PricingMetadata {
        &self.pricing
    }

    /// Check if product can be sold (must be Active, not Archived).
    pub fn can_be_sold(&self) -> bool {
        self.status == ProductStatus::Active
    }
}

impl AggregateRoot for Product {
    type Id = ProductId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: CreateProduct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProduct {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub sku: String,
    pub name: String,
    pub pricing: Option<PricingMetadata>,
    pub occurred_at: DateTime<Utc>,
}

/// Command: ActivateProduct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivateProduct {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub occurred_at: DateTime<Utc>,
}

/// Command: ArchiveProduct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveProduct {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductCommand {
    CreateProduct(CreateProduct),
    ActivateProduct(ActivateProduct),
    ArchiveProduct(ArchiveProduct),
}

/// Event: ProductCreated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductCreated {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub sku: String,
    pub name: String,
    pub pricing: PricingMetadata,
    pub occurred_at: DateTime<Utc>,
}

/// Event: ProductActivated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductActivated {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub occurred_at: DateTime<Utc>,
}

/// Event: ProductArchived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductArchived {
    pub tenant_id: TenantId,
    pub product_id: ProductId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductEvent {
    ProductCreated(ProductCreated),
    ProductActivated(ProductActivated),
    ProductArchived(ProductArchived),
}

impl Event for ProductEvent {
    fn event_type(&self) -> &'static str {
        match self {
            ProductEvent::ProductCreated(_) => "products.product.created",
            ProductEvent::ProductActivated(_) => "products.product.activated",
            ProductEvent::ProductArchived(_) => "products.product.archived",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            ProductEvent::ProductCreated(e) => e.occurred_at,
            ProductEvent::ProductActivated(e) => e.occurred_at,
            ProductEvent::ProductArchived(e) => e.occurred_at,
        }
    }
}

impl Aggregate for Product {
    type Command = ProductCommand;
    type Event = ProductEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            ProductEvent::ProductCreated(e) => {
                self.id = e.product_id;
                self.tenant_id = Some(e.tenant_id);
                self.sku = e.sku.clone();
                self.name = e.name.clone();
                self.status = ProductStatus::Draft;
                self.pricing = e.pricing.clone();
                self.created = true;
            }
            ProductEvent::ProductActivated(_) => {
                self.status = ProductStatus::Active;
            }
            ProductEvent::ProductArchived(_) => {
                self.status = ProductStatus::Archived;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            ProductCommand::CreateProduct(cmd) => self.handle_create(cmd),
            ProductCommand::ActivateProduct(cmd) => self.handle_activate(cmd),
            ProductCommand::ArchiveProduct(cmd) => self.handle_archive(cmd),
        }
    }
}

impl Product {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_product_id(&self, product_id: ProductId) -> Result<(), DomainError> {
        if self.id != product_id {
            return Err(DomainError::invariant("product_id mismatch"));
        }
        Ok(())
    }

    fn handle_create(&self, cmd: &CreateProduct) -> Result<Vec<ProductEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("product already exists"));
        }
        
        if cmd.name.trim().is_empty() {
            return Err(DomainError::validation("name cannot be empty"));
        }
        
        if cmd.sku.trim().is_empty() {
            return Err(DomainError::validation("SKU cannot be empty"));
        }
        
        // Note: True SKU uniqueness per tenant requires infrastructure support
        // (checking event store or read model). At the aggregate level, we can only
        // enforce that SKU is non-empty. Infrastructure layer should validate uniqueness
        // before dispatching the command.

        Ok(vec![ProductEvent::ProductCreated(ProductCreated {
            tenant_id: cmd.tenant_id,
            product_id: cmd.product_id,
            sku: cmd.sku.clone(),
            name: cmd.name.clone(),
            pricing: cmd.pricing.clone().unwrap_or_default(),
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_activate(&self, cmd: &ActivateProduct) -> Result<Vec<ProductEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_product_id(cmd.product_id)?;

        if self.status == ProductStatus::Active {
            return Err(DomainError::conflict("product is already active"));
        }

        if self.status == ProductStatus::Archived {
            return Err(DomainError::invariant("archived products cannot be activated"));
        }

        Ok(vec![ProductEvent::ProductActivated(ProductActivated {
            tenant_id: cmd.tenant_id,
            product_id: cmd.product_id,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_archive(&self, cmd: &ArchiveProduct) -> Result<Vec<ProductEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_product_id(cmd.product_id)?;

        if self.status == ProductStatus::Archived {
            return Err(DomainError::conflict("product is already archived"));
        }

        Ok(vec![ProductEvent::ProductArchived(ProductArchived {
            tenant_id: cmd.tenant_id,
            product_id: cmd.product_id,
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

    fn test_product_id() -> ProductId {
        ProductId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn create_product_emits_product_created_event() {
        let product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(cmd.clone())).unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            ProductEvent::ProductCreated(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.product_id, product_id);
                assert_eq!(e.sku, "SKU-001");
                assert_eq!(e.name, "Test Product");
            }
            _ => panic!("Expected ProductCreated event"),
        }
    }

    #[test]
    fn create_product_rejects_empty_name() {
        let product = Product::empty(test_product_id());
        let cmd = CreateProduct {
            tenant_id: test_tenant_id(),
            product_id: test_product_id(),
            sku: "SKU-001".to_string(),
            name: "   ".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let err = product.handle(&ProductCommand::CreateProduct(cmd)).unwrap_err();
        match err {
            DomainError::Validation(_) => {}
            _ => panic!("Expected Validation error for empty name"),
        }
    }

    #[test]
    fn create_product_rejects_empty_sku() {
        let product = Product::empty(test_product_id());
        let cmd = CreateProduct {
            tenant_id: test_tenant_id(),
            product_id: test_product_id(),
            sku: "   ".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let err = product.handle(&ProductCommand::CreateProduct(cmd)).unwrap_err();
        match err {
            DomainError::Validation(_) => {}
            _ => panic!("Expected Validation error for empty SKU"),
        }
    }

    #[test]
    fn create_product_rejects_duplicate_creation() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        // Create the product
        let events = product.handle(&ProductCommand::CreateProduct(create_cmd.clone())).unwrap();
        product.apply(&events[0]);

        // Try to create again
        let err = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for duplicate creation"),
        }
    }

    #[test]
    fn create_product_with_pricing_metadata() {
        let product = Product::empty(test_product_id());
        let pricing = PricingMetadata {
            base_price: Some(9999), // $99.99 in cents
            currency: Some("USD".to_string()),
        };
        let cmd = CreateProduct {
            tenant_id: test_tenant_id(),
            product_id: test_product_id(),
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: Some(pricing.clone()),
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(cmd)).unwrap();
        match &events[0] {
            ProductEvent::ProductCreated(e) => {
                assert_eq!(e.pricing.base_price, pricing.base_price);
                assert_eq!(e.pricing.currency, pricing.currency);
            }
            _ => panic!("Expected ProductCreated event"),
        }
    }

    #[test]
    fn activate_product_emits_product_activated_event() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        // Create the product first
        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.status(), ProductStatus::Draft);

        // Activate product
        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone())).unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            ProductEvent::ProductActivated(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.product_id, product_id);
            }
            _ => panic!("Expected ProductActivated event"),
        }
    }

    #[test]
    fn activate_product_updates_status_to_active() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.status(), ProductStatus::Draft);

        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.status(), ProductStatus::Active);
        assert!(product.can_be_sold());
    }

    #[test]
    fn activate_product_rejects_already_active() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone())).unwrap();
        product.apply(&events[0]);

        // Try to activate again
        let err = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for already active product"),
        }
    }

    #[test]
    fn activate_product_rejects_archived_product() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        // Archive the product
        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap();
        product.apply(&events[0]);

        // Try to activate archived product
        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };

        let err = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap_err();
        match err {
            DomainError::InvariantViolation(msg) if msg.contains("archived products cannot be activated") => {}
            _ => panic!("Expected InvariantViolation error for archived product"),
        }
    }

    #[test]
    fn archive_product_emits_product_archived_event() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd.clone())).unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            ProductEvent::ProductArchived(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.product_id, product_id);
            }
            _ => panic!("Expected ProductArchived event"),
        }
    }

    #[test]
    fn archive_product_updates_status_to_archived() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        // Activate first
        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.status(), ProductStatus::Active);
        assert!(product.can_be_sold());

        // Archive
        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.status(), ProductStatus::Archived);
        assert!(!product.can_be_sold());
    }

    #[test]
    fn archive_product_rejects_already_archived() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd.clone())).unwrap();
        product.apply(&events[0]);

        // Try to archive again
        let err = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for already archived product"),
        }
    }

    #[test]
    fn archived_products_cannot_be_sold() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        // Draft products cannot be sold
        assert!(!product.can_be_sold());

        // Activate
        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap();
        product.apply(&events[0]);
        assert!(product.can_be_sold());

        // Archive
        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap();
        product.apply(&events[0]);
        assert!(!product.can_be_sold());
    }

    #[test]
    fn archive_product_rejects_non_existent_product() {
        let product = Product::empty(test_product_id());
        let archive_cmd = ArchiveProduct {
            tenant_id: test_tenant_id(),
            product_id: test_product_id(),
            occurred_at: test_time(),
        };

        let err = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap_err();
        match err {
            DomainError::NotFound => {}
            _ => panic!("Expected NotFound error for non-existent product"),
        }
    }

    #[test]
    fn archive_product_rejects_wrong_tenant() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);

        // Try to archive with wrong tenant
        let wrong_tenant = test_tenant_id();
        let archive_cmd = ArchiveProduct {
            tenant_id: wrong_tenant,
            product_id,
            occurred_at: test_time(),
        };

        let err = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap_err();
        match err {
            DomainError::InvariantViolation(_) => {}
            _ => panic!("Expected InvariantViolation error for tenant mismatch"),
        }
    }

    #[test]
    fn version_increments_on_apply() {
        let mut product = Product::empty(test_product_id());
        assert_eq!(product.version(), 0);

        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.version(), 1);

        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.version(), 2);

        let archive_cmd = ArchiveProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };
        let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap();
        product.apply(&events[0]);
        assert_eq!(product.version(), 3);
    }

    #[test]
    fn handle_does_not_mutate_state() {
        let mut product = Product::empty(test_product_id());
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let create_cmd = CreateProduct {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: None,
            occurred_at: test_time(),
        };

        let events = product.handle(&ProductCommand::CreateProduct(create_cmd.clone())).unwrap();
        product.apply(&events[0]);
        let initial_version = product.version();
        let initial_status = product.status();

        let activate_cmd = ActivateProduct {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        };

        let events1 = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone())).unwrap();
        let version_after_handle1 = product.version();
        let status_after_handle1 = product.status();

        let events2 = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone())).unwrap();
        let version_after_handle2 = product.version();
        let status_after_handle2 = product.status();

        assert_eq!(version_after_handle1, initial_version);
        assert_eq!(version_after_handle2, initial_version);
        assert_eq!(status_after_handle1, initial_status);
        assert_eq!(status_after_handle2, initial_status);

        assert_eq!(events1, events2);
    }

    #[test]
    fn apply_is_deterministic() {
        let tenant_id = test_tenant_id();
        let product_id = test_product_id();
        let event1 = ProductEvent::ProductCreated(ProductCreated {
            tenant_id,
            product_id,
            sku: "SKU-001".to_string(),
            name: "Test Product".to_string(),
            pricing: PricingMetadata::default(),
            occurred_at: test_time(),
        });
        let event2 = ProductEvent::ProductActivated(ProductActivated {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        });
        let event3 = ProductEvent::ProductArchived(ProductArchived {
            tenant_id,
            product_id,
            occurred_at: test_time(),
        });

        let mut product1 = Product::empty(product_id);
        product1.apply(&event1);
        product1.apply(&event2);
        product1.apply(&event3);

        let mut product2 = Product::empty(product_id);
        product2.apply(&event1);
        product2.apply(&event2);
        product2.apply(&event3);

        assert_eq!(product1.version(), product2.version());
        assert_eq!(product1.status(), product2.status());
        assert_eq!(product1.sku(), product2.sku());
        assert_eq!(product1.name(), product2.name());
        assert_eq!(product1.tenant_id(), product2.tenant_id());
        assert_eq!(product1.status(), ProductStatus::Archived);
        assert_eq!(product1.version(), 3);
    }

    #[cfg(test)]
    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig {
                // Use deterministic seed for CI reproducibility
                cases: 1000,
                ..ProptestConfig::default()
            })]

            /// Property: Handle is deterministic (same state + command = same events).
            #[test]
            fn handle_is_deterministic(
                sku in "[A-Z0-9]{1,20}",
                name in "[A-Za-z][A-Za-z0-9 ]{0,99}"
            ) {
                let mut product = Product::empty(test_product_id());
                let tenant_id = test_tenant_id();
                let product_id = test_product_id();

                // Create the product
                let create_cmd = CreateProduct {
                    tenant_id,
                    product_id,
                    sku: sku.clone(),
                    name: name.clone(),
                    pricing: None,
                    occurred_at: Utc::now(),
                };
                let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
                product.apply(&events[0]);

                // Save state
                let state_before = product.clone();

                // Call handle with same command multiple times
                let activate_cmd = ActivateProduct {
                    tenant_id,
                    product_id,
                    occurred_at: Utc::now(),
                };

                let events1 = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone()));
                let state_after_handle1 = product.clone();

                let events2 = product.handle(&ProductCommand::ActivateProduct(activate_cmd.clone()));
                let state_after_handle2 = product.clone();

                // State should be unchanged by handle() calls
                prop_assert_eq!(&state_before, &state_after_handle1);
                prop_assert_eq!(&state_before, &state_after_handle2);

                // Events should be identical
                prop_assert_eq!(events1, events2);
            }

            /// Property: Apply is deterministic (same events = same final state).
            #[test]
            fn apply_is_deterministic(
                sku in "[A-Z0-9]{1,20}",
                name in "[A-Za-z][A-Za-z0-9 ]{0,99}"
            ) {
                let tenant_id = test_tenant_id();
                let product_id = test_product_id();

                let events: Vec<ProductEvent> = vec![
                    ProductEvent::ProductCreated(ProductCreated {
                        tenant_id,
                        product_id,
                        sku: sku.clone(),
                        name: name.clone(),
                        pricing: PricingMetadata::default(),
                        occurred_at: Utc::now(),
                    }),
                    ProductEvent::ProductActivated(ProductActivated {
                        tenant_id,
                        product_id,
                        occurred_at: Utc::now(),
                    }),
                    ProductEvent::ProductArchived(ProductArchived {
                        tenant_id,
                        product_id,
                        occurred_at: Utc::now(),
                    }),
                ];

                // Apply events to two separate aggregates
                let mut product1 = Product::empty(product_id);
                for event in &events {
                    product1.apply(event);
                }

                let mut product2 = Product::empty(product_id);
                for event in &events {
                    product2.apply(event);
                }

                // Both should be in identical state
                prop_assert_eq!(product1.version(), product2.version());
                prop_assert_eq!(product1.status(), product2.status());
                prop_assert_eq!(product1.sku(), product2.sku());
                prop_assert_eq!(product1.name(), product2.name());
                prop_assert_eq!(product1.tenant_id(), product2.tenant_id());
                prop_assert_eq!(product1.status(), ProductStatus::Archived);
                prop_assert!(!product1.can_be_sold());
            }

            /// Property: Archived products cannot be sold (can_be_sold invariant).
            #[test]
            fn archived_products_cannot_be_sold(
                sku in "[A-Z0-9]{1,20}",
                name in "[A-Za-z][A-Za-z0-9 ]{0,99}"
            ) {
                let mut product = Product::empty(test_product_id());
                let tenant_id = test_tenant_id();
                let product_id = test_product_id();

                // Create the product
                let create_cmd = CreateProduct {
                    tenant_id,
                    product_id,
                    sku,
                    name,
                    pricing: None,
                    occurred_at: Utc::now(),
                };
                let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
                product.apply(&events[0]);

                // Draft cannot be sold
                prop_assert!(!product.can_be_sold());

                // Activate
                let activate_cmd = ActivateProduct {
                    tenant_id,
                    product_id,
                    occurred_at: Utc::now(),
                };
                let events = product.handle(&ProductCommand::ActivateProduct(activate_cmd)).unwrap();
                product.apply(&events[0]);
                prop_assert!(product.can_be_sold());

                // Archive
                let archive_cmd = ArchiveProduct {
                    tenant_id,
                    product_id,
                    occurred_at: Utc::now(),
                };
                let events = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)).unwrap();
                product.apply(&events[0]);
                prop_assert!(!product.can_be_sold());
                prop_assert_eq!(product.status(), ProductStatus::Archived);
            }

            /// Property: Version increments monotonically with each applied event.
            #[test]
            fn version_increments_monotonically(
                sku in "[A-Z0-9]{1,20}",
                name in "[A-Za-z][A-Za-z0-9 ]{0,99}"
            ) {
                let mut product = Product::empty(test_product_id());
                let tenant_id = test_tenant_id();
                let product_id = test_product_id();

                // Create the product
                let create_cmd = CreateProduct {
                    tenant_id,
                    product_id,
                    sku,
                    name,
                    pricing: None,
                    occurred_at: Utc::now(),
                };
                let events = product.handle(&ProductCommand::CreateProduct(create_cmd)).unwrap();
                product.apply(&events[0]);

                let mut previous_version = product.version();
                prop_assert_eq!(previous_version, 1);

                // Activate
                let activate_cmd = ActivateProduct {
                    tenant_id,
                    product_id,
                    occurred_at: Utc::now(),
                };
                if let Ok(events) = product.handle(&ProductCommand::ActivateProduct(activate_cmd)) {
                    for event in events {
                        product.apply(&event);
                        let current_version = product.version();
                        prop_assert!(
                            current_version > previous_version,
                            "Version did not increase: {} -> {}",
                            previous_version,
                            current_version
                        );
                        previous_version = current_version;
                    }
                }

                // Archive
                let archive_cmd = ArchiveProduct {
                    tenant_id,
                    product_id,
                    occurred_at: Utc::now(),
                };
                if let Ok(events) = product.handle(&ProductCommand::ArchiveProduct(archive_cmd)) {
                    for event in events {
                        product.apply(&event);
                        let current_version = product.version();
                        prop_assert!(
                            current_version > previous_version,
                            "Version did not increase: {} -> {}",
                            previous_version,
                            current_version
                        );
                        previous_version = current_version;
                    }
                }
            }
        }
    }
}

