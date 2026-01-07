//! Core job types and policies.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgeerp_core::TenantId;

/// Unique job identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Job kind/type for routing to appropriate handlers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// AI inference job (e.g., inventory anomaly detection)
    AiInference { job_type: String },
    /// Projection rebuild job
    ProjectionRebuild { projection_name: String },
    /// Saga step execution
    SagaStep {
        saga_type: String,
        step_name: String,
    },
    /// Generic/custom job
    Custom { kind: String },
}

impl JobKind {
    pub fn ai_inference(job_type: impl Into<String>) -> Self {
        Self::AiInference {
            job_type: job_type.into(),
        }
    }

    pub fn projection_rebuild(projection_name: impl Into<String>) -> Self {
        Self::ProjectionRebuild {
            projection_name: projection_name.into(),
        }
    }

    pub fn saga_step(saga_type: impl Into<String>, step_name: impl Into<String>) -> Self {
        Self::SagaStep {
            saga_type: saga_type.into(),
            step_name: step_name.into(),
        }
    }

    pub fn custom(kind: impl Into<String>) -> Self {
        Self::Custom { kind: kind.into() }
    }

    pub fn type_name(&self) -> &str {
        match self {
            JobKind::AiInference { job_type } => job_type,
            JobKind::ProjectionRebuild { projection_name } => projection_name,
            JobKind::SagaStep { saga_type, .. } => saga_type,
            JobKind::Custom { kind } => kind,
        }
    }
}

/// Job execution status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Queued, waiting to be picked up
    Pending,
    /// Currently being executed
    Running,
    /// Completed successfully
    Completed,
    /// Failed, will be retried
    Failed { error: String, attempt: u32 },
    /// Exhausted retries, moved to DLQ
    DeadLettered { error: String, attempts: u32 },
    /// Cancelled by user/system
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::DeadLettered { .. } | JobStatus::Cancelled
        )
    }

    pub fn is_retriable(&self) -> bool {
        matches!(self, JobStatus::Failed { .. })
    }
}

/// Backoff strategy for retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed,
    /// Exponential backoff: base * 2^attempt
    Exponential,
    /// Linear backoff: base * attempt
    Linear,
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential
    }
}

/// Retry policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 = no retries)
    pub max_attempts: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Maximum delay cap
    pub max_delay: Duration,
    /// Backoff strategy
    pub strategy: BackoffStrategy,
    /// Jitter factor (0.0-1.0) to add randomness
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(60),
            strategy: BackoffStrategy::Exponential,
            jitter: 0.1,
        }
    }
}

impl RetryPolicy {
    /// Create a policy with no retries.
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 0,
            ..Default::default()
        }
    }

    /// Create a policy with fixed delays.
    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay: delay,
            max_delay: delay,
            strategy: BackoffStrategy::Fixed,
            jitter: 0.0,
        }
    }

    /// Create a policy with exponential backoff.
    pub fn exponential(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
            strategy: BackoffStrategy::Exponential,
            jitter: 0.1,
        }
    }

    /// Calculate delay for a given attempt number (1-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_ms = self.base_delay.as_millis() as f64;
        let max_ms = self.max_delay.as_millis() as f64;

        let delay_ms = match self.strategy {
            BackoffStrategy::Fixed => base_ms,
            BackoffStrategy::Exponential => {
                let exp = 2_f64.powi((attempt - 1) as i32);
                (base_ms * exp).min(max_ms)
            }
            BackoffStrategy::Linear => {
                let linear = base_ms * (attempt as f64);
                linear.min(max_ms)
            }
        };

        // Apply jitter
        let jitter_range = delay_ms * self.jitter;
        let jitter = if jitter_range > 0.0 {
            // Simple deterministic "jitter" based on attempt
            let pseudo_random = ((attempt as f64 * 17.0) % 100.0) / 100.0;
            jitter_range * (pseudo_random - 0.5) * 2.0
        } else {
            0.0
        };

        Duration::from_millis((delay_ms + jitter).max(0.0) as u64)
    }

    /// Check if more retries are allowed.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }
}

/// A background job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job ID
    pub id: JobId,
    /// Tenant scope
    pub tenant_id: TenantId,
    /// Job kind for routing
    pub kind: JobKind,
    /// JSON payload
    pub payload: serde_json::Value,
    /// Current status
    pub status: JobStatus,
    /// Retry policy
    pub retry_policy: RetryPolicy,
    /// Current attempt number (starts at 0)
    pub attempt: u32,
    /// When the job was created
    pub created_at: DateTime<Utc>,
    /// When the job was last updated
    pub updated_at: DateTime<Utc>,
    /// When the job should next be executed (for scheduled/delayed jobs)
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Execution history (errors from previous attempts)
    pub history: Vec<JobAttemptRecord>,
}

/// Record of a job execution attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAttemptRecord {
    pub attempt: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl Job {
    /// Create a new job.
    pub fn new(tenant_id: TenantId, kind: JobKind, payload: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: JobId::new(),
            tenant_id,
            kind,
            payload,
            status: JobStatus::Pending,
            retry_policy: RetryPolicy::default(),
            attempt: 0,
            created_at: now,
            updated_at: now,
            scheduled_at: None,
            history: Vec::new(),
        }
    }

    /// Set a custom retry policy.
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Schedule the job for later execution.
    pub fn scheduled_at(mut self, at: DateTime<Utc>) -> Self {
        self.scheduled_at = Some(at);
        self
    }

    /// Schedule the job with a delay from now.
    pub fn delayed(mut self, delay: Duration) -> Self {
        self.scheduled_at = Some(Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default());
        self
    }

    /// Check if the job is ready to execute.
    pub fn is_ready(&self) -> bool {
        match self.scheduled_at {
            Some(at) => Utc::now() >= at,
            None => true,
        }
    }

    /// Mark job as running.
    pub fn mark_running(&mut self) {
        self.status = JobStatus::Running;
        self.attempt += 1;
        self.updated_at = Utc::now();
    }

    /// Mark job as completed.
    pub fn mark_completed(&mut self, started_at: DateTime<Utc>) {
        let now = Utc::now();
        self.status = JobStatus::Completed;
        self.updated_at = now;
        self.history.push(JobAttemptRecord {
            attempt: self.attempt,
            started_at,
            finished_at: now,
            success: true,
            error: None,
            duration_ms: (now - started_at).num_milliseconds().max(0) as u64,
        });
    }

    /// Mark job as failed.
    pub fn mark_failed(&mut self, error: String, started_at: DateTime<Utc>) {
        let now = Utc::now();
        self.updated_at = now;
        self.history.push(JobAttemptRecord {
            attempt: self.attempt,
            started_at,
            finished_at: now,
            success: false,
            error: Some(error.clone()),
            duration_ms: (now - started_at).num_milliseconds().max(0) as u64,
        });

        if self.retry_policy.should_retry(self.attempt) {
            // Schedule retry with backoff
            let delay = self.retry_policy.delay_for_attempt(self.attempt);
            self.scheduled_at = Some(now + chrono::Duration::from_std(delay).unwrap_or_default());
            self.status = JobStatus::Failed {
                error,
                attempt: self.attempt,
            };
        } else {
            // Move to dead letter
            self.status = JobStatus::DeadLettered {
                error,
                attempts: self.attempt,
            };
        }
    }

    /// Mark job as cancelled.
    pub fn mark_cancelled(&mut self) {
        self.status = JobStatus::Cancelled;
        self.updated_at = Utc::now();
    }
}

/// Result of job execution.
#[derive(Debug)]
pub enum JobResult {
    /// Job completed successfully
    Success,
    /// Job failed with an error
    Failure(String),
    /// Job should be retried immediately (transient failure)
    RetryNow,
    /// Job should be retried after a delay
    RetryAfter(Duration),
}

/// Entry in the dead-letter queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterEntry {
    pub job: Job,
    pub dead_lettered_at: DateTime<Utc>,
    pub reason: String,
}

impl DeadLetterEntry {
    pub fn new(job: Job, reason: String) -> Self {
        Self {
            job,
            dead_lettered_at: Utc::now(),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_backoff_calculates_correctly() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            strategy: BackoffStrategy::Exponential,
            jitter: 0.0,
        };

        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(800));
    }

    #[test]
    fn fixed_backoff_is_constant() {
        let policy = RetryPolicy::fixed(3, Duration::from_millis(500));

        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(500));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(500));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(500));
    }

    #[test]
    fn linear_backoff_increases_linearly() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            strategy: BackoffStrategy::Linear,
            jitter: 0.0,
        };

        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(300));
    }

    #[test]
    fn should_retry_respects_max_attempts() {
        let policy = RetryPolicy {
            max_attempts: 3,
            ..Default::default()
        };

        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
        assert!(!policy.should_retry(4));
    }

    #[test]
    fn job_lifecycle() {
        let tenant_id = TenantId::new();
        let mut job = Job::new(
            tenant_id,
            JobKind::custom("test"),
            serde_json::json!({"key": "value"}),
        );

        assert!(matches!(job.status, JobStatus::Pending));
        assert_eq!(job.attempt, 0);

        job.mark_running();
        assert!(matches!(job.status, JobStatus::Running));
        assert_eq!(job.attempt, 1);

        let started = Utc::now();
        job.mark_completed(started);
        assert!(matches!(job.status, JobStatus::Completed));
        assert_eq!(job.history.len(), 1);
        assert!(job.history[0].success);
    }

    #[test]
    fn job_failure_and_retry() {
        let tenant_id = TenantId::new();
        let mut job = Job::new(
            tenant_id,
            JobKind::custom("test"),
            serde_json::json!({}),
        )
        .with_retry_policy(RetryPolicy {
            max_attempts: 2,
            ..Default::default()
        });

        job.mark_running();
        let started = Utc::now();
        job.mark_failed("error 1".to_string(), started);

        assert!(matches!(job.status, JobStatus::Failed { .. }));
        assert!(job.scheduled_at.is_some());

        job.mark_running();
        let started = Utc::now();
        job.mark_failed("error 2".to_string(), started);

        assert!(matches!(job.status, JobStatus::DeadLettered { .. }));
    }
}

