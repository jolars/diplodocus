//! QMD policy validation and preparation without execution or local paths.

use std::collections::BTreeMap;

use panache_parser::syntax::{
    AstNode, ChunkInfoItem, CodeBlock, SyntaxKind, SyntaxNode, YamlBlockMapKey, YamlFlowMapKey,
};

use crate::configuration::{ContentConfiguration, ExecutionConfigurationError, ExecutionMode};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::execution::{EffectiveOption, ExecutionDefaults, OptionOrigin, PreparedCell};
use crate::ir::{Attributes, Block, CodeCell, Inline, SourceSpan};
use crate::validation::validate_document_execution;

use super::{AuthoredFormat, DocumentParse, ParseMode, parse_document, span};

mod options;
mod scalars;

/// A validated authored document and, when valid QMD, its preparation data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDocument {
    /// Original document with source-ordered parser and policy diagnostics.
    pub parsed: DocumentParse,
    /// Absent for GFM or any document with an error diagnostic.
    pub preparation: Option<QmdPreparation>,
}

/// Pure preparation under `qmd-mvp-v1`, before kernel language selection.
///
/// Valid disabled pages retain their cells and options. This record is not an
/// execution request or proof of authorization; callers still gate dispatch on
/// the command being run. In particular, checks must never dispatch execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QmdPreparation {
    /// Effective inheritable options with winning declaration origins.
    pub defaults: ExecutionDefaults,
    /// Top-level `execute: false`, which no cell can override.
    pub page_veto: bool,
    /// Every authored cell in source order, including disabled cells.
    pub cells: Vec<PreparedCell>,
    /// Collection authority, no veto, and at least one `eval: true` cell.
    ///
    /// This is preliminary eligibility, not a request to discover a kernel.
    /// The executor must still match the explicitly selected kernel's language.
    pub execution_eligible: bool,
}

/// Parse, validate, and prepare a collection's document without performing I/O.
///
/// The original document is retained unchanged. QMD validation checks every
/// declaration, including overridden options and disabled cells. Errors remain
/// in `parsed.diagnostics` and prevent preparation; warnings do not. Effective
/// options use hashpipe, inline, document, then policy-default precedence.
/// No kernel is discovered, no environment input is read, and no artifact is
/// created. Rendering and execution consume the prepared options separately.
///
/// # Errors
///
/// Returns an error when the collection's execution configuration is invalid.
pub fn prepare_collection_document(
    source: &str,
    collection: &ContentConfiguration,
) -> Result<PreparedDocument, ExecutionConfigurationError> {
    collection.validate_execution()?;
    let mut parsed = parse_document(source, collection.format, ParseMode::Collection);
    if collection.format == AuthoredFormat::Gfm {
        return Ok(PreparedDocument {
            parsed,
            preparation: None,
        });
    }
    let authority = validate_document_execution(&parsed.document, collection)?;
    // Selectors have one authoritative diagnostic, even if their values also
    // contain YAML syntax outside the supported subset.
    parsed.diagnostics.retain(|diagnostic| {
        diagnostic.code != DiagnosticCode::InvalidQmdMetadata
            || !authority.iter().any(|authority| {
                std::iter::once(authority.span)
                    .flatten()
                    .chain(authority.related_spans.iter().copied())
                    .any(|range| diagnostic.span.is_some_and(|s| contains(range, s)))
            })
    });
    parsed.diagnostics.extend(authority);
    // Rebuild ambiguity diagnostics for every tier, including overridden ones.
    parsed
        .diagnostics
        .retain(|d| d.code != DiagnosticCode::AmbiguousCellOption);
    let mut context = PreparationContext {
        source,
        diagnostics: &mut parsed.diagnostics,
        cells: Vec::new(),
        anchors: BTreeMap::new(),
    };
    let (defaults, page_veto) = options::document_defaults(
        source,
        parsed.document.frontmatter.as_ref(),
        context.diagnostics,
    );
    context.blocks(&parsed.document.blocks, &defaults);
    context.check_anchors();
    let cells = context.cells;
    let execution_eligible = collection.execution.mode == ExecutionMode::Execute
        && !page_veto
        && cells.iter().any(|cell| cell.options.execution.eval.value);
    parsed.diagnostics.sort();
    let preparation = (!parsed
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error))
    .then_some(QmdPreparation {
        defaults,
        page_veto,
        cells,
        execution_eligible,
    });
    Ok(PreparedDocument {
        parsed,
        preparation,
    })
}

struct PreparationContext<'a> {
    source: &'a str,
    diagnostics: &'a mut Vec<Diagnostic>,
    cells: Vec<PreparedCell>,
    anchors: BTreeMap<String, Vec<Anchor>>,
}

struct Anchor {
    span: SourceSpan,
    cell: bool,
}

impl PreparationContext<'_> {
    fn blocks(&mut self, blocks: &[Block], defaults: &ExecutionDefaults) {
        for block in blocks {
            match block {
                Block::CodeCell(cell) => self.cell(cell, defaults),
                Block::BlockQuote { blocks, .. } => self.blocks(blocks, defaults),
                Block::Callout {
                    attributes, blocks, ..
                } => {
                    self.attributes(attributes);
                    self.blocks(blocks, defaults);
                }
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&item.blocks, defaults);
                    }
                }
                Block::Table { caption, rows, .. } => {
                    self.inlines(caption);
                    for row in rows {
                        for cell in &row.cells {
                            self.blocks(&cell.blocks, defaults);
                        }
                    }
                }
                Block::Heading {
                    attributes,
                    inlines,
                    ..
                } => {
                    self.attributes(attributes);
                    self.inlines(inlines);
                }
                Block::Paragraph { inlines, .. } => self.inlines(inlines),
                Block::ThematicBreak { .. }
                | Block::CodeBlock { .. }
                | Block::Unsupported { .. } => {}
            }
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Link {
                    attributes,
                    inlines,
                    ..
                } => {
                    self.attributes(attributes);
                    self.inlines(inlines);
                }
                Inline::Image {
                    attributes, alt, ..
                } => {
                    self.attributes(attributes);
                    self.inlines(alt);
                }
                Inline::Emphasis { inlines, .. }
                | Inline::Strong { inlines, .. }
                | Inline::Strikeout { inlines, .. } => self.inlines(inlines),
                _ => {}
            }
        }
    }

    fn attributes(&mut self, attributes: &Attributes) {
        if let Some(id) = &attributes.identifier {
            self.anchors
                .entry(id.value.clone())
                .or_default()
                .push(Anchor {
                    span: id.span,
                    cell: false,
                });
        }
    }

    fn cell(&mut self, cell: &CodeCell, defaults: &ExecutionDefaults) {
        let mut options = options::cell_options(self.source, cell, defaults, self.diagnostics);
        for class in &cell.classes {
            self.diagnostics.push(error(
                DiagnosticCode::UnsupportedCellOption,
                "Cell fence classes are unsupported.",
                class.span,
            ));
        }
        if let Some(identifier) = &cell.identifier {
            if !options::valid_label(&identifier.value) {
                self.diagnostics.push(error(
                    DiagnosticCode::InvalidCellOption,
                    "Cell identifiers must match [A-Za-z][A-Za-z0-9_.:-]*.",
                    identifier.span,
                ));
            }
            if let Some(label) = &options.label.value {
                if label != &identifier.value {
                    let mut diagnostic = error(
                        DiagnosticCode::InvalidCellOption,
                        "The cell label and fence identifier must agree.",
                        origin_span(options.label.origin).unwrap(),
                    );
                    diagnostic.related_spans.push(identifier.span);
                    self.diagnostics.push(diagnostic);
                }
            } else {
                options.label = EffectiveOption {
                    value: Some(identifier.value.clone()),
                    origin: OptionOrigin::FenceIdentifier {
                        span: identifier.span,
                    },
                };
            }
        }
        if let Some(label) = &options.label.value {
            self.anchors.entry(label.clone()).or_default().push(Anchor {
                span: origin_span(options.label.origin).unwrap(),
                cell: true,
            });
        }
        self.cells.push(PreparedCell {
            ordinal: self.cells.len(),
            cell: cell.clone(),
            options,
        });
    }

    fn check_anchors(&mut self) {
        for anchors in self.anchors.values_mut() {
            if anchors.len() < 2 || !anchors.iter().any(|a| a.cell) {
                continue;
            }
            anchors.sort_by_key(|a| (a.span.start, a.span.end));
            let mut diagnostic = error(
                DiagnosticCode::InvalidCellOption,
                "A cell label conflicts with another authored anchor on this page.",
                anchors[0].span,
            );
            diagnostic
                .related_spans
                .extend(anchors[1..].iter().map(|a| a.span));
            self.diagnostics.push(diagnostic);
        }
    }
}

fn origin_span(origin: OptionOrigin) -> Option<SourceSpan> {
    match origin {
        OptionOrigin::Default => None,
        OptionOrigin::Document { span }
        | OptionOrigin::Inline { span }
        | OptionOrigin::Hashpipe { span }
        | OptionOrigin::FenceIdentifier { span } => Some(span),
    }
}

fn error(code: DiagnosticCode, message: impl Into<String>, span: SourceSpan) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Error,
        message: message.into(),
        span: Some(span),
        related_spans: Vec::new(),
        related_entity: None,
        source: None,
    }
}

fn contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Inspect syntax that Panache's semantic YAML projection deliberately omits.
pub(super) fn syntax_diagnostics(root: &SyntaxNode) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for code in root.descendants().filter_map(CodeBlock::cast) {
        if code.executable_cell().is_some()
            && let Some(info) = code.info()
        {
            let identifiers = info
                .chunk_items()
                .filter_map(|item| match item {
                    ChunkInfoItem::Id(id) => Some(span(id.range())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if identifiers.len() > 1 {
                let mut diagnostic = error(
                    DiagnosticCode::InvalidCellOption,
                    "A cell fence may declare only one identifier.",
                    identifiers[0],
                );
                diagnostic
                    .related_spans
                    .extend_from_slice(&identifiers[1..]);
                diagnostics.push(diagnostic);
            }
        }
    }
    for element in root.descendants_with_tokens() {
        if matches!(
            element.kind(),
            SyntaxKind::YAML_TAG | SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS
        ) {
            let parent = element.parent().expect("YAML token parent");
            let code = yaml_diagnostic_code(&parent);
            diagnostics.push(error(
                code,
                "YAML tags, anchors, and aliases are unsupported.",
                span(element.text_range()),
            ));
        }
        if let Some(node) = element.as_node() {
            let invalid_key = match node.kind() {
                SyntaxKind::YAML_BLOCK_MAP_KEY => YamlBlockMapKey::cast(node.clone())
                    .unwrap()
                    .scalar()
                    .is_none(),
                SyntaxKind::YAML_FLOW_MAP_KEY => YamlFlowMapKey::cast(node.clone())
                    .unwrap()
                    .scalar()
                    .is_none(),
                _ => false,
            };
            if invalid_key {
                diagnostics.push(error(
                    yaml_diagnostic_code(node),
                    "QMD option and metadata keys must be scalars.",
                    span(node.text_range()),
                ));
            }
            if node.kind() == SyntaxKind::CHUNK_LABEL {
                diagnostics.push(error(
                    DiagnosticCode::UnsupportedCellOption,
                    "Bare cell labels are unsupported; use a fence identifier or the label option.",
                    span(node.text_range()),
                ));
            }
        }
    }
    diagnostics
}

fn yaml_diagnostic_code(node: &SyntaxNode) -> DiagnosticCode {
    if node
        .ancestors()
        .any(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
    {
        DiagnosticCode::InvalidCellOption
    } else {
        DiagnosticCode::InvalidQmdMetadata
    }
}
