//! Supervision outlives a caller that drops its startup future or session handle.

use std::future::ready;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::FailureSource;
use super::discovery::SelectedKernel;
use super::process::KernelProcess;
use super::transport::Channels;
use crate::execution::{
    ExecutionContext, ExecutionDeadlines, ExecutionFailure, ExecutionFailureKind, ExecutionPhase,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KernelRuntime {
    pub implementation: String,
    pub implementation_version: String,
    pub language: String,
    pub language_version: String,
    pub protocol_version: String,
}

pub(super) struct KernelSession {
    pub runtime: KernelRuntime,
    pub kernel: SelectedKernel,
    handle: SessionHandle,
}

impl KernelSession {
    pub async fn shutdown(mut self) -> Result<(), ExecutionFailure> {
        self.handle.stop(Stop::Shutdown);
        self.handle.finish().await
    }

    pub async fn cancel(mut self) -> Result<(), ExecutionFailure> {
        self.handle.stop(Stop::Cancel);
        self.handle.finish().await
    }
}

enum Stop {
    Shutdown,
    Cancel,
}

struct SessionHandle {
    // Dropping this sender also wakes the supervisor, including during startup.
    stop: Option<oneshot::Sender<Stop>>,
    task: JoinHandle<Result<(), ExecutionFailure>>,
    source: FailureSource,
}

impl SessionHandle {
    fn stop(&mut self, reason: Stop) {
        if let Some(sender) = self.stop.take() {
            let _ = sender.send(reason);
        }
    }

    async fn finish(&mut self) -> Result<(), ExecutionFailure> {
        (&mut self.task).await.unwrap_or_else(|_| {
            Err(self.source.failure(
                ExecutionFailureKind::Cleanup,
                "The kernel supervisor stopped unexpectedly.",
            ))
        })
    }
}

pub(super) async fn start_session(
    kernel: SelectedKernel,
    context: &mut ExecutionContext<'_>,
    source: FailureSource,
) -> Result<KernelSession, ExecutionFailure> {
    tokio::select! {
        biased;
        _ = &mut context.cancellation => {
            return Err(source.failure(ExecutionFailureKind::Cancelled, "Kernel startup was canceled."));
        }
        _ = ready(()) => {}
    }
    let limits = context.deadlines;
    if [
        limits.startup,
        limits.cell,
        limits.terminal_sync,
        limits.interrupt,
        limits.shutdown,
        limits.termination,
        limits.forced_exit,
    ]
    .contains(&0)
    {
        return Err(source.failure(
            ExecutionFailureKind::Startup,
            "Execution deadlines must be positive.",
        ));
    }
    let inputs = SessionInputs {
        repository_root: context.repository_root.clone(),
        page_path: context.page_path.clone(),
        deadlines: limits,
        source: source.clone(),
    };
    let (stop, stopped) = oneshot::channel();
    let (ready, started) = oneshot::channel();
    let task = tokio::spawn(supervise(kernel.clone(), inputs, stopped, ready));
    let mut handle = SessionHandle {
        stop: Some(stop),
        task,
        source: source.clone(),
    };
    tokio::select! {
        biased;
        _ = &mut context.cancellation => { handle.stop(Stop::Cancel); }
        result = started => {
            if let Ok(runtime) = result {
                return Ok(KernelSession { runtime, kernel, handle });
            }
        }
    }
    handle.finish().await?;
    Err(source.failure(
        ExecutionFailureKind::Protocol,
        "The kernel stopped before readiness was established.",
    ))
}

pub(super) struct SessionInputs {
    pub repository_root: PathBuf,
    pub page_path: PathBuf,
    pub deadlines: ExecutionDeadlines,
    pub source: FailureSource,
}

async fn supervise(
    kernel: SelectedKernel,
    inputs: SessionInputs,
    mut stopped: oneshot::Receiver<Stop>,
    ready: oneshot::Sender<KernelRuntime>,
) -> Result<(), ExecutionFailure> {
    let mut process = KernelProcess::default();
    let mut channels = None;
    let startup = async {
        let connection = process.launch(&kernel, &inputs).await?;
        let runtime = tokio::select! {
            _ = process.child.as_mut().expect("spawned child").wait() => {
                return Err(inputs.source.failure(ExecutionFailureKind::Protocol, "The kernel exited during startup."));
            }
            result = async {
                channels = Some(Channels::connect(&connection, &inputs.source).await?);
                channels.as_mut().expect("connected channels").handshake(&kernel.language, &inputs.source).await
            } => result?,
        };
        if process
            .child
            .as_mut()
            .expect("spawned child")
            .try_wait()
            .map_err(|_| {
                inputs.source.failure(
                    ExecutionFailureKind::Protocol,
                    "Kernel liveness could not be checked.",
                )
            })?
            .is_some()
        {
            return Err(inputs.source.failure(
                ExecutionFailureKind::Protocol,
                "The kernel exited during startup.",
            ));
        }
        Ok(runtime)
    };
    let outcome = tokio::select! {
        biased;
        _ = &mut stopped => Err(inputs.source.failure(ExecutionFailureKind::Cancelled, "Kernel startup was canceled.")),
        result = timeout(Duration::from_millis(inputs.deadlines.startup), startup) => {
            result.unwrap_or_else(|_| Err(inputs.source.failure(
                ExecutionFailureKind::Timeout { phase: ExecutionPhase::Startup },
                "Kernel startup timed out.")))
        }
    };
    let outcome = match outcome {
        Ok(runtime) => {
            if ready.send(runtime).is_err() {
                Err(inputs.source.failure(
                    ExecutionFailureKind::Cancelled,
                    "The kernel session was dropped.",
                ))
            } else {
                tokio::select! {
                    biased;
                    stop = &mut stopped => match stop {
                        Ok(Stop::Shutdown) => Ok(()),
                        _ => Err(inputs.source.failure(ExecutionFailureKind::Cancelled, "The kernel session was canceled.")),
                    },
                    _ = process.child.as_mut().expect("spawned child").wait() => {
                        Err(inputs.source.failure(ExecutionFailureKind::Protocol, "The kernel exited unexpectedly."))
                    }
                }
            }
        }
        Err(failure) => {
            drop(ready);
            Err(failure)
        }
    };
    let interrupt = matches!(&outcome, Err(failure)
        if matches!(failure.kind, ExecutionFailureKind::Cancelled | ExecutionFailureKind::Timeout { .. }));
    let cleanup = process
        .cleanup(&mut channels, &kernel, &inputs, interrupt)
        .await;
    match outcome {
        Err(mut failure) => {
            failure.diagnostics.splice(0..0, kernel.diagnostics);
            failure.cleanup_diagnostics.extend(cleanup);
            Err(failure)
        }
        Ok(()) if cleanup.is_empty() => Ok(()),
        Ok(()) => {
            let mut failure = inputs.source.failure(
                ExecutionFailureKind::Cleanup,
                "The kernel session could not be fully cleaned up.",
            );
            failure.cleanup_diagnostics = cleanup;
            Err(failure)
        }
    }
}
