//! The browser lane's sync↔async bridge (M12).
//!
//! The engine is synchronous; the browser executor ([`operator_exec_browser`])
//! is async (tokio + CDP over WebSocket). A naive `Runtime::new().block_on(...)`
//! would panic if the caller is *already* inside a tokio runtime. So the lane
//! owns a dedicated OS thread running its own multi-threaded tokio runtime, and
//! the engine talks to it over **std** channels only — no tokio primitives touch
//! the caller's thread, so it is safe from the sync CLI and from within another
//! runtime alike. The reactor stays live on the lane thread (multi-thread
//! runtime), so the executor's background WebSocket tasks keep running while the
//! engine blocks on a reply.

use std::path::PathBuf;
use std::sync::mpsc as smpsc;
use std::thread::JoinHandle;

use operator_core::types::Step;
use operator_exec_browser::{BrowserExecutor, BrowserLaunchOptions};
use serde_json::Value;
use tokio::sync::mpsc as tmpsc;

use crate::RuntimeError;

/// A command sent from the engine thread to the browser lane thread.
enum Cmd {
    /// Connect (if needed) and execute one browser step; reply with the result.
    Execute {
        step: Box<Step>,
        reply: smpsc::Sender<Result<Value, String>>,
    },
    /// Close the executor and end the lane thread; ack when done.
    Close(smpsc::Sender<()>),
}

/// A per-run browser lane: a background Chrome (isolated profile by default)
/// driven over CDP, exposed to the synchronous engine as a blocking `execute`.
pub struct BrowserLane {
    cmd_tx: tmpsc::UnboundedSender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl BrowserLane {
    /// Spawns the lane thread + runtime and constructs the (not-yet-connected)
    /// executor. Chrome is launched lazily on the first `execute`.
    pub fn spawn(
        allow_domains: Vec<String>,
        options: BrowserLaunchOptions,
    ) -> Result<Self, RuntimeError> {
        let (cmd_tx, mut cmd_rx) = tmpsc::unbounded_channel::<Cmd>();
        let (ready_tx, ready_rx) = smpsc::channel::<Result<(), String>>();

        let thread = std::thread::Builder::new()
            .name("operator-browser-lane".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("failed to build browser runtime: {e}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    let mut executor = BrowserExecutor::new(allow_domains).with_options(options);
                    let mut connected = false;
                    let _ = ready_tx.send(Ok(()));

                    while let Some(cmd) = cmd_rx.recv().await {
                        match cmd {
                            Cmd::Execute { step, reply } => {
                                if !connected {
                                    if let Err(e) = executor.connect().await {
                                        let _ = reply.send(Err(e.to_string()));
                                        continue;
                                    }
                                    connected = true;
                                }
                                let r = executor
                                    .execute_step(&step)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = reply.send(r);
                            }
                            Cmd::Close(ack) => {
                                executor.close().await;
                                let _ = ack.send(());
                                break;
                            }
                        }
                    }
                });
            })
            .map_err(|e| RuntimeError::Other(format!("failed to spawn browser lane: {e}")))?;

        // Wait for the runtime + executor to be constructed.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                cmd_tx,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(RuntimeError::Other(e)),
            Err(_) => Err(RuntimeError::Other(
                "browser lane thread exited before ready".to_string(),
            )),
        }
    }

    /// Executes one browser step, blocking the engine thread until the lane
    /// replies. Connects Chrome on the first call.
    pub fn execute(&self, step: &Step) -> Result<Value, RuntimeError> {
        let (reply_tx, reply_rx) = smpsc::channel();
        self.cmd_tx
            .send(Cmd::Execute {
                step: Box::new(step.clone()),
                reply: reply_tx,
            })
            .map_err(|_| RuntimeError::Other("browser lane is not running".to_string()))?;
        match reply_rx.recv() {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(RuntimeError::Other(format!("browser: {e}"))),
            Err(_) => Err(RuntimeError::Other(
                "browser lane dropped the reply".to_string(),
            )),
        }
    }

    /// Closes Chrome and joins the lane thread.
    pub fn shutdown(mut self) {
        let (ack_tx, ack_rx) = smpsc::channel();
        if self.cmd_tx.send(Cmd::Close(ack_tx)).is_ok() {
            let _ = ack_rx.recv();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for BrowserLane {
    fn drop(&mut self) {
        // Best-effort: if not shut down explicitly, signal close so Chrome isn't
        // orphaned. (No ack wait — Drop must not block indefinitely.)
        if self.thread.is_some() {
            let (ack_tx, _ack_rx) = smpsc::channel();
            let _ = self.cmd_tx.send(Cmd::Close(ack_tx));
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

/// Builds the lane's launch options from a domain allow-list and whether the
/// escalating `browser.real_profile` capability was granted. `profile_dir`
/// names a persistent profile; `None` = ephemeral per run.
pub fn lane_options(
    headless: bool,
    allow_real_profile: bool,
    profile_dir: Option<PathBuf>,
) -> BrowserLaunchOptions {
    BrowserLaunchOptions {
        user_data_dir: profile_dir,
        headless,
        allow_real_profile,
    }
}

/// A lazily-spawned browser lane owned by a run. The Chrome process/thread is
/// created on the first browser step (many runs never touch the browser lane),
/// and torn down by [`shutdown`](BrowserHandle::shutdown).
pub struct BrowserHandle {
    lane: Option<BrowserLane>,
    allow_domains: Vec<String>,
    options: BrowserLaunchOptions,
}

impl BrowserHandle {
    /// Creates a handle that will spawn its lane on first use.
    pub fn new(allow_domains: Vec<String>, options: BrowserLaunchOptions) -> Self {
        Self {
            lane: None,
            allow_domains,
            options,
        }
    }

    /// Executes a browser step, spawning the lane (and launching Chrome) on the
    /// first call.
    pub fn execute(&mut self, step: &Step) -> Result<Value, RuntimeError> {
        if self.lane.is_none() {
            self.lane = Some(BrowserLane::spawn(
                self.allow_domains.clone(),
                self.options.clone(),
            )?);
        }
        self.lane.as_ref().unwrap().execute(step)
    }

    /// Tears down the lane if it was spawned.
    pub fn shutdown(&mut self) {
        if let Some(lane) = self.lane.take() {
            lane.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_options_default_is_isolated() {
        // Default (no profile dir, not real-profile) = ephemeral isolated Chrome.
        let opts = lane_options(true, false, None);
        assert!(opts.user_data_dir.is_none());
        assert!(opts.headless);
        assert!(!opts.allow_real_profile);
    }

    #[test]
    fn lane_options_named_profile_and_real_profile() {
        let dir = PathBuf::from("/tmp/oj-profile");
        let opts = lane_options(false, true, Some(dir.clone()));
        assert_eq!(opts.user_data_dir, Some(dir));
        assert!(!opts.headless);
        assert!(opts.allow_real_profile);
    }

    #[test]
    fn handle_does_not_spawn_chrome_until_used() {
        // Constructing a handle must not launch Chrome (lazy); the lane is None.
        let h = BrowserHandle::new(vec![], lane_options(true, false, None));
        assert!(h.lane.is_none());
    }
}
