//! Background job system with retry, backoff, and dead-letter handling.
//!
//! ## Design
//!
//! - Jobs are tenant-scoped and typed
//! - Retry policy with exponential backoff
//! - Dead-letter queue for failed jobs after max retries
//! - Visibility into job status and failures
//! - Integrates with AI jobs, projection rebuilds, saga steps
//!
//! ## Components
//!
//! - `Job`: Core job abstraction with payload and metadata
//! - `JobStore`: Persistence for jobs (in-memory or durable)
//! - `JobExecutor`: Runs jobs with retry logic
//! - `DeadLetterQueue`: Failed jobs for inspection/replay

pub mod executor;
pub mod store;
pub mod types;

pub use executor::{JobExecutor, JobExecutorHandle};
pub use store::{InMemoryJobStore, JobStore};
pub use types::{
    BackoffStrategy, DeadLetterEntry, Job, JobId, JobKind, JobResult, JobStatus, RetryPolicy,
};

