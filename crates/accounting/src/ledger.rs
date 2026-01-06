use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;

/// High-level account kind (determines normal balance side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

/// Account identifier + metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Account {
    pub code: String, // e.g. "1000"
    pub name: String, // e.g. "Cash"
    pub kind: AccountKind,
}

/// One side of a journal entry (immutable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntryLine {
    pub account: Account,
    /// Positive amount in smallest unit (e.g., cents).
    pub amount: i64,
    /// true = debit, false = credit.
    pub is_debit: bool,
}

/// Ledger identifier (aggregate id).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LedgerId(pub AggregateId);

impl LedgerId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for LedgerId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Aggregate root: Ledger (double-entry journal).
///
/// Note: Ledger does NOT hold balances; it only tracks identity + tenant.
/// Balances are derived from projections over `JournalEntryPosted` events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    id: LedgerId,
    tenant_id: Option<TenantId>,
    version: u64,
    created: bool,
}

impl Ledger {
    /// Empty aggregate for rehydration.
    pub fn empty(id: LedgerId) -> Self {
        Self {
            id,
            tenant_id: None,
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> LedgerId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
}

impl AggregateRoot for Ledger {
    type Id = LedgerId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: PostJournalEntry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostJournalEntry {
    pub tenant_id: TenantId,
    pub ledger_id: LedgerId,
    pub entry_id: uuid::Uuid,
    pub lines: Vec<JournalEntryLine>,
    pub occurred_at: DateTime<Utc>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalCommand {
    PostJournalEntry(PostJournalEntry),
}

/// Event: JournalEntryPosted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntryPosted {
    pub tenant_id: TenantId,
    pub ledger_id: LedgerId,
    pub entry_id: uuid::Uuid,
    pub lines: Vec<JournalEntryLine>,
    pub description: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerEvent {
    JournalEntryPosted(JournalEntryPosted),
}

impl Event for LedgerEvent {
    fn event_type(&self) -> &'static str {
        match self {
            LedgerEvent::JournalEntryPosted(_) => "accounting.ledger.journal_entry_posted",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            LedgerEvent::JournalEntryPosted(e) => e.occurred_at,
        }
    }
}

impl Aggregate for Ledger {
    type Command = JournalCommand;
    type Event = LedgerEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            LedgerEvent::JournalEntryPosted(e) => {
                self.id = e.ledger_id;
                if self.tenant_id.is_none() {
                    self.tenant_id = Some(e.tenant_id);
                    self.created = true;
                }
            }
        }

        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            JournalCommand::PostJournalEntry(cmd) => self.handle_post(cmd),
        }
    }
}

impl Ledger {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn handle_post(&self, cmd: &PostJournalEntry) -> Result<Vec<LedgerEvent>, DomainError> {
        self.ensure_tenant(cmd.tenant_id)?;

        if cmd.lines.is_empty() {
            return Err(DomainError::validation("journal entry must have lines"));
        }

        let mut debit_total: i128 = 0;
        let mut credit_total: i128 = 0;

        for line in &cmd.lines {
            if line.amount <= 0 {
                return Err(DomainError::validation("amount must be positive"));
            }
            if line.is_debit {
                debit_total += line.amount as i128;
            } else {
                credit_total += line.amount as i128;
            }
        }

        if debit_total != credit_total {
            return Err(DomainError::invariant("debits must equal credits"));
        }

        Ok(vec![LedgerEvent::JournalEntryPosted(JournalEntryPosted {
            tenant_id: cmd.tenant_id,
            ledger_id: cmd.ledger_id,
            entry_id: cmd.entry_id,
            lines: cmd.lines.clone(),
            description: cmd.description.clone(),
            occurred_at: cmd.occurred_at,
        })])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgeerp_core::AggregateId;
    use proptest::prelude::*;

    fn test_tenant_id() -> TenantId {
        TenantId::new()
    }

    fn test_ledger_id() -> LedgerId {
        LedgerId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    fn test_account(code: &str, kind: AccountKind) -> Account {
        Account {
            code: code.to_string(),
            name: code.to_string(),
            kind,
        }
    }

    #[test]
    fn post_journal_entry_emits_event_when_balanced() {
        let ledger = Ledger::empty(test_ledger_id());
        let tenant_id = test_tenant_id();
        let ledger_id = test_ledger_id();

        let lines = vec![
            JournalEntryLine {
                account: test_account("1000", AccountKind::Asset),
                amount: 100,
                is_debit: true,
            },
            JournalEntryLine {
                account: test_account("2000", AccountKind::Liability),
                amount: 100,
                is_debit: false,
            },
        ];

        let cmd = PostJournalEntry {
            tenant_id,
            ledger_id,
            entry_id: uuid::Uuid::now_v7(),
            lines: lines.clone(),
            occurred_at: test_time(),
            description: Some("Test entry".to_string()),
        };

        let events = ledger
            .handle(&JournalCommand::PostJournalEntry(cmd))
            .unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            LedgerEvent::JournalEntryPosted(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.ledger_id, ledger_id);
                assert_eq!(e.lines, lines);
            }
        }
    }

    #[test]
    fn unbalanced_entry_is_rejected() {
        let ledger = Ledger::empty(test_ledger_id());
        let tenant_id = test_tenant_id();

        let lines = vec![
            JournalEntryLine {
                account: test_account("1000", AccountKind::Asset),
                amount: 100,
                is_debit: true,
            },
            JournalEntryLine {
                account: test_account("2000", AccountKind::Liability),
                amount: 90,
                is_debit: false,
            },
        ];

        let cmd = PostJournalEntry {
            tenant_id,
            ledger_id: test_ledger_id(),
            entry_id: uuid::Uuid::now_v7(),
            lines,
            occurred_at: test_time(),
            description: None,
        };

        let err = ledger.handle(&JournalCommand::PostJournalEntry(cmd)).unwrap_err();
        match err {
            DomainError::InvariantViolation(msg) if msg.contains("debits must equal credits") => {}
            _ => panic!("Expected invariant violation for unbalanced entry"),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            ..ProptestConfig::default()
        })]

        /// Property: For any generated sequence of balanced journal entries,
        /// the sum of debits minus credits across all posted events is zero.
        #[test]
        fn debits_equal_credits_in_posted_events(
            amounts in prop::collection::vec(1i64..1_000_000i64, 1..10)
        ) {
            let tenant_id = test_tenant_id();
            let ledger_id = test_ledger_id();
            let mut ledger = Ledger::empty(ledger_id);

            let mut all_events: Vec<LedgerEvent> = Vec::new();

            for amount in amounts {
                // Build a balanced entry: one debit, one credit with same amount.
                let lines = vec![
                    JournalEntryLine {
                        account: test_account("1000", AccountKind::Asset),
                        amount,
                        is_debit: true,
                    },
                    JournalEntryLine {
                        account: test_account("2000", AccountKind::Liability),
                        amount,
                        is_debit: false,
                    },
                ];

                let cmd = PostJournalEntry {
                    tenant_id,
                    ledger_id,
                    entry_id: uuid::Uuid::now_v7(),
                    lines: lines.clone(),
                    occurred_at: test_time(),
                    description: None,
                };

                let events = ledger.handle(&JournalCommand::PostJournalEntry(cmd)).unwrap();
                for e in &events {
                    ledger.apply(e);
                }
                all_events.extend(events);
            }

            // Compute sum of debits - credits from all events.
            let mut total: i128 = 0;
            for ev in &all_events {
                let LedgerEvent::JournalEntryPosted(je) = ev;
                for line in &je.lines {
                    if line.is_debit {
                        total += line.amount as i128;
                    } else {
                        total -= line.amount as i128;
                    }
                }
            }

            prop_assert_eq!(total, 0);
        }
    }
}


