//! Projection runner utilities (read model builders).
//!
//! Read models are **disposable**; events are the source of truth.
//! This module provides deterministic replay and cursor/version tracking
//! without making storage assumptions.

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

/// Runs envelopes through a projection and tracks progress.
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
    /// The factory is used to create a fresh projection instance.
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


