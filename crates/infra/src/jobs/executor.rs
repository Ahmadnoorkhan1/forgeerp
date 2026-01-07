//! Job executor with retry and backoff logic.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use tracing::{debug, error, info, warn};

use forgeerp_core::TenantId;

use super::store::{JobStore, JobStoreError};
use super::types::{Job, JobId, JobKind, JobResult, JobStatus};

/// Job handler function type.
pub type JobHandler = Box<dyn Fn(&Job) -> JobResult + Send + Sync>;

/// Job executor configuration.
#[derive(Debug, Clone)]
pub struct JobExecutorConfig {
    /// How often to poll for new jobs
    pub poll_interval: Duration,
    /// Maximum concurrent jobs
    pub max_concurrent: usize,
    /// Name for logging
    pub name: String,
    /// Optional tenant filter
    pub tenant_id: Option<TenantId>,
}

impl Default for JobExecutorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
            max_concurrent: 4,
            name: "job-executor".to_string(),
            tenant_id: None,
        }
    }
}

impl JobExecutorConfig {
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_tenant(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }
}

/// Handle to control a running executor.
#[derive(Debug)]
pub struct JobExecutorHandle {
    shutdown: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
    stats: Arc<Mutex<ExecutorStats>>,
}

impl JobExecutorHandle {
    /// Request graceful shutdown.
    pub fn shutdown(mut self) {
        let _ = self.shutdown.send(());
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }

    /// Get current executor statistics.
    pub fn stats(&self) -> ExecutorStats {
        self.stats.lock().unwrap().clone()
    }
}

/// Executor runtime statistics.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExecutorStats {
    pub jobs_processed: u64,
    pub jobs_succeeded: u64,
    pub jobs_failed: u64,
    pub jobs_dead_lettered: u64,
    pub current_running: usize,
    pub uptime_secs: u64,
}

/// Background job executor.
///
/// Polls a job store for pending jobs, executes them with registered handlers,
/// and handles retries and dead-lettering.
pub struct JobExecutor<S: JobStore> {
    store: S,
    handlers: HashMap<String, JobHandler>,
}

impl<S: JobStore + 'static> JobExecutor<S> {
    /// Create a new executor with the given store.
    pub fn new(store: S) -> Self {
        Self {
            store,
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a job kind.
    pub fn register_handler<F>(&mut self, kind_pattern: impl Into<String>, handler: F)
    where
        F: Fn(&Job) -> JobResult + Send + Sync + 'static,
    {
        self.handlers.insert(kind_pattern.into(), Box::new(handler));
    }

    /// Get the handler for a job kind.
    fn get_handler(&self, kind: &JobKind) -> Option<&JobHandler> {
        // Try exact match first
        let type_name = kind.type_name();
        if let Some(h) = self.handlers.get(type_name) {
            return Some(h);
        }

        // Try category match (e.g., "ai.*" matches "ai.inventory_anomaly")
        for (pattern, handler) in &self.handlers {
            if pattern.ends_with(".*") {
                let prefix = &pattern[..pattern.len() - 2];
                if type_name.starts_with(prefix) {
                    return Some(handler);
                }
            }
        }

        // Try wildcard
        self.handlers.get("*")
    }

    /// Spawn the executor in a background thread.
    pub fn spawn(self, config: JobExecutorConfig) -> JobExecutorHandle
    where
        S: Send,
    {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let stats = Arc::new(Mutex::new(ExecutorStats::default()));
        let stats_clone = stats.clone();

        let name = config.name.clone();
        let join = thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                executor_loop(self, config, shutdown_rx, stats_clone);
            })
            .expect("failed to spawn job executor thread");

        JobExecutorHandle {
            shutdown: shutdown_tx,
            join: Some(join),
            stats,
        }
    }

    /// Execute a single job (for testing or synchronous use).
    pub fn execute_one(&self, job: &mut Job) -> Result<(), String> {
        let handler = self
            .get_handler(&job.kind)
            .ok_or_else(|| format!("no handler for job kind: {:?}", job.kind))?;

        let started = Utc::now();
        job.mark_running();

        match handler(job) {
            JobResult::Success => {
                job.mark_completed(started);
                self.store.update(job).map_err(|e| e.to_string())?;
                Ok(())
            }
            JobResult::Failure(error) => {
                job.mark_failed(error.clone(), started);
                self.store.update(job).map_err(|e| e.to_string())?;

                if matches!(job.status, JobStatus::DeadLettered { .. }) {
                    self.store
                        .dead_letter(job.clone(), error.clone())
                        .map_err(|e| e.to_string())?;
                }

                Err(error)
            }
            JobResult::RetryNow => {
                job.mark_failed("retry requested".to_string(), started);
                job.scheduled_at = None; // Clear any backoff
                self.store.update(job).map_err(|e| e.to_string())?;
                Err("retry requested".to_string())
            }
            JobResult::RetryAfter(delay) => {
                job.mark_failed("retry after delay".to_string(), started);
                job.scheduled_at =
                    Some(Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default());
                self.store.update(job).map_err(|e| e.to_string())?;
                Err("retry after delay".to_string())
            }
        }
    }
}

fn executor_loop<S: JobStore>(
    executor: JobExecutor<S>,
    config: JobExecutorConfig,
    shutdown_rx: mpsc::Receiver<()>,
    stats: Arc<Mutex<ExecutorStats>>,
) {
    info!(executor = %config.name, "job executor started");
    let start_time = Instant::now();

    loop {
        // Check for shutdown
        if shutdown_rx.try_recv().is_ok() {
            break;
        }

        // Update uptime
        {
            let mut s = stats.lock().unwrap();
            s.uptime_secs = start_time.elapsed().as_secs();
        }

        // Try to claim a job
        match executor.store.claim_next(config.tenant_id) {
            Ok(Some(mut job)) => {
                debug!(
                    executor = %config.name,
                    job_id = %job.id,
                    kind = ?job.kind,
                    "claimed job"
                );

                {
                    let mut s = stats.lock().unwrap();
                    s.current_running += 1;
                }

                let result = execute_job(&executor, &mut job);

                {
                    let mut s = stats.lock().unwrap();
                    s.current_running = s.current_running.saturating_sub(1);
                    s.jobs_processed += 1;
                    match result {
                        Ok(()) => s.jobs_succeeded += 1,
                        Err(ref e) if job.status.is_terminal() => {
                            s.jobs_failed += 1;
                            if matches!(job.status, JobStatus::DeadLettered { .. }) {
                                s.jobs_dead_lettered += 1;
                            }
                        }
                        Err(_) => s.jobs_failed += 1,
                    }
                }

                if let Err(e) = result {
                    debug!(
                        executor = %config.name,
                        job_id = %job.id,
                        error = %e,
                        status = ?job.status,
                        "job execution failed"
                    );
                }
            }
            Ok(None) => {
                // No jobs available, sleep
                thread::sleep(config.poll_interval);
            }
            Err(e) => {
                error!(executor = %config.name, error = ?e, "failed to claim job");
                thread::sleep(config.poll_interval);
            }
        }
    }

    info!(executor = %config.name, "job executor stopped");
}

fn execute_job<S: JobStore>(executor: &JobExecutor<S>, job: &mut Job) -> Result<(), String> {
    let handler = match executor.get_handler(&job.kind) {
        Some(h) => h,
        None => {
            let error = format!("no handler for job kind: {:?}", job.kind);
            warn!(job_id = %job.id, error = %error, "no handler for job");
            job.mark_failed(error.clone(), Utc::now());
            executor.store.update(job).ok();
            return Err(error);
        }
    };

    let started = Utc::now();

    match handler(job) {
        JobResult::Success => {
            job.mark_completed(started);
            executor.store.update(job).map_err(|e| e.to_string())?;
            debug!(job_id = %job.id, "job completed successfully");
            Ok(())
        }
        JobResult::Failure(error) => {
            job.mark_failed(error.clone(), started);
            executor.store.update(job).map_err(|e| e.to_string())?;

            if matches!(job.status, JobStatus::DeadLettered { .. }) {
                warn!(job_id = %job.id, error = %error, "job dead-lettered");
                executor.store.dead_letter(job.clone(), error.clone()).ok();
            }

            Err(error)
        }
        JobResult::RetryNow => {
            job.mark_failed("retry requested".to_string(), started);
            job.scheduled_at = None;
            executor.store.update(job).map_err(|e| e.to_string())?;
            Err("retry requested".to_string())
        }
        JobResult::RetryAfter(delay) => {
            job.mark_failed("retry after delay".to_string(), started);
            job.scheduled_at =
                Some(Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default());
            executor.store.update(job).map_err(|e| e.to_string())?;
            Err("retry after delay".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::store::InMemoryJobStore;

    fn test_tenant() -> TenantId {
        TenantId::new()
    }

    #[test]
    fn execute_successful_job() {
        let store = Arc::new(InMemoryJobStore::new());
        let mut executor = JobExecutor::new(store.clone());

        executor.register_handler("test", |_job| JobResult::Success);

        let tenant = test_tenant();
        let job = Job::new(tenant, JobKind::custom("test"), serde_json::json!({}));
        store.enqueue(job.clone()).unwrap();

        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        let result = executor.execute_one(&mut claimed);

        assert!(result.is_ok());
        assert!(matches!(claimed.status, JobStatus::Completed));
    }

    #[test]
    fn execute_failing_job_with_retry() {
        let store = Arc::new(InMemoryJobStore::new());
        let mut executor = JobExecutor::new(store.clone());

        executor.register_handler("test", |_job| JobResult::Failure("test error".to_string()));

        let tenant = test_tenant();
        let mut job = Job::new(tenant, JobKind::custom("test"), serde_json::json!({}))
            .with_retry_policy(super::super::types::RetryPolicy {
                max_attempts: 2,
                ..Default::default()
            });

        store.enqueue(job.clone()).unwrap();

        // First attempt
        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        let result = executor.execute_one(&mut claimed);
        assert!(result.is_err());
        assert!(matches!(claimed.status, JobStatus::Failed { .. }));

        // Second attempt (after backoff would expire)
        claimed.scheduled_at = None; // Skip backoff for test
        store.update(&claimed).unwrap();

        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        let result = executor.execute_one(&mut claimed);
        assert!(result.is_err());
        assert!(matches!(claimed.status, JobStatus::DeadLettered { .. }));
    }

    #[test]
    fn wildcard_handler() {
        let store = Arc::new(InMemoryJobStore::new());
        let mut executor = JobExecutor::new(store.clone());

        executor.register_handler("*", |_job| JobResult::Success);

        let tenant = test_tenant();
        let job = Job::new(tenant, JobKind::custom("anything"), serde_json::json!({}));
        store.enqueue(job.clone()).unwrap();

        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        let result = executor.execute_one(&mut claimed);

        assert!(result.is_ok());
    }

    #[test]
    fn category_handler() {
        let store = Arc::new(InMemoryJobStore::new());
        let mut executor = JobExecutor::new(store.clone());

        executor.register_handler("ai.*", |_job| JobResult::Success);

        let tenant = test_tenant();
        let job = Job::new(
            tenant,
            JobKind::ai_inference("ai.inventory_anomaly"),
            serde_json::json!({}),
        );
        store.enqueue(job.clone()).unwrap();

        let mut claimed = store.claim_next(Some(tenant)).unwrap().unwrap();
        let result = executor.execute_one(&mut claimed);

        assert!(result.is_ok());
    }
}

