//! Generated Markdown is local output content, without authored-page authority.

use std::collections::{BTreeMap, HashMap};

use panache_parser::ParserOptions;
use panache_parser::syntax::{AstNode, CodeBlock, Document, TextRange};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticSource};
use crate::ir::{
    Block, CellOutput, CellOutputKind, OutputRepresentation, Provenance, ProvenanceActivity,
    SourceLocation, SourceSegment,
};

use super::{AuthoredFormat, parse_document, source_segments, span};

const PANACHE_VERSION: &str = "0.29.0";

/// Portable attribution supplied by the producer of one Markdown output.
///
/// Ordinals refer to authored source and stable output order, never Jupyter
/// execution counts or transient protocol identifiers. The producer determines
/// this context; generated Markdown cannot change it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownFragmentOrigin {
    /// Owning authored content collection ID.
    pub collection: String,
    /// Zero-based producing cell ordinal in authored source order.
    pub cell: usize,
    /// Zero-based output-slot ordinal within the producing cell.
    pub output: usize,
    /// Producing page and, when known, its authored cell's fence range.
    pub source: SourceLocation,
}

/// An inert Markdown representation together with its attribution and diagnostics.
///
/// Block and diagnostic ranges are relative to the generated fragment's UTF-8
/// bytes. Diagnostic sources remain absent because the fragment is not a file.
/// [`Self::provenance`] identifies the producing page, cell, and output slot;
/// its own source range refers to the authored cell, not the fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownFragmentParse {
    /// Structured output, always [`OutputRepresentation::MarkdownBlocks`].
    pub representation: OutputRepresentation,
    /// Adapter evidence and the original producing cell's portable identity.
    pub provenance: Provenance,
    /// Deterministic fragment-relative parser and unsupported-syntax diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

impl MarkdownFragmentParse {
    /// Attach the fragment to a typed output without losing its attribution.
    ///
    /// Use a display event for Markdown MIME output, or an explicitly selected
    /// `output: asis` stdout stream. Ordinary streams must stay on the
    /// [`OutputRepresentation::PlainText`] path. The returned diagnostics remain
    /// relative to this output and must be kept with its provenance.
    pub fn into_cell_output(self, kind: CellOutputKind) -> (CellOutput, Vec<Diagnostic>) {
        (
            CellOutput {
                kind,
                representations: vec![self.representation],
                provenance: vec![self.provenance],
            },
            self.diagnostics,
        )
    }
}

/// Parse explicitly selected Markdown output as an isolated, inert GFM fragment.
///
/// Uses the same in-process Panache adapter as authored documents, with
/// executable syntax and automatic target creation disabled at the parser and
/// adapter boundaries. Frontmatter is visibly unsupported, hashpipe lines stay
/// display code, and nested containers cannot introduce executable cells.
/// Headings and labels are output content only: consumers must not register them
/// as page anchors, navigation entries, or semantic identities. Semantic
/// references and ordinary links retain their targets for resolution in the
/// owning page's context; this function does not resolve or validate links.
///
/// Raw HTML remains unsupported source for escaped display and never becomes an
/// HTML candidate. Parsing grants no rendering trust. MIME selection, URL and
/// asset validation, sanitization, execution, and rendering are separate stages.
/// This function performs no filesystem access and never reparses the page.
pub fn parse_markdown_fragment(
    source: &str,
    origin: MarkdownFragmentOrigin,
) -> MarkdownFragmentParse {
    let parsed = parse_document(source, AuthoredFormat::Gfm, true);
    MarkdownFragmentParse {
        representation: OutputRepresentation::MarkdownBlocks {
            media_type: "text/markdown".into(),
            blocks: parsed.document.blocks,
        },
        provenance: Provenance {
            activity: ProvenanceActivity::GeneratedMarkdown {
                collection: origin.collection,
                cell: origin.cell,
                output: origin.output,
            },
            source: Some(DiagnosticSource::Repository {
                repository: origin.source.repository,
                path: origin.source.path,
            }),
            span: origin.source.span,
            tools: BTreeMap::from([
                ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
                ("panache-parser".into(), PANACHE_VERSION.into()),
            ]),
        },
        diagnostics: parsed.diagnostics,
    }
}

// Panache recognizes hashpipe preambles even with executable syntax disabled,
// and its code-source accessor excludes those bytes. A second, isolated GFM
// read with only the affected fence info blanked gives us its exact display
// segments, including hashpipes and correct container framing. Equal byte
// lengths preserve all body offsets, including CRLF and unterminated fences.
// The original tree still owns language, structure, and diagnostics.
pub(super) fn display_sources(
    source: &str,
    document: &Document,
    options: ParserOptions,
) -> HashMap<TextRange, Vec<SourceSegment>> {
    let mut display_source = source.to_owned();
    let mut changed = false;
    for code in document.syntax().descendants().filter_map(CodeBlock::cast) {
        if code.hashpipe_yaml_preamble().is_some()
            && let Some(info) = code.info()
        {
            let range = span(info.syntax().text_range());
            display_source
                .replace_range(range.start..range.end, &" ".repeat(range.end - range.start));
            changed = true;
        }
    }
    if !changed {
        return HashMap::new();
    }
    let parsed = panache_parser::parse_document(&display_source, Some(options));
    parsed
        .document()
        .syntax()
        .descendants()
        .filter_map(CodeBlock::cast)
        .map(|code| {
            (
                code.syntax().text_range(),
                source_segments(code.code_source_segments()),
            )
        })
        .collect()
}

pub(super) fn display_code(
    code: &CodeBlock,
    display_sources: &mut HashMap<TextRange, Vec<SourceSegment>>,
) -> Option<Block> {
    let source_segments = if code.hashpipe_yaml_preamble().is_some() {
        display_sources.remove(&code.syntax().text_range())?
    } else {
        source_segments(code.code_source_segments())
    };
    Some(Block::CodeBlock {
        language: code.language(),
        source: source_segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect(),
        source_segments,
        span: span(code.syntax().text_range()),
    })
}
