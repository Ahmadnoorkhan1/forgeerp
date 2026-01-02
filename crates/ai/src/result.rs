use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;

/// Result of an AI/ML inference.
///
/// This is *not* a domain event. It is an insight that can be persisted or displayed
/// by higher layers (infra/API) without mutating domain state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiResult {
    /// Primary score for the inference (model-specific meaning).
    pub score: f64,

    /// Confidence in \[0, 1\] (recommended convention; not enforced).
    pub confidence: f64,

    /// Optional human-readable explanation.
    pub explanation: Option<String>,

    /// Free-form metadata (model name, feature flags, timings, etc).
    pub metadata: JsonValue,
}

impl AiResult {
    pub fn new(score: f64, confidence: f64) -> Self {
        Self {
            score,
            confidence,
            explanation: None,
            metadata: JsonValue::Null,
        }
    }

    pub fn with_explanation(mut self, explanation: impl Into<String>) -> Self {
        self.explanation = Some(explanation.into());
        self
    }

    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = metadata;
        self
    }
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error("invalid job input: {0}")]
    InvalidInput(String),

    #[error("inference failed: {0}")]
    InferenceFailed(String),

    #[error("internal error: {0}")]
    Internal(String),
}


