//! Parties domain module (customers and suppliers, event-sourced).
//!
//! This crate contains business rules for parties (customers and suppliers),
//! implemented purely as deterministic domain logic (no IO, no HTTP, no storage).

pub mod party;

pub use party::{
    ContactInfo, Party, PartyCommand, PartyEvent, PartyId, PartyKind, PartyRegistered,
    PartyStatus, PartySuspended, PartyUpdated, RegisterParty, SuspendParty, UpdateDetails,
};


