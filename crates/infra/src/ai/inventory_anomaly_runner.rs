use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use forgeerp_core::TenantId;
use forgeerp_ai::{
    AiResult, AiScheduler, InventoryAnomalyJob, InventorySnapshot, LocalAiScheduler, ReadModelReader,
    TenantScope,
};

/// Sink for AI insights.
///
/// This is intentionally separate from the domain event stream:
/// AI outputs are *insights*, not domain events.
pub trait AiInsightSink: Send + Sync + 'static {
    fn emit(&self, tenant_id: TenantId, result: AiResult);
}

/// In-memory sink for tests/dev.
#[derive(Debug, Default)]
pub struct InMemoryAiInsightSink {
    inner: std::sync::Mutex<Vec<(TenantId, AiResult)>>,
}

impl InMemoryAiInsightSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all(&self) -> Vec<(TenantId, AiResult)> {
        self.inner.lock().unwrap().clone()
    }
}

impl AiInsightSink for InMemoryAiInsightSink {
    fn emit(&self, tenant_id: TenantId, result: AiResult) {
        self.inner.lock().unwrap().push((tenant_id, result));
    }
}

/// Config for the inventory anomaly runner.
#[derive(Debug, Clone)]
pub struct InventoryAnomalyRunner {
    pub interval: Duration,
    pub max_retries: u32,
    pub base_backoff: Duration,
    pub window: usize,
    pub z_threshold: f64,
}

impl Default for InventoryAnomalyRunner {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            max_retries: 5,
            base_backoff: Duration::from_millis(250),
            window: 10,
            z_threshold: 3.0,
        }
    }
}

/// Handle for the running AI runner (shutdown + trigger hook).
#[derive(Debug)]
pub struct InventoryAnomalyRunnerHandle {
    shutdown: mpsc::Sender<()>,
    trigger: mpsc::SyncSender<()>,
    join: Option<thread::JoinHandle<()>>,
}

impl InventoryAnomalyRunnerHandle {
    /// Event-trigger hook: call after a successful projection update.
    ///
    /// Backpressure: triggers are coalesced (bounded queue). If the runner is already pending,
    /// this becomes a no-op.
    pub fn trigger(&self) {
        // Coalesce: channel capacity=1; ignore if already full.
        let _ = self.trigger.try_send(());
    }

    /// Gracefully stop the runner thread.
    pub fn shutdown(mut self) {
        let _ = self.shutdown.send(());
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl InventoryAnomalyRunner {
    /// Spawn a tenant-scoped runner.
    ///
    /// - Schedule: runs every `interval`
    /// - Event-trigger: call `handle.trigger()` after projection updates
    /// - Failures: logged + retried with bounded exponential backoff; never propagate
    pub fn spawn_for_tenant<R, S>(
        &self,
        name: &'static str,
        tenant_id: TenantId,
        reader: Arc<R>,
        sink: Arc<S>,
    ) -> InventoryAnomalyRunnerHandle
    where
        R: ReadModelReader<InventorySnapshot> + 'static,
        S: AiInsightSink + 'static,
    {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (trigger_tx, trigger_rx) = mpsc::sync_channel::<()>(1);

        let cfg = self.clone();
        let join = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                runner_loop(
                    name,
                    tenant_id,
                    cfg,
                    shutdown_rx,
                    trigger_rx,
                    reader,
                    sink,
                )
            })
            .expect("failed to spawn inventory anomaly runner thread");

        InventoryAnomalyRunnerHandle {
            shutdown: shutdown_tx,
            trigger: trigger_tx,
            join: Some(join),
        }
    }
}

fn runner_loop<R, S>(
    name: &'static str,
    tenant_id: TenantId,
    cfg: InventoryAnomalyRunner,
    shutdown_rx: mpsc::Receiver<()>,
    trigger_rx: mpsc::Receiver<()>,
    reader: Arc<R>,
    sink: Arc<S>,
) where
    R: ReadModelReader<InventorySnapshot> + 'static,
    S: AiInsightSink + 'static,
{
    info!(runner = name, tenant = %tenant_id, "inventory anomaly runner started");

    let scheduler = LocalAiScheduler::new(TenantScope::Tenant(tenant_id));

    let mut next_tick = Instant::now() + cfg.interval;
    let mut pending = true; // run once on startup
    let mut failures: u32 = 0;
    let mut backoff_until: Option<Instant> = None;

    loop {
        // Shutdown has priority.
        if shutdown_rx.try_recv().is_ok() {
            break;
        }

        let now = Instant::now();
        if now >= next_tick {
            pending = true;
            // Keep a stable cadence even if we were delayed.
            while next_tick <= now {
                next_tick += cfg.interval;
            }
        }

        // Event-trigger: non-blocking drain to coalesce multiple triggers.
        while trigger_rx.try_recv().is_ok() {
            pending = true;
        }

        // Backoff gate.
        if let Some(until) = backoff_until {
            if Instant::now() < until {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            backoff_until = None;
        }

        if !pending {
            // Wait until next tick or trigger or shutdown.
            let sleep_for = next_tick
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250));
            thread::sleep(sleep_for);
            continue;
        }

        pending = false;

        // 1) Get tenant snapshot (read model).
        let snapshot = match reader.get_snapshot(tenant_id) {
            Ok(s) => s,
            Err(e) => {
                warn!(runner = name, tenant = %tenant_id, error = ?e, "failed to get inventory snapshot");
                failures += 1;
                if failures <= cfg.max_retries {
                    pending = true;
                    backoff_until = Some(Instant::now() + backoff(cfg.base_backoff, failures));
                } else {
                    failures = 0;
                }
                continue;
            }
        };

        // 2) Run deterministic inference.
        let job = InventoryAnomalyJob::new(tenant_id, snapshot)
            .with_window(cfg.window)
            .with_z_threshold(cfg.z_threshold);

        match scheduler.run(job) {
            Ok(result) => {
                failures = 0;
                sink.emit(tenant_id, result);
            }
            Err(e) => {
                warn!(runner = name, tenant = %tenant_id, error = ?e, "inventory anomaly job failed");
                failures += 1;
                if failures <= cfg.max_retries {
                    pending = true;
                    backoff_until = Some(Instant::now() + backoff(cfg.base_backoff, failures));
                } else {
                    failures = 0;
                }
            }
        }
    }

    info!(runner = name, tenant = %tenant_id, "inventory anomaly runner stopped");
}

fn backoff(base: Duration, attempt: u32) -> Duration {
    // Exponential backoff: base * 2^(attempt-1), capped.
    let pow = 1u32 << attempt.saturating_sub(1).min(10);
    let ms = base.as_millis().saturating_mul(pow as u128);
    Duration::from_millis(ms.min(10_000) as u64)
}


