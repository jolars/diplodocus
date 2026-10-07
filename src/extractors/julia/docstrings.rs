//! Inert Julia Markdown with exact attribution through Fatou's string maps.

use fatou_parser::ast::{AstNode, StaticDocText};
use fatou_parser::documentation::syntax::SyntaxNode;
use fatou_parser::documentation::{self, ast as md};
use rowan::TextRange;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::*;

use super::{declaration_provenance, diagnostic, span};

/// Structured documentation and diagnostics mapped into the original .jl file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JuliaDocstringParse {
    /// Inert document with decoded-text-relative spans.
    pub document: SourcedDocument,
    /// Diagnostics with exact original-source locations.
    pub diagnostics: Vec<Diagnostic>,
}

/// Convert a statically decoded Julia docstring without evaluating any content.
///
/// The caller supplies the original literal's location. Fatou's source map
/// addresses absolute source-file bytes; document-node spans address decoded
/// text, just as the other extracted documentation formats do.
pub fn parse(text: &StaticDocText, source: SourceLocation) -> JuliaDocstringParse {
    let parsed = documentation::parse(text.as_str());
    let mut context = Context {
        text,
        source: source.clone(),
        diagnostics: Vec::new(),
        provenance: Vec::new(),
    };
    for native in parsed.diagnostics {
        context.warning(
            native.range,
            format!("Malformed Julia Markdown: {:?}.", native.kind),
        );
    }
    let root = md::Document::cast(parsed.cst).unwrap();
    let blocks = root.blocks().map(|block| context.block(block)).collect();
    let document = SourcedDocument {
        document: Document {
            span: SourceSpan {
                start: 0,
                end: text.as_str().len(),
            },
            frontmatter: None,
            blocks,
        },
        source_format: DocumentFormat::Extracted {
            name: "julia-markdown".into(),
        },
        source_location: Some(source),
        raw_source: Some(text.as_str().into()),
        provenance: context.provenance,
    };
    JuliaDocstringParse {
        document,
        diagnostics: context.diagnostics,
    }
}

pub(super) fn opaque(raw: String, source: SourceLocation) -> SourcedDocument {
    let range = SourceSpan {
        start: 0,
        end: raw.len(),
    };
    SourcedDocument {
        document: Document {
            span: range,
            frontmatter: None,
            blocks: vec![Block::Unsupported {
                source_kind: "opaque-julia-docstring".into(),
                raw: raw.clone(),
                span: range,
            }],
        },
        source_format: DocumentFormat::Extracted {
            name: "julia-markdown".into(),
        },
        source_location: Some(source.clone()),
        raw_source: Some(raw),
        provenance: vec![declaration_provenance(&source)],
    }
}

struct Context<'a> {
    text: &'a StaticDocText,
    source: SourceLocation,
    diagnostics: Vec<Diagnostic>,
    provenance: Vec<Provenance>,
}

impl Context<'_> {
    fn location(&self, range: TextRange) -> SourceLocation {
        SourceLocation {
            span: self
                .text
                .source_map()
                .source_range(range)
                .map(span)
                .or(self.source.span),
            ..self.source.clone()
        }
    }

    fn record(&mut self, range: TextRange) {
        let mut provenance = declaration_provenance(&self.location(range));
        provenance.activity = ProvenanceActivity::DecodedDocumentation {
            decoded_span: span(range),
        };
        self.provenance.push(provenance);
    }

    fn warning(&mut self, range: TextRange, message: impl Into<String>) {
        self.diagnostics.push(diagnostic(
            DiagnosticCode::JuliaUnsupportedMarkdown,
            Severity::Warning,
            message,
            &self.location(range),
        ));
    }

    fn unsupported(&mut self, node: &SyntaxNode) -> Block {
        self.warning(
            node.text_range(),
            format!(
                "Julia Markdown {:?} is retained without interpretation.",
                node.kind()
            ),
        );
        Block::Unsupported {
            source_kind: format!("julia-{:?}", node.kind()),
            raw: node.text().to_string(),
            span: span(node.text_range()),
        }
    }

    fn block(&mut self, block: md::Block) -> Block {
        let range = block.syntax().text_range();
        self.record(range);
        let span = span(range);
        match block {
            md::Block::Paragraph(node) => Block::Paragraph {
                inlines: self.inlines(node.inlines()),
                span,
            },
            md::Block::Heading(node) => Block::Heading {
                level: (usize::from(node.level()) + 1).min(6),
                attributes: Attributes {
                    identifier: Some(SpannedString {
                        value: node.slug(),
                        span,
                    }),
                    ..Attributes::default()
                },
                inlines: self.inlines(node.inlines()),
                span,
            },
            md::Block::BlockQuote(node) => Block::BlockQuote {
                blocks: node.blocks().map(|block| self.block(block)).collect(),
                span,
            },
            md::Block::List(node) => {
                if node.ordered_start().is_some_and(|start| start != 1) {
                    return self.unsupported(node.syntax());
                }
                Block::List {
                    ordered: node.ordered_start().is_some(),
                    items: node
                        .items()
                        .map(|item| {
                            let item_span = super::span(item.syntax().text_range());
                            let mut blocks: Vec<_> =
                                item.blocks().map(|block| self.block(block)).collect();
                            let inlines = self.inlines(item.inlines());
                            if !inlines.is_empty() {
                                blocks.insert(
                                    0,
                                    Block::Paragraph {
                                        inlines,
                                        span: item_span,
                                    },
                                );
                            }
                            ListItem {
                                checked: None,
                                blocks,
                                span: item_span,
                            }
                        })
                        .collect(),
                    span,
                }
            }
            md::Block::Admonition(node) => {
                let kind = match node.category().as_str() {
                    "note" | "info" => CalloutKind::Note,
                    "tip" | "hint" => CalloutKind::Tip,
                    "important" => CalloutKind::Important,
                    "warning" => CalloutKind::Warning,
                    "caution" | "danger" => CalloutKind::Caution,
                    _ => return self.unsupported(node.syntax()),
                };
                let mut blocks: Vec<_> = node.blocks().map(|block| self.block(block)).collect();
                blocks.insert(
                    0,
                    Block::Paragraph {
                        inlines: vec![Inline::Strong {
                            inlines: vec![Inline::Text {
                                value: node.title(),
                                span,
                            }],
                            span,
                        }],
                        span,
                    },
                );
                Block::Callout {
                    kind,
                    attributes: Attributes::default(),
                    blocks,
                    span,
                }
            }
            md::Block::CodeBlock(node) => {
                let language = match node.fence_kind() {
                    md::FenceKind::Plain => None,
                    md::FenceKind::Julia
                    | md::FenceKind::JuliaRepl
                    | md::FenceKind::JlDoctest { .. } => Some("julia".into()),
                    md::FenceKind::Documenter {
                        directive: md::DocumenterDirective::Example | md::DocumenterDirective::Repl,
                        ..
                    } => Some("julia".into()),
                    md::FenceKind::Other(language) => Some(language),
                    _ => return self.unsupported(node.syntax()),
                };
                let content = node.content();
                let segments = node
                    .content_range()
                    .map(|range| {
                        vec![SourceSegment {
                            span: SourceSpan {
                                start: u32::from(range.start()) as usize,
                                end: u32::from(range.start()) as usize + content.len(),
                            },
                            text: content.clone(),
                        }]
                    })
                    .unwrap_or_default();
                Block::CodeBlock {
                    language,
                    source: content,
                    source_segments: segments,
                    span,
                }
            }
            md::Block::IndentedCodeBlock(node) => {
                let source = node.content();
                let mut source_segments = Vec::new();
                for token in node
                    .syntax()
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                {
                    if matches!(
                        token.kind(),
                        documentation::syntax::SyntaxKind::CODE_CONTENT
                            | documentation::syntax::SyntaxKind::NEWLINE
                    ) {
                        source_segments.push(SourceSegment {
                            text: token.text().into(),
                            span: super::span(token.text_range()),
                        });
                    }
                }
                // Julia chomps final newlines; retain only the contributing bytes.
                let mut remaining = source.len();
                for segment in &mut source_segments {
                    let count = remaining.min(segment.text.len());
                    segment.text.truncate(count);
                    segment.span.end = segment.span.start + count;
                    remaining -= count;
                }
                source_segments.retain(|segment| !segment.text.is_empty());
                Block::CodeBlock {
                    language: Some("julia".into()),
                    source,
                    source_segments,
                    span,
                }
            }
            md::Block::Table(node) => {
                let rows: Vec<_> = node.rows().collect();
                let alignments = rows
                    .get(1)
                    .map(|row| {
                        row.cells()
                            .map(|cell| {
                                let text = cell.syntax().text().to_string();
                                let text = text.trim();
                                match (text.starts_with(':'), text.ends_with(':')) {
                                    (true, true) => TableAlignment::Center,
                                    (false, true) => TableAlignment::Right,
                                    (true, false) => TableAlignment::Left,
                                    _ => TableAlignment::Default,
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Block::Table {
                    caption: Vec::new(),
                    alignments,
                    rows: rows
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| *index != 1)
                        .map(|(index, row)| TableRow {
                            header: index == 0,
                            span: super::span(row.syntax().text_range()),
                            cells: row
                                .cells()
                                .map(|cell| {
                                    let span = super::span(cell.syntax().text_range());
                                    TableCell {
                                        span,
                                        blocks: vec![Block::Paragraph {
                                            inlines: self.inlines(cell.inlines()),
                                            span,
                                        }],
                                    }
                                })
                                .collect(),
                        })
                        .collect(),
                    span,
                }
            }
            md::Block::ThematicBreak(_) => Block::ThematicBreak { span },
            other => self.unsupported(other.syntax()),
        }
    }

    fn inlines(&mut self, inlines: impl Iterator<Item = md::Inline>) -> Vec<Inline> {
        inlines.map(|inline| self.inline(inline)).collect()
    }

    fn inline(&mut self, inline: md::Inline) -> Inline {
        self.record(inline_range(&inline));
        match inline {
            md::Inline::Text(token)
            | md::Inline::Escape(token)
            | md::Inline::EnDash(token)
            | md::Inline::EmDash(token) => {
                let span = super::span(token.text_range());
                let value = match token.kind() {
                    documentation::syntax::SyntaxKind::ESCAPE => token
                        .text()
                        .strip_prefix('\\')
                        .unwrap_or(token.text())
                        .into(),
                    documentation::syntax::SyntaxKind::EN_DASH => "–".into(),
                    documentation::syntax::SyntaxKind::EM_DASH => "—".into(),
                    _ => token.text().into(),
                };
                Inline::Text { value, span }
            }
            md::Inline::SoftBreak(token) => Inline::SoftBreak {
                span: super::span(token.text_range()),
            },
            md::Inline::HardBreak(node) => Inline::HardBreak {
                span: super::span(node.syntax().text_range()),
            },
            md::Inline::Emphasis(node) => Inline::Emphasis {
                inlines: self.inlines(inline_children(node.syntax()).into_iter()),
                span: super::span(node.syntax().text_range()),
            },
            md::Inline::Strong(node) => Inline::Strong {
                inlines: self.inlines(inline_children(node.syntax()).into_iter()),
                span: super::span(node.syntax().text_range()),
            },
            md::Inline::Code(node) => Inline::Code {
                value: node.content(),
                span: super::span(node.syntax().text_range()),
            },
            md::Inline::Link(node) => {
                let span = super::span(node.syntax().text_range());
                if let Some(documenter) = node.documenter_link() {
                    if documenter.kind() == md::DocumenterLinkKind::Ref {
                        let inferred = node.label().find_map(|inline| {
                            if let md::Inline::Code(code) = inline {
                                Some((code.content(), code.syntax().text_range()))
                            } else {
                                None
                            }
                        });
                        let target = documenter
                            .target()
                            .map(|target| (target.to_owned(), documenter.target_range().unwrap()))
                            .or(inferred);
                        if let Some((target, range)) = target {
                            self.record(range);
                            return Inline::Link {
                                target: format!("@ref {target}"),
                                inlines: self.inlines(node.label()),
                                title: None,
                                attributes: Attributes::default(),
                                span,
                            };
                        }
                    }
                    return self.unsupported_inline(node.syntax());
                }
                Inline::Link {
                    inlines: self.inlines(node.label()),
                    target: node.destination(),
                    title: None,
                    attributes: Attributes::default(),
                    span,
                }
            }
            md::Inline::Image(node) => {
                let span = super::span(node.syntax().text_range());
                Inline::Image {
                    alt: vec![Inline::Text {
                        value: node.alt(),
                        span,
                    }],
                    target: node.destination(),
                    title: None,
                    attributes: Attributes::default(),
                    span,
                }
            }
            md::Inline::Autolink(node) => Inline::AutoLink {
                target: node.destination(),
                span: super::span(node.syntax().text_range()),
            },
            md::Inline::Math(node) => self.unsupported_inline(node.syntax()),
            md::Inline::FootnoteReference(node) => self.unsupported_inline(node.syntax()),
            md::Inline::Interpolation(node) => self.unsupported_inline(node.syntax()),
        }
    }

    fn unsupported_inline(&mut self, node: &SyntaxNode) -> Inline {
        self.warning(
            node.text_range(),
            format!(
                "Julia Markdown {:?} is retained without interpretation.",
                node.kind()
            ),
        );
        Inline::Unsupported {
            source_kind: format!("julia-{:?}", node.kind()),
            raw: node.text().to_string(),
            span: super::span(node.text_range()),
        }
    }
}

// Fatou 0.8.1 has typed emphasis wrappers but no child accessors for them.
// Keep this navigation bridge local; Julia Markdown parsing remains in Fatou.
fn inline_children(node: &SyntaxNode) -> Vec<md::Inline> {
    use documentation::syntax::SyntaxKind as K;
    node.children_with_tokens()
        .filter_map(|element| match element {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                K::TEXT => Some(md::Inline::Text(token)),
                K::ESCAPE => Some(md::Inline::Escape(token)),
                K::EN_DASH => Some(md::Inline::EnDash(token)),
                K::EM_DASH => Some(md::Inline::EmDash(token)),
                K::SOFT_BREAK => Some(md::Inline::SoftBreak(token)),
                _ => None,
            },
            rowan::NodeOrToken::Node(node) => match node.kind() {
                K::EMPHASIS => md::Emphasis::cast(node).map(md::Inline::Emphasis),
                K::STRONG => md::Strong::cast(node).map(md::Inline::Strong),
                K::INLINE_CODE => md::InlineCode::cast(node).map(md::Inline::Code),
                K::INLINE_MATH => md::InlineMath::cast(node).map(md::Inline::Math),
                K::LINK => md::Link::cast(node).map(md::Inline::Link),
                K::IMAGE => md::Image::cast(node).map(md::Inline::Image),
                K::AUTOLINK => md::Autolink::cast(node).map(md::Inline::Autolink),
                K::FOOTNOTE_REFERENCE => {
                    md::FootnoteReference::cast(node).map(md::Inline::FootnoteReference)
                }
                K::INTERPOLATION => md::Interpolation::cast(node).map(md::Inline::Interpolation),
                K::HARD_BREAK => md::HardBreak::cast(node).map(md::Inline::HardBreak),
                _ => None,
            },
        })
        .collect()
}

fn inline_range(inline: &md::Inline) -> TextRange {
    match inline {
        md::Inline::Text(token)
        | md::Inline::Escape(token)
        | md::Inline::EnDash(token)
        | md::Inline::EmDash(token)
        | md::Inline::SoftBreak(token) => token.text_range(),
        md::Inline::Emphasis(node) => node.syntax().text_range(),
        md::Inline::Strong(node) => node.syntax().text_range(),
        md::Inline::Code(node) => node.syntax().text_range(),
        md::Inline::Math(node) => node.syntax().text_range(),
        md::Inline::Link(node) => node.syntax().text_range(),
        md::Inline::Image(node) => node.syntax().text_range(),
        md::Inline::Autolink(node) => node.syntax().text_range(),
        md::Inline::FootnoteReference(node) => node.syntax().text_range(),
        md::Inline::Interpolation(node) => node.syntax().text_range(),
        md::Inline::HardBreak(node) => node.syntax().text_range(),
    }
}
