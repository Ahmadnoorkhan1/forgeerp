//! Accounting module (double-entry ledger, event-sourced).
//!
//! Pure domain logic only: no IO, no HTTP, no persistence concerns.

pub mod ledger;

pub use ledger::{
    Account, AccountKind, JournalCommand, JournalEntryLine, JournalEntryPosted, Ledger,
    LedgerEvent, LedgerId, LedgerError, PostJournalEntry,
};


