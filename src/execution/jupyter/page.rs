//! Run one prepared page; output validation and publishable records are separate.

use super::FailureSource;
use super::discovery::{SearchEnvironment, SelectedKernel, discover_kernel, normalize_language};
use super::execution::CellEvent;
use super::session::{KernelRuntime, start_session};
use crate::configuration::ExecutionMode;
use crate::diagnostics::Diagnostic;
use crate::documents::AuthoredFormat;
use crate::execution::{
    CellOutcome, CellSkipReason, ExecutionContext, ExecutionFailure, ExecutionFailureKind,
    PageExecutionRequest, PreparedCell,
};

/// Local, unvalidated execution evidence, never a publishable page result.
pub(super) struct ExecutedPage {
    pub cells: Vec<ExecutedCell>,
    pub diagnostics: Vec<Diagnostic>,
    pub kernel: Option<SelectedKernel>,
    pub runtime: Option<KernelRuntime>,
}

#[derive(Debug)]
pub(super) struct ExecutedCell {
    pub ordinal: usize,
    pub outcome: CellOutcome,
    pub events: Vec<CellEvent>,
}

#[derive(Debug, Default)]
pub(super) struct ExecutedCells {
    pub cells: Vec<ExecutedCell>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(super) async fn execute_page(
    context: ExecutionContext<'_>,
    request: &PageExecutionRequest,
) -> Result<ExecutedPage, ExecutionFailure> {
    let source = page_source(request);
    validate_request(request, &source)?;
    if request
        .cells
        .iter()
        .all(|cell| !cell.options.execution.eval.value)
    {
        return Ok(unexecuted(request, None));
    }
    let environment = SearchEnvironment::capture(&source)?;
    execute_page_with_environment(context, request, &environment).await
}

pub(super) async fn execute_page_with_environment(
    mut context: ExecutionContext<'_>,
    request: &PageExecutionRequest,
    environment: &SearchEnvironment,
) -> Result<ExecutedPage, ExecutionFailure> {
    let source = page_source(request);
    validate_request(request, &source)?;
    if request
        .cells
        .iter()
        .all(|cell| !cell.options.execution.eval.value)
    {
        return Ok(unexecuted(request, None));
    }
    let kernel = tokio::select! {
        biased;
        _ = &mut context.cancellation => {
            return Err(source.failure(ExecutionFailureKind::Cancelled, "Page execution was canceled."));
        }
        result = discover_kernel(&request.kernel, environment, &source) => result?,
    };
    if request
        .cells
        .iter()
        .all(|cell| skip_reason(cell, &kernel.language).is_some())
    {
        return Ok(unexecuted(request, Some(kernel)));
    }
    let session = start_session(kernel, &mut context, source).await?;
    let runtime = session.runtime.clone();
    let kernel = session.kernel.clone();
    let result = session
        .execute(request.cells.clone(), &mut context.cancellation)
        .await?;
    Ok(ExecutedPage {
        cells: result.cells,
        diagnostics: kernel
            .diagnostics
            .iter()
            .cloned()
            .chain(result.diagnostics)
            .collect(),
        kernel: Some(kernel),
        runtime: Some(runtime),
    })
}

fn page_source(request: &PageExecutionRequest) -> FailureSource {
    FailureSource {
        collection: request.page.collection.clone(),
        source: request.page.source.clone(),
    }
}

fn validate_request(
    request: &PageExecutionRequest,
    source: &FailureSource,
) -> Result<(), ExecutionFailure> {
    if request.page.format != AuthoredFormat::Qmd
        || request.page.mode != ExecutionMode::Execute
        || request.page.page_veto
    {
        return Err(source.failure(
            ExecutionFailureKind::Startup,
            "The page has no execution authority.",
        ));
    }
    if request
        .cells
        .iter()
        .enumerate()
        .any(|(ordinal, cell)| cell.ordinal != ordinal || !cell.cell.outputs.is_empty())
        || request
            .cells
            .windows(2)
            .any(|cells| cells[0].cell.span.end > cells[1].cell.span.start)
    {
        return Err(source.failure(
            ExecutionFailureKind::Startup,
            "Prepared cells must be unexecuted and in authored order.",
        ));
    }
    Ok(())
}

pub(super) fn skip_reason(cell: &PreparedCell, language: &str) -> Option<CellSkipReason> {
    if cell
        .cell
        .language
        .as_deref()
        .map(normalize_language)
        .as_deref()
        != Some(language)
    {
        Some(CellSkipReason::LanguageMismatch)
    } else if !cell.options.execution.eval.value {
        Some(CellSkipReason::EvalFalse)
    } else {
        None
    }
}

fn unexecuted(request: &PageExecutionRequest, kernel: Option<SelectedKernel>) -> ExecutedPage {
    let cells = request
        .cells
        .iter()
        .map(|cell| ExecutedCell {
            ordinal: cell.ordinal,
            outcome: CellOutcome::Skipped {
                reason: kernel
                    .as_ref()
                    .and_then(|kernel| skip_reason(cell, &kernel.language))
                    .unwrap_or(CellSkipReason::EvalFalse),
            },
            events: Vec::new(),
        })
        .collect();
    ExecutedPage {
        cells,
        diagnostics: kernel
            .as_ref()
            .map(|kernel| kernel.diagnostics.clone())
            .unwrap_or_default(),
        kernel,
        runtime: None,
    }
}
