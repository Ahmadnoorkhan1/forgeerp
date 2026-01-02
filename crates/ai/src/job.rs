use forgeerp_core::TenantId;

use crate::result::{AiError, AiResult};

/// A tenant-scoped AI inference unit.
///
/// Jobs may consume **projections** or **event streams** via their `Input` type.
/// This crate stays storage-agnostic: inputs are provided by callers (infra/workers).
pub trait AiJob: Send + Sync + 'static {
    type Input: Send + Sync + 'static;

    /// The tenant this job belongs to (tenant-safe execution model).
    fn tenant_id(&self) -> TenantId;

    /// The input snapshot the job will run inference on.
    fn input(&self) -> &Self::Input;

    /// Execute inference and return an AI insight.
    ///
    /// Must not mutate domain state.
    fn run(&self) -> Result<AiResult, AiError>;
}


