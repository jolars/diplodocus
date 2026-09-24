//! Rendering of validated site models into static output.

mod html;
mod publish;
mod signatures;
pub use html::render_site;
pub use publish::{RenderedFile, RenderedSite};

use crate::configuration::ExecutionMode;
use crate::execution::{
    CellExecutionResult, EffectiveCellOptions, ExecutionFailure, ExecutionOutput, ExecutionPage,
    OutputVisibility, PreparedCell, validate_figure_options,
};
use crate::ir::SourceSegment;

/// Render literal output as escaped, preformatted HTML without Markdown parsing.
///
/// Use this for `PlainText` representations, stderr, and normalized error text.
/// Keep the IR's original bytes and escape only at this final HTML boundary.
pub fn render_preformatted_text(text: &str) -> String {
    let mut html = String::from("<pre><code>");
    for ch in text.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            _ => html.push(ch),
        }
    }
    html.push_str("</code></pre>");
    html
}

/// A borrowed presentation view that preserves the complete execution evidence.
///
/// This view applies visibility only. Text still requires escaping, and output
/// representations still require the active MIME, HTML, and asset validators.
/// Creating a view never grants rendering trust to decoded execution records.
#[derive(Debug, PartialEq, Eq)]
pub struct CellPresentation<'a> {
    /// Visible code segments; concatenate their text in order and escape it.
    pub source: Option<&'a [SourceSegment]>,
    /// Visible final outputs in slot order, including allowed errors.
    pub outputs: &'a [ExecutionOutput],
}

/// Present a prepared cell when no execution result exists.
///
/// Use the collection's mode, independently of page veto or cell eligibility.
/// In `never` collections, source remains visible even with `echo: false` or
/// `include: false`. In authorized collections, those options also apply to
/// cells skipped by evaluation, language matching, or a page veto. Unexecuted
/// cells have no output and require no runtime figure-count validation.
pub fn present_prepared_cell(mode: ExecutionMode, cell: &PreparedCell) -> CellPresentation<'_> {
    presentation(mode, &cell.cell.source_segments, &cell.options, &[])
}

/// Validate figure counts and apply visibility to a cell's completed result.
///
/// Call only after the entire page has finished and all display updates and
/// clearing have been applied. Hidden source and outputs remain in the original
/// record for validation and provenance. `output: asis` displays the converted
/// output unchanged; parsing stdout fragments belongs to output conversion.
/// This function does not replace output safety or record validation.
///
/// # Errors
///
/// Returns an output-validation failure for mismatched subcaptions, including
/// when `include: false` or `output: false` would hide the figures.
pub fn present_cell<'a>(
    page: &ExecutionPage,
    cell: &'a CellExecutionResult,
) -> Result<CellPresentation<'a>, ExecutionFailure> {
    validate_figure_options(page, std::slice::from_ref(cell))?;
    Ok(presentation(
        page.mode,
        &cell.source_segments,
        &cell.options,
        &cell.outputs,
    ))
}

fn presentation<'a>(
    mode: ExecutionMode,
    source: &'a [SourceSegment],
    options: &EffectiveCellOptions,
    outputs: &'a [ExecutionOutput],
) -> CellPresentation<'a> {
    if mode == ExecutionMode::Never {
        return CellPresentation {
            source: Some(source),
            outputs: &[],
        };
    }
    let execution = &options.execution;
    CellPresentation {
        source: (execution.include.value && execution.echo.value).then_some(source),
        outputs: if execution.include.value && execution.output.value != OutputVisibility::Hide {
            outputs
        } else {
            &[]
        },
    }
}
