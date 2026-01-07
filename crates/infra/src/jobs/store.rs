//! Job storage implementations.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use forgeerp_core::TenantId;

use super::types::{DeadLetterEntry, Job, JobId, JobKind, JobStatus};

/// Job store abstraction.
pub trait JobStore: Send + Sync {
    /// Enqueue a new job.
    fn enqueue(&self, job: Job) -> Result<JobId, JobStoreError>;

    /// Get a job by ID.
    fn get(&self, tenant_id: TenantId, job_id: JobId) -> Result<Option<Job>, JobStoreError>;

    /// Update a job.
    fn update(&self, job: &Job) -> Result<(), JobStoreError>;

    /// Claim the next pending job that is ready to execute.
    /// Returns None if no jobs are available.
    fn claim_next(&self, tenant_id: Option<TenantId>) -> Result<Option<Job>, JobStoreError>;

    /// List jobs by status.
    fn list_by_status(
        &self,
        tenant_id: TenantId,
        status: Option<JobStatus>,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError>;

    /// List jobs by kind.
    fn list_by_kind(
        &self,
        tenant_id: TenantId,
        kind: &JobKind,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError>;

    /// Move a job to the dead-letter queue.
    fn dead_letter(&self, job: Job, reason: String) -> Result<(), JobStoreError>;

    /// List dead-lettered jobs.
    fn list_dead_letters(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<DeadLetterEntry>, JobStoreError>;

    /// Retry a dead-lettered job (move back to pending).
    fn retry_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<Job, JobStoreError>;

    /// Delete a dead-lettered job.
    fn delete_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<(), JobStoreError>;

    /// Get job statistics.
    fn stats(&self, tenant_id: TenantId) -> Result<JobStats, JobStoreError>;
}

/// Job store error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum JobStoreError {
    #[error("job not found: {0}")]
    NotFound(JobId),
    #[error("tenant isolation violation")]
    TenantIsolation,
    #[error("job already exists: {0}")]
    AlreadyExists(JobId),
    #[error("storage error: {0}")]
    Storage(String),
}

/// Job statistics.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct JobStats {
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub dead_lettered: usize,
    pub cancelled: usize,
}

/// In-memory job store for tests/dev.
#[derive(Debug)]
pub struct InMemoryJobStore {
    jobs: RwLock<HashMap<JobId, Job>>,
    dead_letters: RwLock<HashMap<JobId, DeadLetterEntry>>,
}

impl InMemoryJobStore {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
            dead_letters: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

impl Default for InMemoryJobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JobStore for InMemoryJobStore {
    fn enqueue(&self, job: Job) -> Result<JobId, JobStoreError> {
        let mut jobs = self.jobs.write().unwrap();
        if jobs.contains_key(&job.id) {
            return Err(JobStoreError::AlreadyExists(job.id));
        }
        let id = job.id;
        jobs.insert(id, job);
        Ok(id)
    }

    fn get(&self, tenant_id: TenantId, job_id: JobId) -> Result<Option<Job>, JobStoreError> {
        let jobs = self.jobs.read().unwrap();
        match jobs.get(&job_id) {
            Some(job) if job.tenant_id == tenant_id => Ok(Some(job.clone())),
            Some(_) => Err(JobStoreError::TenantIsolation),
            None => Ok(None),
        }
    }

    fn update(&self, job: &Job) -> Result<(), JobStoreError> {
        let mut jobs = self.jobs.write().unwrap();
        if !jobs.contains_key(&job.id) {
            return Err(JobStoreError::NotFound(job.id));
        }
        jobs.insert(job.id, job.clone());
        Ok(())
    }

    fn claim_next(&self, tenant_id: Option<TenantId>) -> Result<Option<Job>, JobStoreError> {
        let mut jobs = self.jobs.write().unwrap();
        let now = Utc::now();

        // Find the oldest ready pending job
        let mut candidates: Vec<_> = jobs
            .values()
            .filter(|j| {
                matches!(j.status, JobStatus::Pending | JobStatus::Failed { .. })
                    && j.is_ready()
                    && tenant_id.map_or(true, |t| j.tenant_id == t)
            })
            .collect();

        // Sort by created_at to ensure FIFO
        candidates.sort_by_key(|j| j.created_at);

        if let Some(job) = candidates.first() {
            let job_id = job.id;
            if let Some(job) = jobs.get_mut(&job_id) {
                job.mark_running();
                return Ok(Some(job.clone()));
            }
        }

        Ok(None)
    }

    fn list_by_status(
        &self,
        tenant_id: TenantId,
        status: Option<JobStatus>,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError> {
        let jobs = self.jobs.read().unwrap();
        let mut result: Vec<_> = jobs
            .values()
            .filter(|j| {
                j.tenant_id == tenant_id
                    && status
                        .as_ref()
                        .map_or(true, |s| std::mem::discriminant(&j.status) == std::mem::discriminant(s))
            })
            .cloned()
            .collect();

        result.sort_by_key(|j| j.created_at);
        result.truncate(limit);
        Ok(result)
    }

    fn list_by_kind(
        &self,
        tenant_id: TenantId,
        kind: &JobKind,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError> {
        let jobs = self.jobs.read().unwrap();
        let mut result: Vec<_> = jobs
            .values()
            .filter(|j| j.tenant_id == tenant_id && &j.kind == kind)
            .cloned()
            .collect();

        result.sort_by_key(|j| j.created_at);
        result.truncate(limit);
        Ok(result)
    }

    fn dead_letter(&self, mut job: Job, reason: String) -> Result<(), JobStoreError> {
        let mut jobs = self.jobs.write().unwrap();
        let mut dls = self.dead_letters.write().unwrap();

        job.status = JobStatus::DeadLettered {
            error: reason.clone(),
            attempts: job.attempt,
        };
        job.updated_at = Utc::now();

        jobs.remove(&job.id);
        dls.insert(job.id, DeadLetterEntry::new(job, reason));

        Ok(())
    }

    fn list_dead_letters(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<DeadLetterEntry>, JobStoreError> {
        let dls = self.dead_letters.read().unwrap();
        let mut result: Vec<_> = dls
            .values()
            .filter(|e| e.job.tenant_id == tenant_id)
            .cloned()
            .collect();

        result.sort_by_key(|e| e.dead_lettered_at);
        result.truncate(limit);
        Ok(result)
    }

    fn retry_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<Job, JobStoreError> {
        let mut jobs = self.jobs.write().unwrap();
        let mut dls = self.dead_letters.write().unwrap();

        let entry = dls
            .remove(&job_id)
            .ok_or(JobStoreError::NotFound(job_id))?;

        if entry.job.tenant_id != tenant_id {
            // Put it back
            dls.insert(job_id, entry);
            return Err(JobStoreError::TenantIsolation);
        }

        let mut job = entry.job;
        job.status = JobStatus::Pending;
        job.attempt = 0;
        job.scheduled_at = None;
        job.updated_at = Utc::now();
        job.history.clear();

        jobs.insert(job.id, job.clone());
        Ok(job)
    }

    fn delete_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<(), JobStoreError> {
        let mut dls = self.dead_letters.write().unwrap();

        let entry = dls.get(&job_id).ok_or(JobStoreError::NotFound(job_id))?;

        if entry.job.tenant_id != tenant_id {
            return Err(JobStoreError::TenantIsolation);
        }

        dls.remove(&job_id);
        Ok(())
    }

    fn stats(&self, tenant_id: TenantId) -> Result<JobStats, JobStoreError> {
        let jobs = self.jobs.read().unwrap();
        let dls = self.dead_letters.read().unwrap();

        let mut stats = JobStats::default();

        for job in jobs.values() {
            if job.tenant_id != tenant_id {
                continue;
            }
            match &job.status {
                JobStatus::Pending => stats.pending += 1,
                JobStatus::Running => stats.running += 1,
                JobStatus::Completed => stats.completed += 1,
                JobStatus::Failed { .. } => stats.failed += 1,
                JobStatus::DeadLettered { .. } => stats.dead_lettered += 1,
                JobStatus::Cancelled => stats.cancelled += 1,
            }
        }

        for entry in dls.values() {
            if entry.job.tenant_id == tenant_id {
                stats.dead_lettered += 1;
            }
        }

        Ok(stats)
    }
}

impl JobStore for Arc<InMemoryJobStore> {
    fn enqueue(&self, job: Job) -> Result<JobId, JobStoreError> {
        (**self).enqueue(job)
    }

    fn get(&self, tenant_id: TenantId, job_id: JobId) -> Result<Option<Job>, JobStoreError> {
        (**self).get(tenant_id, job_id)
    }

    fn update(&self, job: &Job) -> Result<(), JobStoreError> {
        (**self).update(job)
    }

    fn claim_next(&self, tenant_id: Option<TenantId>) -> Result<Option<Job>, JobStoreError> {
        (**self).claim_next(tenant_id)
    }

    fn list_by_status(
        &self,
        tenant_id: TenantId,
        status: Option<JobStatus>,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError> {
        (**self).list_by_status(tenant_id, status, limit)
    }

    fn list_by_kind(
        &self,
        tenant_id: TenantId,
        kind: &JobKind,
        limit: usize,
    ) -> Result<Vec<Job>, JobStoreError> {
        (**self).list_by_kind(tenant_id, kind, limit)
    }

    fn dead_letter(&self, job: Job, reason: String) -> Result<(), JobStoreError> {
        (**self).dead_letter(job, reason)
    }

    fn list_dead_letters(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<DeadLetterEntry>, JobStoreError> {
        (**self).list_dead_letters(tenant_id, limit)
    }

    fn retry_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<Job, JobStoreError> {
        (**self).retry_dead_letter(tenant_id, job_id)
    }

    fn delete_dead_letter(&self, tenant_id: TenantId, job_id: JobId) -> Result<(), JobStoreError> {
        (**self).delete_dead_letter(tenant_id, job_id)
    }

    fn stats(&self, tenant_id: TenantId) -> Result<JobStats, JobStoreError> {
        (**self).stats(tenant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tenant() -> TenantId {
        TenantId::new()
    }

    #[test]
    fn enqueue_and_claim() {
        let store = InMemoryJobStore::new();
        let tenant = test_tenant();

        let job = Job::new(tenant, JobKind::custom("test"), serde_json::json!({}));
        let job_id = store.enqueue(job).unwrap();

        let claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        assert_eq!(claimed.id, job_id);
        assert!(matches!(claimed.status, JobStatus::Running));
        assert_eq!(claimed.attempt, 1);

        // No more jobs
        assert!(store.claim_next(Some(tenant)).unwrap().is_none());
    }

    #[test]
    fn tenant_isolation() {
        let store = InMemoryJobStore::new();
        let tenant1 = test_tenant();
        let tenant2 = test_tenant();

        let job = Job::new(tenant1, JobKind::custom("test"), serde_json::json!({}));
        let job_id = store.enqueue(job).unwrap();

        // Can't get job from wrong tenant
        assert!(matches!(
            store.get(tenant2, job_id),
            Err(JobStoreError::TenantIsolation)
        ));

        // Can't claim job from wrong tenant
        assert!(store.claim_next(Some(tenant2)).unwrap().is_none());
    }

    #[test]
    fn dead_letter_flow() {
        let store = InMemoryJobStore::new();
        let tenant = test_tenant();

        let job = Job::new(tenant, JobKind::custom("test"), serde_json::json!({}));
        let job_id = job.id;
        store.enqueue(job).unwrap();

        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        claimed.mark_failed("test error".to_string(), Utc::now());

        store.dead_letter(claimed, "max retries exceeded".to_string()).unwrap();

        // Job is no longer in main queue
        assert!(store.get(tenant, job_id).unwrap().is_none());

        // Job is in DLQ
        let dls = store.list_dead_letters(tenant, 10).unwrap();
        assert_eq!(dls.len(), 1);
        assert_eq!(dls[0].job.id, job_id);

        // Retry the job
        let retried = store.retry_dead_letter(tenant, job_id).unwrap();
        assert!(matches!(retried.status, JobStatus::Pending));

        // DLQ is now empty
        let dls = store.list_dead_letters(tenant, 10).unwrap();
        assert!(dls.is_empty());
    }

    #[test]
    fn stats_tracking() {
        let store = InMemoryJobStore::new();
        let tenant = test_tenant();

        for i in 0..5 {
            let job = Job::new(tenant, JobKind::custom("test"), serde_json::json!({"i": i}));
            store.enqueue(job).unwrap();
        }

        let stats = store.stats(tenant).unwrap();
        assert_eq!(stats.pending, 5);

        store.claim_next(Some(tenant)).unwrap();
        store.claim_next(Some(tenant)).unwrap();

        let stats = store.stats(tenant).unwrap();
        assert_eq!(stats.pending, 3);
        assert_eq!(stats.running, 2);
    }
}

