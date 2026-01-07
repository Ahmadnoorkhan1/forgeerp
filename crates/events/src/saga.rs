//! Saga / Process Manager mechanics (framework only, no business rules).
//!
//! - Explicit state machines per saga
//! - Event-driven transitions
//! - Compensating actions expressed as commands
//! - Persistence via existing event store (append-only, tenant-scoped)
//!
//! Infra is responsible for running sagas, loading/applying saga events to state,
//! and dispatching resulting commands.
//!
//! Design notes:
//! - A saga instance is identified by a correlation id (e.g., a `SalesOrderId`)
//! - Infra computes a deterministic saga aggregate id from (saga_type, correlation_id)
//! - Saga state is advanced by emitting saga-specific events into the event store
//! - `SagaAction::Command` and `SagaAction::Compensate` emit commands to other aggregates
//! - Actions are idempotent; runners must guard against duplicate deliveries

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value as JsonValue;

use forgeerp_core::{AggregateId, TenantId};

use crate::EventEnvelope;

/// Actions a saga can emit in response to an incoming domain event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SagaAction {
    /// Append a saga event (JSON payload) to this saga's stream.
    Emit {
        event_type: String,
        payload: JsonValue,
    },
    /// Dispatch a command to a target aggregate.
    Command {
        aggregate_type: String,
        command_type: String,
        payload: JsonValue,
    },
    /// Dispatch a compensating command to undo prior side-effects.
    Compensate {
        aggregate_type: String,
        command_type: String,
        payload: JsonValue,
    },
    /// Mark saga as completed (infra may emit a terminal saga event).
    Complete,
}

/// Saga contract (mechanics only).
///
/// Implementors define:
/// - a typed state (encodes explicit state machine)
/// - how to correlate incoming events to saga instances
/// - how to react to incoming events given current state
/// - how to apply saga events to mutate state
pub trait Saga: Send + Sync + 'static {
    /// Typed state machine (must be serde for persistence).
    type State: Clone + Default + Serialize + DeserializeOwned + Send + Sync + 'static;
    /// JSON-wrapped saga events (persisted in event store).
    type SagaEvent: Serialize + DeserializeOwned + Send + Sync + 'static;
    /// Correlation id (e.g., `SalesOrderId`), used to route events to a saga instance.
    type CorrelationId: Clone + Send + Sync + 'static;

    /// Stable saga type identifier (used for aggregate_type: e.g., "saga.sales_ar").
    fn saga_type() -> &'static str;

    /// Extract correlation id from a domain event envelope (return None if not relevant).
    fn correlate(envelope: &EventEnvelope<JsonValue>) -> Option<Self::CorrelationId>;

    /// Compute deterministic saga aggregate id from correlation id (per-tenant).
    fn saga_id(tenant_id: TenantId, correlation: &Self::CorrelationId) -> AggregateId;

    /// Return initial state for a new saga instance.
    fn initial_state(_tenant_id: TenantId, _correlation: &Self::CorrelationId) -> Self::State {
        Self::State::default()
    }

    /// Apply a saga event to mutate state (explicit state machine transitions).
    fn apply(state: &mut Self::State, event: &Self::SagaEvent);

    /// React to an incoming domain event, producing zero or more actions.
    ///
    /// Infra will:
    /// - persist any `Emit` events and re-apply them to state
    /// - execute `Command`/`Compensate` via a dispatcher
    /// - persist a terminal event for `Complete` if desired
    fn react(
        state: &Self::State,
        tenant_id: TenantId,
        correlation: &Self::CorrelationId,
        incoming: &EventEnvelope<JsonValue>,
    ) -> Vec<SagaAction>;
}


