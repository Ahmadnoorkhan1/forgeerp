//! Products domain module (event-sourced).
//!
//! This crate contains business rules for products/catalog, implemented purely as
//! deterministic domain logic (no IO, no HTTP, no storage).

pub mod product;

pub use product::{
    ActivateProduct, ArchiveProduct, CreateProduct, Product, ProductArchived, ProductActivated,
    PricingMetadata, ProductCommand, ProductCreated, ProductEvent, ProductId, ProductStatus,
};

