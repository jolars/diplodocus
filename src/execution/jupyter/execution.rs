//! Sequential cell submission and request-correlated terminal synchronization.

use std::time::Duration;

use jupyter_protocol::{
    ExecuteRequest, ExecutionState, JupyterMessage, JupyterMessageContent, ReplyStatus, Stdio,
};
use serde_json::{Map, Value};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout};

use super::FailureSource;
use super::page::{ExecutedCell, ExecutedCells, skip_reason};
use super::session::{PendingCell, SessionInputs, Stop};
use super::transport::Channels;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::execution::{
    CellOutcome, ExecutionDeadlines, ExecutionFailure, ExecutionFailureKind, ExecutionPhase,
    PreparedCell,
};
use crate::ir::StreamName;

/// These events contain untrusted kernel data, not validated `CellOutput` nodes.
/// Display IDs and raw tracebacks stay inside this nonserializable adapter model.
#[derive(Debug)]
pub(super) enum CellEvent {
    Stream {
        name: StreamName,
        text: String,
    },
    Display {
        bundle: MimeBundle,
        display_id: Option<String>,
    },
    UpdateDisplay {
        bundle: MimeBundle,
        display_id: Option<String>,
    },
    Result {
        bundle: MimeBundle,
        display_id: Option<String>,
    },
    Error {
        name: String,
        message: String,
        traceback: Vec<String>,
    },
    Clear {
        wait: bool,
    },
}

#[derive(Debug)]
pub(super) struct MimeBundle {
    pub data: Value,
    pub metadata: Map<String, Value>,
}

pub(super) async fn execute_cells(
    cells: Vec<PreparedCell>,
    language: &str,
    channels: &mut Channels,
    child: &mut Child,
    stopped: &mut oneshot::Receiver<Stop>,
    inputs: &SessionInputs,
    output: &mpsc::Sender<PendingCell>,
) -> Result<ExecutedCells, ExecutionFailure> {
    let mut result = ExecutedCells::default();
    for cell in cells {
        let source = inputs.source.for_cell(&cell);
        let execution = async {
            let completed = if let Some(reason) = skip_reason(&cell, language) {
                ExecutedCell {
                    ordinal: cell.ordinal,
                    outcome: CellOutcome::Skipped { reason },
                    events: Vec::new(),
                }
            } else {
                timeout(
                    Duration::from_millis(inputs.deadlines.cell),
                    channels.execute_cell(
                        &cell,
                        inputs.deadlines,
                        &source,
                        &mut result.diagnostics,
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    Err(source.failure(
                        ExecutionFailureKind::Timeout {
                            phase: ExecutionPhase::Cell,
                        },
                        "Cell execution timed out.",
                    ))
                })?
            };
            let (accepted, response) = oneshot::channel();
            output
                .send(PendingCell {
                    cell: completed,
                    accepted,
                })
                .await
                .map_err(|_| {
                    source.failure(
                        ExecutionFailureKind::Cancelled,
                        "The output consumer was dropped.",
                    )
                })?;
            response.await.map_err(|_| {
                source.failure(
                    ExecutionFailureKind::Cancelled,
                    "The output consumer was dropped.",
                )
            })?
        };
        let outcome = tokio::select! {
            biased;
            _ = &mut *stopped => Err(source.failure(ExecutionFailureKind::Cancelled, "Page execution was canceled.")),
            _ = child.wait() => Err(source.failure(ExecutionFailureKind::Protocol, "The kernel exited during cell execution.")),
            result = execution => result,
        };
        match outcome {
            Ok(cell) => result.cells.push(cell),
            Err(mut failure) => {
                failure.diagnostics.splice(0..0, result.diagnostics);
                return Err(failure);
            }
        }
    }
    Ok(result)
}

impl Channels {
    async fn execute_cell(
        &mut self,
        cell: &PreparedCell,
        deadlines: ExecutionDeadlines,
        source: &FailureSource,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<ExecutedCell, ExecutionFailure> {
        let protocol_failure = || {
            source.failure(
                ExecutionFailureKind::Protocol,
                "Kernel communication or execution protocol validation failed.",
            )
        };
        let request: JupyterMessage = ExecuteRequest {
            code: cell.cell.source.clone(),
            silent: false,
            store_history: true,
            user_expressions: None,
            allow_stdin: false,
            stop_on_error: true,
        }
        .into();
        let id = request.header.msg_id.clone();
        self.shell
            .send(request)
            .await
            .map_err(|_| protocol_failure())?;
        let mut reply = None;
        let mut idle = false;
        let mut terminal_deadline = None;
        let mut events = Vec::new();
        let mut language_error = false;
        while reply.is_none() || !idle {
            tokio::select! {
                biased;
                _ = async {
                    if let Some(deadline) = terminal_deadline { sleep_until(deadline).await; }
                    else { std::future::pending::<()>().await; }
                } => return Err(source.failure(ExecutionFailureKind::Timeout { phase: ExecutionPhase::TerminalSync }, "Cell terminal synchronization timed out.")),
                message = self.stdin.read() => {
                    let message = message.map_err(|_| protocol_failure())?;
                    if matches!(message.content, JupyterMessageContent::InputRequest(_)) {
                        return Err(source.failure(ExecutionFailureKind::InputRequested, "The kernel requested interactive input during cell execution."));
                    }
                    unsupported(diagnostics, source);
                }
                message = self.shell.read() => {
                    let message = message.map_err(|_| protocol_failure())?;
                    if !matches_parent(&message, &id) {
                        unsupported(diagnostics, source);
                        continue;
                    }
                    let JupyterMessageContent::ExecuteReply(response) = message.content else { return Err(protocol_failure()); };
                    if reply.is_some() || (response.status == ReplyStatus::Error && response.error.is_none())
                        || (response.status == ReplyStatus::Ok && response.error.is_some()) {
                        return Err(protocol_failure());
                    }
                    if !response.payload.is_empty() || response.user_expressions.as_ref().is_some_and(|values| !values.is_empty()) || !message.buffers.is_empty() {
                        unsupported(diagnostics, source);
                    }
                    if response.status == ReplyStatus::Aborted {
                        return Err(source.failure(ExecutionFailureKind::CellError, "The kernel aborted the cell."));
                    }
                    language_error |= response.status == ReplyStatus::Error;
                    reply = Some(response);
                    terminal_deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(deadlines.terminal_sync));
                }
                message = self.iopub.read() => {
                    let message = message.map_err(|_| protocol_failure())?;
                    if !matches_parent(&message, &id) || idle {
                        unsupported(diagnostics, source);
                        continue;
                    }
                    if !message.buffers.is_empty() { unsupported(diagnostics, source); }
                    match message.content {
                        JupyterMessageContent::Status(status) => match status.execution_state {
                            ExecutionState::Idle => {
                                idle = true;
                                terminal_deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(deadlines.terminal_sync));
                            }
                            ExecutionState::Busy => {},
                            _ => return Err(protocol_failure()),
                        },
                        JupyterMessageContent::ExecuteInput(_) => {},
                        JupyterMessageContent::StreamContent(stream) => events.push(CellEvent::Stream {
                            name: match stream.name { Stdio::Stdout => StreamName::Stdout, Stdio::Stderr => StreamName::Stderr }, text: stream.text,
                        }),
                        JupyterMessageContent::DisplayData(display) => events.push(CellEvent::Display {
                            bundle: MimeBundle { data: serde_json::to_value(display.data).map_err(|_| protocol_failure())?, metadata: display.metadata },
                            display_id: display.transient.and_then(|value| value.display_id),
                        }),
                        JupyterMessageContent::UpdateDisplayData(display) => events.push(CellEvent::UpdateDisplay {
                            bundle: MimeBundle { data: serde_json::to_value(display.data).map_err(|_| protocol_failure())?, metadata: display.metadata },
                            display_id: display.transient.display_id,
                        }),
                        JupyterMessageContent::ExecuteResult(display) => events.push(CellEvent::Result {
                            bundle: MimeBundle { data: serde_json::to_value(display.data).map_err(|_| protocol_failure())?, metadata: display.metadata },
                            display_id: display.transient.and_then(|value| value.display_id),
                        }),
                        JupyterMessageContent::ErrorOutput(error) => {
                            language_error = true;
                            events.push(CellEvent::Error { name: error.ename, message: error.evalue, traceback: error.traceback });
                        }
                        JupyterMessageContent::ClearOutput(clear) => events.push(CellEvent::Clear { wait: clear.wait }),
                        _ => unsupported(diagnostics, source),
                    }
                }
            }
        }
        if language_error && !cell.options.execution.error.value {
            return Err(source.failure(
                ExecutionFailureKind::CellError,
                "The cell raised a language error.",
            ));
        }
        if !events
            .iter()
            .any(|event| matches!(event, CellEvent::Error { .. }))
            && let Some(error) = reply.and_then(|reply| reply.error)
        {
            events.push(CellEvent::Error {
                name: error.ename,
                message: error.evalue,
                traceback: error.traceback,
            });
        }
        Ok(ExecutedCell {
            ordinal: cell.ordinal,
            outcome: if language_error {
                CellOutcome::AllowedError
            } else {
                CellOutcome::Ok
            },
            events,
        })
    }
}

fn matches_parent(message: &JupyterMessage, id: &str) -> bool {
    message
        .parent_header
        .as_ref()
        .is_some_and(|parent| parent.msg_id == id)
}

fn unsupported(diagnostics: &mut Vec<Diagnostic>, source: &FailureSource) {
    let mut diagnostic =
        ExecutionFailureKind::Protocol.to_diagnostic(&source.collection, source.source.clone());
    diagnostic.code = DiagnosticCode::UnsupportedKernelMessage;
    diagnostic.severity = Severity::Warning;
    diagnostic.message = "An unrelated, late, or unsupported kernel message was ignored.".into();
    diagnostics.push(diagnostic);
}
