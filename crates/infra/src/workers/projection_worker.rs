use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tracing::warn;

use forgeerp_core::TenantId;
use forgeerp_events::{EventBus, Subscription, TenantScoped};

/// Handle to control and join a background worker.
#[derive(Debug)]
pub struct WorkerHandle {
    shutdown: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}

impl WorkerHandle {
    /// Request graceful shutdown and wait for the worker to stop.
    pub fn shutdown(mut self) {
        let _ = self.shutdown.send(());
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Generic projection worker loop.
///
/// - Subscribes to an event bus
/// - Applies an idempotent handler for each message
/// - Supports graceful shutdown
/// - Optional tenant filtering for safe initialization
#[derive(Debug)]
pub struct ProjectionWorker;

impl ProjectionWorker {
    /// Spawn a worker thread that processes events from the bus subscription.
    ///
    /// - `tenant_id`: when provided, messages for other tenants are ignored
    /// - `handler`: must be idempotent (at-least-once delivery safe)
    pub fn spawn<M, B, H, E>(
        name: &'static str,
        bus: B,
        tenant_id: Option<TenantId>,
        mut handler: H,
    ) -> WorkerHandle
    where
        M: TenantScoped + Send + 'static,
        B: EventBus<M> + Send + Sync + 'static,
        H: FnMut(M) -> Result<(), E> + Send + 'static,
        E: core::fmt::Debug + Send + 'static,
    {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let sub: Subscription<M> = bus.subscribe();

        let join = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || worker_loop(name, sub, shutdown_rx, tenant_id, &mut handler))
            .expect("failed to spawn projection worker thread");

        WorkerHandle {
            shutdown: shutdown_tx,
            join: Some(join),
        }
    }
}

fn worker_loop<M, H, E>(
    name: &'static str,
    sub: Subscription<M>,
    shutdown_rx: mpsc::Receiver<()>,
    tenant_id: Option<TenantId>,
    handler: &mut H,
) where
    M: TenantScoped,
    H: FnMut(M) -> Result<(), E>,
    E: core::fmt::Debug,
{
    let tick = Duration::from_millis(250);

    loop {
        // Shutdown check (non-blocking)
        if shutdown_rx.try_recv().is_ok() {
            break;
        }

        match sub.recv_timeout(tick) {
            Ok(msg) => {
                if let Some(t) = tenant_id {
                    if msg.tenant_id() != t {
                        // Tenant-safe: ignore other tenants.
                        continue;
                    }
                }

                if let Err(err) = handler(msg) {
                    warn!(worker = name, error = ?err, "projection worker handler failed");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}


