//! Supervision outlives a caller that drops its startup future or session handle.

use std::future::{Future, pending, ready};
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::FailureSource;
use super::deadline::until;
use super::discovery::SelectedKernel;
#[cfg(test)]
use super::execution::CellEvent;
use super::execution::execute_cells;
use super::launch::{ResolvedKernel, with_discovery};
use super::page::{ExecutedCell, ExecutedCells};
use super::process::KernelProcess;
use super::transport::Channels;
use crate::execution::identity::LaunchIdentityInput;
use crate::execution::{
    ExecutionCancellation, ExecutionContext, ExecutionDeadlines, ExecutionFailure,
    ExecutionFailureKind, ExecutionPhase, PreparedCell,
};

mod failure;
pub(super) use failure::SessionFailure;

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
    #[cfg(test)]
    pub async fn execute(
        self,
        cells: Vec<PreparedCell>,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<ExecutedCells, ExecutionFailure> {
        self.execute_with(cells, cancellation, |mut cell| {
            cell.events
                .retain(|event| !matches!(event, CellEvent::Warning(_)));
            ready(Ok(cell))
        })
        .await
    }

    /// Process each completed cell before the supervisor can submit another.
    /// The caller may borrow its reducer and asset owner; the supervisor retains
    /// kernel ownership and watches cancellation and process exit while awaiting
    /// the fallible response. Dropping this future still wakes supervised cleanup.
    #[cfg(test)]
    pub async fn execute_with<F, Fut>(
        self,
        cells: Vec<PreparedCell>,
        cancellation: &mut ExecutionCancellation<'_>,
        accept: F,
    ) -> Result<ExecutedCells, ExecutionFailure>
    where
        F: FnMut(ExecutedCell) -> Fut,
        Fut: Future<Output = Result<ExecutedCell, ExecutionFailure>>,
    {
        self.execute_detailed(cells, cancellation, accept)
            .await
            .map_err(|failure| failure.into_failure())
    }

    /// A rejected cell must carry its reducer's complete diagnostic ledger.
    /// Transport failures retain their separate pending protocol warnings.
    pub async fn execute_detailed<F, Fut>(
        mut self,
        cells: Vec<PreparedCell>,
        cancellation: &mut ExecutionCancellation<'_>,
        mut accept: F,
    ) -> Result<ExecutedCells, SessionFailure>
    where
        F: FnMut(ExecutedCell) -> Fut,
        Fut: Future<Output = Result<ExecutedCell, ExecutionFailure>>,
    {
        let (completed, mut completion) = oneshot::channel();
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
            result = &mut completion => (result.ok(), false),
            _ = processing => unreachable!("cell processing waits for page completion"),
        };
        self.handle.finish_detailed().await?;
        if cancelled {
            let mut failure: SessionFailure = self
                .handle
                .source
                .failure(
                    ExecutionFailureKind::Cancelled,
                    "Page execution was canceled.",
                )
                .into();
            failure.collection = self.handle.source.collection.clone();
            failure.before = self.kernel.diagnostics.clone();
            failure.previous = completion
                .try_recv()
                .map(|cells| cells.protocol_diagnostics)
                .unwrap_or_default();
            return Err(failure);
        }
        result.ok_or_else(|| {
            let mut failure: SessionFailure = self
                .handle
                .source
                .failure(
                    ExecutionFailureKind::Protocol,
                    "The kernel stopped before completing the page.",
                )
                .into();
            failure.before = self.kernel.diagnostics.clone();
            failure
        })
    }

    pub async fn shutdown(mut self) -> Result<(), ExecutionFailure> {
        self.handle.stop(Stop::Shutdown);
        self.handle.finish().await
    }

    #[cfg(test)]
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
    task: JoinHandle<Result<(), SessionFailure>>,
    source: FailureSource,
}

impl SessionHandle {
    fn stop(&mut self, reason: Stop) {
        if let Some(sender) = self.stop.take() {
            let _ = sender.send(reason);
        }
    }

    async fn finish(&mut self) -> Result<(), ExecutionFailure> {
        self.finish_detailed()
            .await
            .map_err(|failure| failure.into_failure())
    }

    async fn finish_detailed(&mut self) -> Result<(), SessionFailure> {
        (&mut self.task).await.unwrap_or_else(|_| {
            Err(self
                .source
                .failure(
                    ExecutionFailureKind::Cleanup,
                    "The kernel supervisor stopped unexpectedly.",
                )
                .into())
        })
    }
}

#[cfg(test)]
pub(super) async fn start_session(
    kernel: SelectedKernel,
    context: &mut ExecutionContext<'_>,
    source: FailureSource,
) -> Result<KernelSession, ExecutionFailure> {
    let diagnostics = kernel.diagnostics.clone();
    let deadline = startup_deadline(context.deadlines, &source)
        .map_err(|failure| with_discovery(failure, &diagnostics))?;
    let repositories = std::collections::BTreeMap::from([(
        source.source.repository.clone(),
        context.repository_root.clone(),
    )]);
    let resolving = ResolvedKernel::resolve(
        kernel,
        repositories,
        &context.repository_root,
        &context.page_path,
        &source,
    );
    let resolved = tokio::select! {
        biased;
        _ = &mut context.cancellation => {
            return Err(with_discovery(source.failure(ExecutionFailureKind::Cancelled, "Kernel startup was canceled."), &diagnostics));
        }
        resolved = until(deadline, resolving) => resolved
            .map_err(|_| with_discovery(source.failure(
                ExecutionFailureKind::Timeout { phase: ExecutionPhase::Startup },
                "Kernel launch resolution timed out.",
            ), &diagnostics))??,
    };
    start_resolved_before(resolved, context, source, deadline).await
}

pub(super) async fn start_resolved_session(
    resolved: ResolvedKernel,
    context: &mut ExecutionContext<'_>,
    source: FailureSource,
) -> Result<KernelSession, ExecutionFailure> {
    let deadline = startup_deadline(context.deadlines, &source)
        .map_err(|failure| with_discovery(failure, resolved.diagnostics()))?;
    start_resolved_before(resolved, context, source, deadline).await
}

fn startup_deadline(
    limits: ExecutionDeadlines,
    source: &FailureSource,
) -> Result<Instant, ExecutionFailure> {
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
    Instant::now()
        .checked_add(Duration::from_millis(limits.startup))
        .ok_or_else(|| {
            source.failure(
                ExecutionFailureKind::Startup,
                "The startup deadline is too large.",
            )
        })
}

pub(super) async fn start_resolved_before(
    resolved: ResolvedKernel,
    context: &mut ExecutionContext<'_>,
    source: FailureSource,
    deadline: Instant,
) -> Result<KernelSession, ExecutionFailure> {
    tokio::select! {
        biased;
        _ = &mut context.cancellation => {
            return Err(with_discovery(source.failure(ExecutionFailureKind::Cancelled, "Kernel startup was canceled."), resolved.diagnostics()));
        }
        _ = ready(()) => {}
    }
    let (kernel, launch) = resolved.into_parts();
    let inputs = SessionInputs {
        repository_root: context.repository_root.clone(),
        page_path: context.page_path.clone(),
        deadlines: context.deadlines,
        startup_deadline: deadline,
        source: source.clone(),
        launch,
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
    pub startup_deadline: Instant,
    pub source: FailureSource,
    pub launch: LaunchIdentityInput,
}

async fn supervise(
    kernel: SelectedKernel,
    inputs: SessionInputs,
    mut stopped: oneshot::Receiver<Stop>,
    ready: oneshot::Sender<KernelRuntime>,
    requested: oneshot::Receiver<PageWork>,
) -> Result<(), SessionFailure> {
    let mut process = KernelProcess::default();
    let mut channels = None;
    let startup = async {
        let connection = process.launch(&inputs).await?;
        let runtime = tokio::select! {
            _ = process.child.as_mut().expect("spawned child").wait() => {
                return Err(inputs.source.failure(ExecutionFailureKind::Protocol, "The kernel exited during startup."));
            }
            result = async {
                channels = Some(Channels::connect(&connection, &inputs.source).await?);
                channels.as_mut().expect("connected channels").handshake(inputs.launch.language(), &inputs.source).await
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
        result = until(inputs.startup_deadline, startup) => {
            result.unwrap_or_else(|_| Err(inputs.source.failure(
                ExecutionFailureKind::Timeout { phase: ExecutionPhase::Startup },
                "Kernel startup timed out.")))
        }
    };
    let mut completed = None;
    let outcome = match outcome {
        Ok(runtime) => {
            if ready.send(runtime).is_err() {
                Err(inputs
                    .source
                    .failure(
                        ExecutionFailureKind::Cancelled,
                        "The kernel session was dropped.",
                    )
                    .into())
            } else {
                tokio::select! {
                    biased;
                    stop = &mut stopped => match stop {
                        Ok(Stop::Shutdown) => Ok(()),
                        _ => Err(inputs.source.failure(ExecutionFailureKind::Cancelled, "The kernel session was canceled.").into()),
                    },
                    _ = process.child.as_mut().expect("spawned child").wait() => {
                        Err(inputs.source.failure(ExecutionFailureKind::Protocol, "The kernel exited unexpectedly.").into())
                    }
                    request = requested => {
                        match request {
                            Ok(work) => {
                                match execute_cells(
                                    work.cells,
                                    inputs.launch.language(),
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
                            Err(_) => Err(inputs.source.failure(ExecutionFailureKind::Cancelled, "The kernel session was dropped.").into()),
                        }
                    }
                }
            }
        }
        Err(failure) => {
            drop(ready);
            Err(failure.into())
        }
    };
    let interrupt = matches!(&outcome, Err(failure)
        if matches!(failure.failure.kind, ExecutionFailureKind::Cancelled | ExecutionFailureKind::Timeout { .. }
            | ExecutionFailureKind::OutputValidation | ExecutionFailureKind::AssetOutsideBoundary
            | ExecutionFailureKind::AssetMissing | ExecutionFailureKind::AssetCollision));
    let cleanup = process.cleanup(&mut channels, &inputs, interrupt).await;
    match outcome {
        Err(mut failure) => {
            failure.before = kernel.diagnostics;
            failure.collection = inputs.source.collection.clone();
            failure.failure.cleanup_diagnostics.extend(cleanup);
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
            let mut failure: SessionFailure = failure.into();
            failure.before = kernel.diagnostics;
            failure.collection = inputs.source.collection.clone();
            failure.previous = completed
                .map(|(_, cells)| cells.protocol_diagnostics)
                .unwrap_or_default();
            Err(failure)
        }
    }
}
