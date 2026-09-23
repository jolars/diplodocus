//! Supervision outlives a caller that drops its startup future or session handle.

use std::future::{Future, pending, ready};
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::FailureSource;
use super::deadline::within;
use super::discovery::SelectedKernel;
use super::execution::execute_cells;
use super::page::{ExecutedCell, ExecutedCells};
use super::process::KernelProcess;
use super::transport::Channels;
use crate::execution::{
    ExecutionCancellation, ExecutionContext, ExecutionDeadlines, ExecutionFailure,
    ExecutionFailureKind, ExecutionPhase, PreparedCell,
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
    page: oneshot::Sender<PageWork>,
}

impl KernelSession {
    pub async fn execute(
        self,
        cells: Vec<PreparedCell>,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<ExecutedCells, ExecutionFailure> {
        self.execute_with(cells, cancellation, |cell| ready(Ok(cell)))
            .await
    }

    /// Process each completed cell before the supervisor can submit another.
    /// The caller may borrow its reducer and asset owner; the supervisor retains
    /// kernel ownership and watches cancellation and process exit while awaiting
    /// the fallible response. Dropping this future still wakes supervised cleanup.
    pub async fn execute_with<F, Fut>(
        mut self,
        cells: Vec<PreparedCell>,
        cancellation: &mut ExecutionCancellation<'_>,
        mut accept: F,
    ) -> Result<ExecutedCells, ExecutionFailure>
    where
        F: FnMut(ExecutedCell) -> Fut,
        Fut: Future<Output = Result<ExecutedCell, ExecutionFailure>>,
    {
        let (completed, completion) = oneshot::channel();
        let (output, mut outputs) = mpsc::channel::<PendingCell>(1);
        let _ = self.page.send(PageWork {
            cells,
            completed,
            output,
        });
        let processing = async {
            while let Some(pending) = outputs.recv().await {
                let result = accept(pending.cell).await;
                if pending.accepted.send(result).is_err() {
                    break;
                }
            }
            pending::<()>().await
        };
        let (result, cancelled) = tokio::select! {
            biased;
            _ = cancellation => {
                self.handle.stop(Stop::Cancel);
                (None, true)
            }
            result = completion => (result.ok(), false),
            _ = processing => unreachable!("cell processing waits for page completion"),
        };
        self.handle.finish().await?;
        if cancelled {
            return Err(self.handle.source.failure(
                ExecutionFailureKind::Cancelled,
                "Page execution was canceled.",
            ));
        }
        result.ok_or_else(|| {
            self.handle.source.failure(
                ExecutionFailureKind::Protocol,
                "The kernel stopped before completing the page.",
            )
        })
    }

    pub async fn shutdown(mut self) -> Result<(), ExecutionFailure> {
        self.handle.stop(Stop::Shutdown);
        self.handle.finish().await
    }

    pub async fn cancel(mut self) -> Result<(), ExecutionFailure> {
        self.handle.stop(Stop::Cancel);
        self.handle.finish().await
    }
}

pub(super) enum Stop {
    Shutdown,
    Cancel,
}

struct PageWork {
    cells: Vec<PreparedCell>,
    completed: oneshot::Sender<ExecutedCells>,
    output: mpsc::Sender<PendingCell>,
}

pub(super) struct PendingCell {
    pub cell: ExecutedCell,
    pub accepted: oneshot::Sender<Result<ExecutedCell, ExecutionFailure>>,
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
    let (page, requested) = oneshot::channel();
    let task = tokio::spawn(supervise(kernel.clone(), inputs, stopped, ready, requested));
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
                return Ok(KernelSession { runtime, kernel, handle, page });
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
    requested: oneshot::Receiver<PageWork>,
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
        result = within(Duration::from_millis(inputs.deadlines.startup), startup) => {
            result.unwrap_or_else(|_| Err(inputs.source.failure(
                ExecutionFailureKind::Timeout { phase: ExecutionPhase::Startup },
                "Kernel startup timed out.")))
        }
    };
    let mut completed = None;
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
                    request = requested => {
                        match request {
                            Ok(work) => {
                                match execute_cells(
                                    work.cells,
                                    &kernel.language,
                                    channels.as_mut().expect("connected channels"),
                                    process.child.as_mut().expect("spawned child"),
                                    &mut stopped,
                                    &inputs,
                                    &work.output,
                                ).await {
                                    Ok(cells) => {
                                        completed = Some((work.completed, cells));
                                        Ok(())
                                    }
                                    Err(failure) => Err(failure),
                                }
                            }
                            Err(_) => Err(inputs.source.failure(ExecutionFailureKind::Cancelled, "The kernel session was dropped.")),
                        }
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
        if matches!(failure.kind, ExecutionFailureKind::Cancelled | ExecutionFailureKind::Timeout { .. }
            | ExecutionFailureKind::OutputValidation | ExecutionFailureKind::AssetOutsideBoundary
            | ExecutionFailureKind::AssetMissing | ExecutionFailureKind::AssetCollision));
    let cleanup = process
        .cleanup(&mut channels, &kernel, &inputs, interrupt)
        .await;
    match outcome {
        Err(mut failure) => {
            failure.diagnostics.splice(0..0, kernel.diagnostics);
            failure.cleanup_diagnostics.extend(cleanup);
            Err(failure)
        }
        Ok(()) if cleanup.is_empty() => {
            if let Some((sender, cells)) = completed {
                let _ = sender.send(cells);
            }
            Ok(())
        }
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
