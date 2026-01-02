//! Projection runner utilities (read model builders).
//!
//! This module provides `ProjectionRunner`, a helper that orchestrates projection execution
//! with built-in tenant isolation, sequence number tracking, and idempotency guarantees.
//!
//! ## Why ProjectionRunner?
//!
//! While the `Projection` trait is simple (just `apply()`), running projections in production
//! requires additional concerns:
//!
//! - **Sequence tracking**: Detect duplicate events and skip them (idempotency)
//! - **Tenant isolation**: Ensure events from different tenants don't mix
//! - **Ordering**: Process events in sequence number order
//! - **Cursor management**: Track progress for resumable processing
//!
//! `ProjectionRunner` encapsulates these concerns, making projections easier to write and
//! ensuring they handle edge cases correctly.
//!
//! ## Design Philosophy
//!
//! Read models are **disposable**; events are the source of truth. This module provides
//! deterministic replay and cursor/version tracking without making storage assumptions.
//! The runner tracks progress, but storage of both the cursor and the read model is the
//! responsibility of the projection implementation.

use forgeerp_core::TenantId;

use crate::{EventEnvelope, Projection};

/// Tracks projection progress for a single tenant.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ProjectionCursor {
    tenant_id: TenantId,
    last_sequence_number: u64,
}

impl ProjectionCursor {
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    pub fn last_sequence_number(&self) -> u64 {
        self.last_sequence_number
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    TenantMismatch { expected: TenantId, found: TenantId },
    NonMonotonicSequence { last: u64, found: u64 },
}

/// Runs envelopes through a projection and tracks progress (cursor management).
///
/// `ProjectionRunner` wraps a `Projection` and adds:
///
/// - **Sequence number tracking**: Detects duplicate events and enforces ordering
/// - **Tenant isolation**: Validates events belong to the expected tenant
/// - **Progress tracking**: Maintains a cursor (last processed sequence number)
/// - **Rebuild support**: Helper for rebuilding projections from scratch
///
/// ## Usage Pattern
///
/// ```ignore
/// let mut runner = ProjectionRunner::new(my_projection);
///
/// // Process events from event bus
/// for envelope in event_stream {
///     runner.apply(&envelope)?;  // Validates tenant, sequence, updates cursor
/// }
///
/// // Check progress
/// let cursor = runner.cursor();  // Option<ProjectionCursor>
/// ```
///
/// ## Tenant Pinning
///
/// Use `new_for_tenant()` to pin a runner to a specific tenant. This prevents accidentally
/// processing events from the wrong tenant (defense in depth). Useful for:
/// - Tenant-specific workers
/// - Testing (ensure test data isolation)
/// - Security-sensitive projections
///
/// ## Cursor Management
///
/// The runner tracks the last processed sequence number per tenant. This enables:
/// - **Resumable processing**: Skip already-processed events
/// - **Duplicate detection**: Reject events with sequence numbers <= last processed
/// - **Ordering**: Ensure events are processed in sequence number order
///
/// Note: The cursor is stored in memory. For persistent cursors, store them in your
/// projection's persistence layer and restore them when creating the runner.
///
/// ## Idempotency Guarantees
///
/// The runner enforces idempotency by:
/// - Tracking the last processed sequence number
/// - Rejecting events with sequence numbers <= last processed (duplicates)
/// - Ensuring events are processed in order (monotonic sequence numbers)
///
/// This gives **exactly-once** processing semantics (within a single runner instance).
/// For distributed systems, use distributed sequence tracking (e.g., database-backed cursors).
#[derive(Debug)]
pub struct ProjectionRunner<P>
where
    P: Projection,
{
    projection: P,
    cursor: Option<ProjectionCursor>,
}

impl<P> ProjectionRunner<P>
where
    P: Projection,
{
    pub fn new(projection: P) -> Self {
        Self {
            projection,
            cursor: None,
        }
    }

    /// Create a runner pinned to a specific tenant.
    ///
    /// This prevents accidentally starting a projection with an event from the
    /// wrong tenant.
    pub fn new_for_tenant(tenant_id: TenantId, projection: P) -> Self {
        Self {
            projection,
            cursor: Some(ProjectionCursor {
                tenant_id,
                last_sequence_number: 0,
            }),
        }
    }

    pub fn projection(&self) -> &P {
        &self.projection
    }

    pub fn projection_mut(&mut self) -> &mut P {
        &mut self.projection
    }

    pub fn into_projection(self) -> P {
        self.projection
    }

    /// Current cursor/version for this projection (if any envelopes were applied).
    pub fn cursor(&self) -> Option<ProjectionCursor> {
        self.cursor
    }

    /// Apply a single envelope, enforcing tenant consistency and monotonic sequencing.
    ///
    /// This method processes one event through the projection, with built-in validation:
    ///
    /// - **Tenant check**: If runner is tenant-pinned, validates event belongs to that tenant
    /// - **Sequence check**: Validates sequence number is greater than last processed (no duplicates)
    /// - **Ordering**: Ensures events are processed in sequence number order
    /// - **Progress update**: Updates cursor after successful processing
    ///
    /// ## Return Semantics
    ///
    /// - `Ok(())`: Event processed successfully, cursor updated
    /// - `Err(ProjectionError::TenantMismatch)`: Event belongs to different tenant (if pinned)
    /// - `Err(ProjectionError::NonMonotonicSequence)`: Sequence number <= last processed (duplicate or out-of-order)
    ///
    /// ## Duplicate Handling
    ///
    /// If an event with sequence number <= last processed is received, it's treated as:
    /// - **Duplicate**: Same sequence number (already processed, skip)
    /// - **Out-of-order**: Lower sequence number (shouldn't happen with ordered streams)
    ///
    /// Both cases return an error. The caller can choose to ignore duplicates or fail hard.
    ///
    /// ## First Event
    ///
    /// On the first event (cursor is `None`), the runner:
    /// - Sets the cursor to the event's tenant and sequence number
    /// - Applies the event to the projection
    /// - No validation checks (can start from any sequence number)
    pub fn apply(&mut self, envelope: &EventEnvelope<P::Ev>) -> Result<(), ProjectionError> {
        let found_tenant = envelope.tenant_id();
        let found_seq = envelope.sequence_number();

        match self.cursor {
            None => {
                self.projection.apply(envelope);
                self.cursor = Some(ProjectionCursor {
                    tenant_id: found_tenant,
                    last_sequence_number: found_seq,
                });
                Ok(())
            }
            Some(mut c) => {
                if c.tenant_id != found_tenant {
                    return Err(ProjectionError::TenantMismatch {
                        expected: c.tenant_id,
                        found: found_tenant,
                    });
                }
                if found_seq <= c.last_sequence_number {
                    return Err(ProjectionError::NonMonotonicSequence {
                        last: c.last_sequence_number,
                        found: found_seq,
                    });
                }

                self.projection.apply(envelope);
                c.last_sequence_number = found_seq;
                self.cursor = Some(c);
                Ok(())
            }
        }
    }

    /// Apply many envelopes in order.
    pub fn run<'a>(
        &mut self,
        envelopes: impl IntoIterator<Item = &'a EventEnvelope<P::Ev>>,
    ) -> Result<(), ProjectionError>
    where
        P::Ev: 'a,
    {
        for env in envelopes {
            self.apply(env)?;
        }
        Ok(())
    }

    /// Rebuild a projection from scratch by replaying the full event history.
    ///
    /// This is a convenience method for rebuilding read models when:
    /// - Schema changes require reprocessing all events
    /// - Bug fixes need to correct projection logic
    /// - Testing projections with full event history
    /// - Disaster recovery (rebuilding from backup events)
    ///
    /// ## Usage Pattern
    ///
    /// ```ignore
    /// let events = event_store.load_all_events_for_tenant(tenant_id)?;
    /// let (projection, cursor) = ProjectionRunner::rebuild_from_scratch(
    ///     || MyProjection::new(),
    ///     &events,
    /// )?;
    /// ```
    ///
    /// ## Factory Function
    ///
    /// The factory closure creates a fresh projection instance. This enables:
    /// - **Clean slate**: Start with empty read model
    /// - **Custom initialization**: Projection can initialize state as needed
    /// - **Generic code**: Runner doesn't need to know how to construct projections
    ///
    /// ## Return Value
    ///
    /// Returns the rebuilt projection and the final cursor (last processed sequence number).
    /// The cursor can be used to:
    /// - Resume processing from where rebuild finished
    /// - Validate all events were processed
    /// - Track projection version/schema
    ///
    /// ## Event Ordering
    ///
    /// Events should be provided in sequence number order (per aggregate stream).
    /// The runner will validate ordering, but pre-sorting improves performance.
    ///
    /// ## Performance Considerations
    ///
    /// Rebuilding from scratch processes all events, which can be slow for large histories.
    /// Consider:
    /// - Incremental updates instead of full rebuilds when possible
    /// - Parallel processing for independent aggregates
    /// - Streaming for very large event histories
    pub fn rebuild_from_scratch<'a>(
        factory: impl FnOnce() -> P,
        envelopes: impl IntoIterator<Item = &'a EventEnvelope<P::Ev>>,
    ) -> Result<(P, Option<ProjectionCursor>), ProjectionError>
    where
        P::Ev: 'a,
    {
        let mut runner = ProjectionRunner::new(factory());
        runner.run(envelopes)?;
        Ok((runner.projection, runner.cursor))
    }
}


