use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use forgeerp_core::{Aggregate, AggregateRoot, AggregateId, DomainError, TenantId};
use forgeerp_events::Event;

/// Party identifier (tenant-scoped via `tenant_id` fields in events/commands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartyId(pub AggregateId);

impl PartyId {
    pub fn new(id: AggregateId) -> Self {
        Self(id)
    }
}

impl core::fmt::Display for PartyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Party kind: customer or supplier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartyKind {
    Customer,
    Supplier,
}

/// Party status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartyStatus {
    Active,
    Suspended,
}

/// Contact information for a party.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactInfo {
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
}

impl Default for ContactInfo {
    fn default() -> Self {
        Self {
            email: None,
            phone: None,
            address: None,
        }
    }
}

/// Aggregate root: Party (customer or supplier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    id: PartyId,
    tenant_id: Option<TenantId>,
    kind: PartyKind,
    name: String,
    contact: ContactInfo,
    status: PartyStatus,
    version: u64,
    created: bool,
}

impl Party {
    /// Create an empty, not-yet-created aggregate instance for rehydration.
    pub fn empty(id: PartyId) -> Self {
        Self {
            id,
            tenant_id: None,
            kind: PartyKind::Customer,
            name: String::new(),
            contact: ContactInfo::default(),
            status: PartyStatus::Active,
            version: 0,
            created: false,
        }
    }

    pub fn id_typed(&self) -> PartyId {
        self.id
    }

    pub fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }

    pub fn kind(&self) -> PartyKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn contact(&self) -> &ContactInfo {
        &self.contact
    }

    pub fn status(&self) -> PartyStatus {
        self.status
    }

    /// Invariant helper: whether this party is allowed to transact.
    ///
    /// Suspended parties cannot transact.
    pub fn can_transact(&self) -> bool {
        self.status == PartyStatus::Active
    }
}

impl AggregateRoot for Party {
    type Id = PartyId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }
}

/// Command: RegisterParty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterParty {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    pub kind: PartyKind,
    pub name: String,
    pub contact: Option<ContactInfo>,
    pub occurred_at: DateTime<Utc>,
}

/// Command: UpdateDetails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateDetails {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    /// Optional new name (if None, keep existing).
    pub name: Option<String>,
    /// Optional new contact info (if None, keep existing).
    pub contact: Option<ContactInfo>,
    pub occurred_at: DateTime<Utc>,
}

/// Command: SuspendParty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuspendParty {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    /// Optional human-readable reason for suspension.
    pub reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyCommand {
    RegisterParty(RegisterParty),
    UpdateDetails(UpdateDetails),
    SuspendParty(SuspendParty),
}

/// Event: PartyRegistered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyRegistered {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    pub kind: PartyKind,
    pub name: String,
    pub contact: ContactInfo,
    pub occurred_at: DateTime<Utc>,
}

/// Event: PartyUpdated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyUpdated {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    pub name: String,
    pub contact: ContactInfo,
    pub occurred_at: DateTime<Utc>,
}

/// Event: PartySuspended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartySuspended {
    pub tenant_id: TenantId,
    pub party_id: PartyId,
    pub reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyEvent {
    PartyRegistered(PartyRegistered),
    PartyUpdated(PartyUpdated),
    PartySuspended(PartySuspended),
}

impl Event for PartyEvent {
    fn event_type(&self) -> &'static str {
        match self {
            PartyEvent::PartyRegistered(_) => "parties.party.registered",
            PartyEvent::PartyUpdated(_) => "parties.party.updated",
            PartyEvent::PartySuspended(_) => "parties.party.suspended",
        }
    }

    fn version(&self) -> u32 {
        1
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            PartyEvent::PartyRegistered(e) => e.occurred_at,
            PartyEvent::PartyUpdated(e) => e.occurred_at,
            PartyEvent::PartySuspended(e) => e.occurred_at,
        }
    }
}

impl Aggregate for Party {
    type Command = PartyCommand;
    type Event = PartyEvent;
    type Error = DomainError;

    fn apply(&mut self, event: &Self::Event) {
        match event {
            PartyEvent::PartyRegistered(e) => {
                self.id = e.party_id;
                self.tenant_id = Some(e.tenant_id);
                self.kind = e.kind;
                self.name = e.name.clone();
                self.contact = e.contact.clone();
                self.status = PartyStatus::Active;
                self.created = true;
            }
            PartyEvent::PartyUpdated(e) => {
                self.name = e.name.clone();
                self.contact = e.contact.clone();
            }
            PartyEvent::PartySuspended(_) => {
                self.status = PartyStatus::Suspended;
            }
        }

        // Deterministic version tracking: +1 per applied event.
        self.version += 1;
    }

    fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            PartyCommand::RegisterParty(cmd) => self.handle_register(cmd),
            PartyCommand::UpdateDetails(cmd) => self.handle_update(cmd),
            PartyCommand::SuspendParty(cmd) => self.handle_suspend(cmd),
        }
    }
}

impl Party {
    fn ensure_tenant(&self, tenant_id: TenantId) -> Result<(), DomainError> {
        if !self.created {
            return Ok(());
        }
        if self.tenant_id != Some(tenant_id) {
            return Err(DomainError::invariant("tenant mismatch"));
        }
        Ok(())
    }

    fn ensure_party_id(&self, party_id: PartyId) -> Result<(), DomainError> {
        if self.id != party_id {
            return Err(DomainError::invariant("party_id mismatch"));
        }
        Ok(())
    }

    fn handle_register(&self, cmd: &RegisterParty) -> Result<Vec<PartyEvent>, DomainError> {
        if self.created {
            return Err(DomainError::conflict("party already exists"));
        }

        if cmd.name.trim().is_empty() {
            return Err(DomainError::validation("name cannot be empty"));
        }

        let contact = cmd.contact.clone().unwrap_or_default();

        Ok(vec![PartyEvent::PartyRegistered(PartyRegistered {
            tenant_id: cmd.tenant_id,
            party_id: cmd.party_id,
            kind: cmd.kind,
            name: cmd.name.clone(),
            contact,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_update(&self, cmd: &UpdateDetails) -> Result<Vec<PartyEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_party_id(cmd.party_id)?;

        let new_name = cmd.name.clone().unwrap_or_else(|| self.name.clone());
        if new_name.trim().is_empty() {
            return Err(DomainError::validation("name cannot be empty"));
        }

        let new_contact = cmd.contact.clone().unwrap_or_else(|| self.contact.clone());

        Ok(vec![PartyEvent::PartyUpdated(PartyUpdated {
            tenant_id: cmd.tenant_id,
            party_id: cmd.party_id,
            name: new_name,
            contact: new_contact,
            occurred_at: cmd.occurred_at,
        })])
    }

    fn handle_suspend(&self, cmd: &SuspendParty) -> Result<Vec<PartyEvent>, DomainError> {
        if !self.created {
            return Err(DomainError::not_found());
        }
        self.ensure_tenant(cmd.tenant_id)?;
        self.ensure_party_id(cmd.party_id)?;

        if self.status == PartyStatus::Suspended {
            return Err(DomainError::conflict("party is already suspended"));
        }

        Ok(vec![PartyEvent::PartySuspended(PartySuspended {
            tenant_id: cmd.tenant_id,
            party_id: cmd.party_id,
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

    fn test_party_id() -> PartyId {
        PartyId::new(AggregateId::new())
    }

    fn test_time() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn register_party_emits_party_registered_event() {
        let party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();
        let contact = ContactInfo {
            email: Some("test@example.com".to_string()),
            phone: Some("+123456789".to_string()),
            address: Some("123 Main St".to_string()),
        };
        let cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: Some(contact.clone()),
            occurred_at: test_time(),
        };

        let events = party.handle(&PartyCommand::RegisterParty(cmd.clone())).unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PartyEvent::PartyRegistered(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.party_id, party_id);
                assert_eq!(e.kind, PartyKind::Customer);
                assert_eq!(e.name, "Test Customer");
                assert_eq!(e.contact, contact);
            }
            _ => panic!("Expected PartyRegistered event"),
        }
    }

    #[test]
    fn register_party_rejects_empty_name() {
        let party = Party::empty(test_party_id());
        let cmd = RegisterParty {
            tenant_id: test_tenant_id(),
            party_id: test_party_id(),
            kind: PartyKind::Supplier,
            name: "   ".to_string(),
            contact: None,
            occurred_at: test_time(),
        };

        let err = party.handle(&PartyCommand::RegisterParty(cmd)).unwrap_err();
        match err {
            DomainError::Validation(_) => {}
            _ => panic!("Expected Validation error for empty name"),
        }
    }

    #[test]
    fn register_party_rejects_duplicate_creation() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();
        let cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: None,
            occurred_at: test_time(),
        };

        let events = party.handle(&PartyCommand::RegisterParty(cmd.clone())).unwrap();
        party.apply(&events[0]);

        let err = party.handle(&PartyCommand::RegisterParty(cmd)).unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for duplicate creation"),
        }
    }

    #[test]
    fn update_details_updates_name_and_contact() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();

        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Old Name".to_string(),
            contact: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd))
            .unwrap();
        party.apply(&events[0]);

        let new_contact = ContactInfo {
            email: Some("new@example.com".to_string()),
            phone: Some("+987654321".to_string()),
            address: None,
        };
        let update_cmd = UpdateDetails {
            tenant_id,
            party_id,
            name: Some("New Name".to_string()),
            contact: Some(new_contact.clone()),
            occurred_at: test_time(),
        };

        let events = party
            .handle(&PartyCommand::UpdateDetails(update_cmd))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PartyEvent::PartyUpdated(e) => {
                assert_eq!(e.name, "New Name");
                assert_eq!(e.contact, new_contact);
            }
            _ => panic!("Expected PartyUpdated event"),
        }
    }

    #[test]
    fn suspend_party_emits_party_suspended_event_and_prevents_transacting() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();

        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Supplier,
            name: "Test Supplier".to_string(),
            contact: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd))
            .unwrap();
        party.apply(&events[0]);
        assert!(party.can_transact());

        let suspend_cmd = SuspendParty {
            tenant_id,
            party_id,
            reason: Some("Risk review".to_string()),
            occurred_at: test_time(),
        };

        let events = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd))
            .unwrap();
        assert_eq!(events.len(), 1);

        match &events[0] {
            PartyEvent::PartySuspended(e) => {
                assert_eq!(e.tenant_id, tenant_id);
                assert_eq!(e.party_id, party_id);
                assert_eq!(e.reason.as_deref(), Some("Risk review"));
            }
            _ => panic!("Expected PartySuspended event"),
        }

        party.apply(&events[0]);
        assert_eq!(party.status(), PartyStatus::Suspended);
        assert!(!party.can_transact());
    }

    #[test]
    fn suspend_party_rejects_already_suspended() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();

        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd))
            .unwrap();
        party.apply(&events[0]);

        let suspend_cmd = SuspendParty {
            tenant_id,
            party_id,
            reason: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd.clone()))
            .unwrap();
        party.apply(&events[0]);

        let err = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd))
            .unwrap_err();
        match err {
            DomainError::Conflict(_) => {}
            _ => panic!("Expected Conflict error for already suspended party"),
        }
    }

    #[test]
    fn suspend_party_rejects_non_existent_party() {
        let party = Party::empty(test_party_id());
        let suspend_cmd = SuspendParty {
            tenant_id: test_tenant_id(),
            party_id: test_party_id(),
            reason: None,
            occurred_at: test_time(),
        };

        let err = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd))
            .unwrap_err();
        match err {
            DomainError::NotFound => {}
            _ => panic!("Expected NotFound error for non-existent party"),
        }
    }

    #[test]
    fn can_transact_reflects_status_invariant() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();

        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd))
            .unwrap();
        party.apply(&events[0]);

        // Active by default after registration.
        assert_eq!(party.status(), PartyStatus::Active);
        assert!(party.can_transact());

        // Suspend party.
        let suspend_cmd = SuspendParty {
            tenant_id,
            party_id,
            reason: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd))
            .unwrap();
        party.apply(&events[0]);

        assert_eq!(party.status(), PartyStatus::Suspended);
        assert!(!party.can_transact());
    }

    #[test]
    fn version_increments_on_apply() {
        let mut party = Party::empty(test_party_id());
        assert_eq!(party.version(), 0);

        let tenant_id = test_tenant_id();
        let party_id = test_party_id();
        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Supplier,
            name: "Test Supplier".to_string(),
            contact: None,
            occurred_at: test_time(),
        };

        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd))
            .unwrap();
        party.apply(&events[0]);
        assert_eq!(party.version(), 1);

        let suspend_cmd = SuspendParty {
            tenant_id,
            party_id,
            reason: None,
            occurred_at: test_time(),
        };
        let events = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd))
            .unwrap();
        party.apply(&events[0]);
        assert_eq!(party.version(), 2);
    }

    #[test]
    fn handle_does_not_mutate_state() {
        let mut party = Party::empty(test_party_id());
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();
        let register_cmd = RegisterParty {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: None,
            occurred_at: test_time(),
        };

        let events = party
            .handle(&PartyCommand::RegisterParty(register_cmd.clone()))
            .unwrap();
        party.apply(&events[0]);
        let initial_version = party.version();
        let initial_status = party.status();

        let suspend_cmd = SuspendParty {
            tenant_id,
            party_id,
            reason: Some("Reason".to_string()),
            occurred_at: test_time(),
        };

        let events1 = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd.clone()))
            .unwrap();
        let version_after_handle1 = party.version();
        let status_after_handle1 = party.status();

        let events2 = party
            .handle(&PartyCommand::SuspendParty(suspend_cmd.clone()))
            .unwrap();
        let version_after_handle2 = party.version();
        let status_after_handle2 = party.status();

        assert_eq!(version_after_handle1, initial_version);
        assert_eq!(version_after_handle2, initial_version);
        assert_eq!(status_after_handle1, initial_status);
        assert_eq!(status_after_handle2, initial_status);

        assert_eq!(events1, events2);
    }

    #[test]
    fn apply_is_deterministic() {
        let tenant_id = test_tenant_id();
        let party_id = test_party_id();
        let contact = ContactInfo {
            email: Some("test@example.com".to_string()),
            phone: None,
            address: None,
        };
        let event1 = PartyEvent::PartyRegistered(PartyRegistered {
            tenant_id,
            party_id,
            kind: PartyKind::Customer,
            name: "Test Customer".to_string(),
            contact: contact.clone(),
            occurred_at: test_time(),
        });
        let event2 = PartyEvent::PartySuspended(PartySuspended {
            tenant_id,
            party_id,
            reason: None,
            occurred_at: test_time(),
        });

        let mut party1 = Party::empty(party_id);
        party1.apply(&event1);
        party1.apply(&event2);

        let mut party2 = Party::empty(party_id);
        party2.apply(&event1);
        party2.apply(&event2);

        assert_eq!(party1.version(), party2.version());
        assert_eq!(party1.status(), party2.status());
        assert_eq!(party1.kind(), party2.kind());
        assert_eq!(party1.name(), party2.name());
        assert_eq!(party1.tenant_id(), party2.tenant_id());
        assert_eq!(party1.status(), PartyStatus::Suspended);
        assert!(!party1.can_transact());
    }
}


