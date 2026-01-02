//! AI orchestration adapters (optional subsystem).
//!
//! These components trigger AI jobs from projection updates or schedules.
//! Failures are isolated and must not impact core workflows.

pub mod inventory_anomaly_runner;

pub use inventory_anomaly_runner::{
    AiInsightSink, InMemoryAiInsightSink, InventoryAnomalyRunner, InventoryAnomalyRunnerHandle,
};


