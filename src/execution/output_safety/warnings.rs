//! Capture parser warnings before any later candidate or asset failure.
use super::*;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{Block, Inline, SourceSpan};

pub(super) fn project(
    blocks: &[Block],
    diagnostics: &[Diagnostic],
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
) -> Result<Vec<ExecutionDiagnostic>, RestoreRejection> {
    let mut nodes = Vec::new();
    visit_blocks(blocks, &mut nodes, 0)?;
    if nodes.len() != diagnostics.len() {
        return Err(RestoreRejection::Structure);
    }
    let length = origin
        .fragment
        .ok_or(RestoreRejection::Structure)?
        .byte_length;
    let valid_span = |span: SourceSpan| span.start <= span.end && span.end <= length;
    let mut warnings = Vec::new();
    for diagnostic in diagnostics {
        if diagnostic.code != DiagnosticCode::UnsupportedAuthoredSyntax
            || diagnostic.severity != Severity::Warning
            || diagnostic.source.is_some()
            || !diagnostic.related_spans.iter().copied().all(valid_span)
        {
            return Err(RestoreRejection::Structure);
        }
        let span = diagnostic
            .span
            .filter(|s| valid_span(*s))
            .ok_or(RestoreRejection::Structure)?;
        let index = nodes
            .iter()
            .position(|(s, _)| *s == span)
            .ok_or(RestoreRejection::Structure)?;
        let (_, source_kind) = nodes.remove(index);
        let mut attribution = DiagnosticAttribution::output(context, origin, Some(span));
        attribution.related_spans = diagnostic.related_spans.clone();
        warnings.push(ExecutionDiagnostic::FragmentUnsupported {
            attribution,
            source_kind: source_kind.into(),
        });
    }
    Ok(warnings)
}
fn visit_blocks<'a>(
    blocks: &'a [Block],
    nodes: &mut Vec<(SourceSpan, &'a str)>,
    depth: usize,
) -> Result<(), RestoreRejection> {
    if depth > 128 {
        return Err(RestoreRejection::Structure);
    }
    for block in blocks {
        match block {
            Block::Unsupported {
                span, source_kind, ..
            } => nodes.push((*span, source_kind)),
            Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                visit_inlines(inlines, nodes, depth + 1)?
            }
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                visit_blocks(blocks, nodes, depth + 1)?
            }
            Block::List { items, .. } => {
                for item in items {
                    visit_blocks(&item.blocks, nodes, depth + 1)?;
                }
            }
            Block::Table { caption, rows, .. } => {
                visit_inlines(caption, nodes, depth + 1)?;
                for row in rows {
                    for cell in &row.cells {
                        visit_blocks(&cell.blocks, nodes, depth + 1)?;
                    }
                }
            }
            Block::ThematicBreak { .. } | Block::CodeBlock { .. } => {}
            Block::CodeCell(_) => return Err(RestoreRejection::Structure),
        }
    }
    Ok(())
}
fn visit_inlines<'a>(
    inlines: &'a [Inline],
    nodes: &mut Vec<(SourceSpan, &'a str)>,
    depth: usize,
) -> Result<(), RestoreRejection> {
    if depth > 128 {
        return Err(RestoreRejection::Structure);
    }
    for inline in inlines {
        match inline {
            Inline::Unsupported {
                span, source_kind, ..
            } => nodes.push((*span, source_kind)),
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. }
            | Inline::Link { inlines, .. } => visit_inlines(inlines, nodes, depth + 1)?,
            Inline::Image { alt, .. } => visit_inlines(alt, nodes, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}
