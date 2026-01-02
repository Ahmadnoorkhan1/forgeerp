use forgeerp_core::TenantId;

use crate::job::AiJob;
use crate::result::{AiError, AiResult};

/// Tenant scope for execution.
///
/// - `Any`: run jobs for any tenant (useful for shared workers).
/// - `Tenant`: only accept jobs for the specified tenant (safe initialization / single-tenant worker).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TenantScope {
    Any,
    Tenant(TenantId),
}

impl TenantScope {
    pub fn allows(&self, tenant_id: TenantId) -> bool {
        match self {
            TenantScope::Any => true,
            TenantScope::Tenant(t) => *t == tenant_id,
        }
    }
}

/// Scheduler/executor for AI jobs.
///
/// This is intentionally minimal and storage/runtime agnostic.
pub trait AiScheduler: Send + Sync + 'static {
    fn scope(&self) -> TenantScope;

    fn run<J: AiJob>(&self, job: J) -> Result<AiResult, AiError> {
        if !self.scope().allows(job.tenant_id()) {
            return Err(AiError::InvalidInput(
                "tenant scope violation (job tenant not allowed by scheduler)".to_string(),
            ));
        }
        job.run()
    }
}

/// Simple synchronous scheduler that runs jobs immediately in-process.
#[derive(Debug, Copy, Clone)]
pub struct LocalAiScheduler {
    scope: TenantScope,
}

impl LocalAiScheduler {
    pub fn new(scope: TenantScope) -> Self {
        Self { scope }
    }

    pub fn for_tenant(tenant_id: TenantId) -> Self {
        Self::new(TenantScope::Tenant(tenant_id))
    }
}

impl AiScheduler for LocalAiScheduler {
    fn scope(&self) -> TenantScope {
        self.scope
    }
}


