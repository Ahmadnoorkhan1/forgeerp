use serde::{Deserialize, Serialize};
use serde_json::json;

use forgeerp_core::TenantId;

use crate::job::AiJob;
use crate::result::{AiError, AiResult};
use crate::scheduler::{InventoryItemSnapshot, InventorySnapshot};

/// Inventory anomaly detection output (AI insight).
///
/// This is an AI result payload, not a domain event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnomalyDetected {
    pub item_id: String,
    pub severity: f64,
    pub explanation: String,
}

/// Deterministic anomaly detection job for inventory stock movements.
///
/// Model:
/// - Convert quantity time-series into deltas.
/// - Compare the most recent delta against a rolling window of previous deltas.
/// - Flag if the z-score exceeds `z_threshold`.
#[derive(Debug, Clone)]
pub struct InventoryAnomalyJob {
    tenant_id: TenantId,
    input: InventorySnapshot,
    /// Rolling window size for baseline deltas (must be >= 2 to compute stddev).
    window: usize,
    /// Z-score threshold (e.g., 3.0).
    z_threshold: f64,
}

impl InventoryAnomalyJob {
    pub fn new(tenant_id: TenantId, input: InventorySnapshot) -> Self {
        Self {
            tenant_id,
            input,
            window: 10,
            z_threshold: 3.0,
        }
    }

    pub fn with_window(mut self, window: usize) -> Self {
        self.window = window;
        self
    }

    pub fn with_z_threshold(mut self, z_threshold: f64) -> Self {
        self.z_threshold = z_threshold;
        self
    }
}

impl AiJob for InventoryAnomalyJob {
    type Input = InventorySnapshot;

    fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    fn input(&self) -> &Self::Input {
        &self.input
    }

    fn run(&self) -> Result<AiResult, AiError> {
        if self.input.tenant_id != self.tenant_id {
            return Err(AiError::InvalidInput(
                "tenant_id mismatch between job and snapshot".to_string(),
            ));
        }

        if self.window < 2 {
            return Err(AiError::InvalidInput(
                "window must be >= 2 to compute standard deviation".to_string(),
            ));
        }

        if !(self.z_threshold.is_finite() && self.z_threshold > 0.0) {
            return Err(AiError::InvalidInput(
                "z_threshold must be a finite positive number".to_string(),
            ));
        }

        let mut anomalies: Vec<AnomalyDetected> = Vec::new();

        for item in &self.input.items {
            if let Some(a) = detect_item_anomaly(item, self.window, self.z_threshold) {
                anomalies.push(a);
            }
        }

        // Emit a single AI result containing all anomalies for the tenant snapshot.
        // (Callers can fan this out if they want per-item messages.)
        let score = anomalies.len() as f64;
        let confidence = if anomalies.is_empty() { 1.0 } else { 0.8 };

        Ok(AiResult::new(score, confidence)
            .with_explanation(format!(
                "detected {} anomalous inventory movement(s) using rolling z-score (window={}, threshold={})",
                anomalies.len(),
                self.window,
                self.z_threshold
            ))
            .with_metadata(json!({
                "kind": "inventory.anomaly_detection",
                "tenant_id": self.tenant_id.to_string(),
                "window": self.window,
                "z_threshold": self.z_threshold,
                "anomalies": anomalies,
            })))
    }
}

fn detect_item_anomaly(
    item: &InventoryItemSnapshot,
    window: usize,
    z_threshold: f64,
) -> Option<AnomalyDetected> {
    // Need at least 3 points: 2 for deltas, and >=2 baseline deltas to compute stddev.
    // Baseline deltas count = window; total points needed = window + 2.
    if item.historical_trend.len() < window + 2 {
        return None;
    }

    // Build deltas.
    let mut deltas: Vec<f64> = Vec::with_capacity(item.historical_trend.len().saturating_sub(1));
    for i in 1..item.historical_trend.len() {
        let prev = item.historical_trend[i - 1] as f64;
        let cur = item.historical_trend[i] as f64;
        deltas.push(cur - prev);
    }

    // Last delta is what we evaluate.
    let last_delta = *deltas.last().unwrap_or(&0.0);

    // Rolling baseline: previous `window` deltas immediately preceding the last delta.
    let start = deltas.len().saturating_sub(window + 1);
    let end = deltas.len().saturating_sub(1);
    let baseline = &deltas[start..end];

    let mean = mean(baseline);
    let std = stddev_sample(baseline, mean);

    // If std is ~0, any deviation from mean is potentially anomalous, but we keep it conservative.
    if std <= f64::EPSILON {
        if (last_delta - mean).abs() > 0.0 {
            let explanation = format!(
                "item {} moved by {last_delta:.2} units; baseline movement is constant at {mean:.2} (std≈0)",
                item.item_id
            );
            return Some(AnomalyDetected {
                item_id: item.item_id.clone(),
                severity: 1.0,
                explanation,
            });
        }
        return None;
    }

    let z = (last_delta - mean) / std;
    let az = z.abs();

    if az < z_threshold {
        return None;
    }

    // Severity scales with how far beyond threshold we are (>= 1.0 means at threshold).
    let severity = az / z_threshold;
    let explanation = format!(
        "item {} moved by {last_delta:.2} units; baseline mean={mean:.2}, std={std:.2}, z={z:.2} (threshold={z_threshold:.2})",
        item.item_id
    );

    Some(AnomalyDetected {
        item_id: item.item_id.clone(),
        severity,
        explanation,
    })
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / (xs.len() as f64)
}

/// Sample standard deviation (n-1), deterministic.
fn stddev_sample(xs: &[f64], mean: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let var = xs
        .iter()
        .map(|x| {
            let d = x - mean;
            d * d
        })
        .sum::<f64>()
        / ((xs.len() - 1) as f64);
    var.sqrt()
}


