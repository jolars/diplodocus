//! Inert Julia Markdown with exact attribution through Fatou's string maps.

use fatou_parser::ast::{AstNode, StaticDocText};
use fatou_parser::documentation::syntax::{SyntaxElement, SyntaxNode};
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
        text: Some(text),
        source: Some(source.clone()),
        authored: false,
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
        generated_source: None,
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
        generated_source: None,
        raw_source: Some(raw),
        provenance: vec![declaration_provenance(&source)],
    }
}

pub(crate) fn parse_documenter(source: &str) -> crate::documents::DocumentParse {
    let parsed = documentation::parse(source);
    let mut context = Context {
        text: None,
        source: None,
        authored: true,
        diagnostics: Vec::new(),
        provenance: Vec::new(),
    };
    for diagnostic in parsed.diagnostics {
        context.warning(
            diagnostic.range,
            format!("Malformed Julia Markdown: {:?}.", diagnostic.kind),
        );
    }
    let root = md::Document::cast(parsed.cst).unwrap();
    let blocks = root.blocks().map(|block| context.block(block)).collect();
    crate::documents::DocumentParse {
        document: Document {
            span: SourceSpan {
                start: 0,
                end: source.len(),
            },
            frontmatter: None,
            blocks,
        },
        diagnostics: context.diagnostics,
    }
}

struct Context<'a> {
    text: Option<&'a StaticDocText>,
    source: Option<SourceLocation>,
    authored: bool,
    diagnostics: Vec<Diagnostic>,
    provenance: Vec<Provenance>,
}

impl Context<'_> {
    fn location(&self, range: TextRange) -> Option<SourceLocation> {
        self.source.as_ref().map(|source| SourceLocation {
            span: self
                .text
                .and_then(|text| text.source_map().source_range(range))
                .map(span)
                .or(source.span),
            ..source.clone()
        })
    }

    fn record(&mut self, range: TextRange) {
        if let Some(location) = self.location(range) {
            let mut provenance = declaration_provenance(&location);
            provenance.activity = ProvenanceActivity::DecodedDocumentation {
                decoded_span: span(range),
            };
            self.provenance.push(provenance);
        }
    }

    fn warning(&mut self, range: TextRange, message: impl Into<String>) {
        let mut diagnostic = if let Some(location) = self.location(range) {
            diagnostic(
                DiagnosticCode::JuliaUnsupportedMarkdown,
                Severity::Warning,
                message,
                &location,
            )
        } else {
            Diagnostic::new(
                DiagnosticCode::UnsupportedAuthoredSyntax,
                Severity::Warning,
                message,
            )
        };
        if self.authored {
            diagnostic.span = Some(span(range));
        }
        self.diagnostics.push(diagnostic);
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
            md::Block::Heading(node) => {
                let anchor = if self.authored && node.inlines().count() == 1 {
                    node.inlines().next().and_then(|inline| {
                        if let md::Inline::Link(link) = inline
                            && let Some(anchor) = link.documenter_link()
                            && anchor.kind() == md::DocumenterLinkKind::Id
                            && let Some(target) = anchor.target()
                        {
                            return Some((target.to_owned(), self.inlines(link.label())));
                        }
                        None
                    })
                } else {
                    None
                };
                let (identifier, inlines) =
                    anchor.unwrap_or_else(|| (node.slug(), self.inlines(node.inlines())));
                Block::Heading {
                    level: (usize::from(node.level()) + usize::from(!self.authored)).min(6),
                    attributes: Attributes {
                        identifier: Some(SpannedString {
                            value: identifier,
                            span,
                        }),
                        ..Attributes::default()
                    },
                    inlines,
                    span,
                }
            }
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
                    items: node.items().map(|item| self.list_item(item)).collect(),
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
                    md::FenceKind::Documenter {
                        directive,
                        arguments: None,
                    } if self.authored
                        && matches!(
                            directive,
                            md::DocumenterDirective::Autodocs | md::DocumenterDirective::Index
                        ) =>
                    {
                        Some(node.info())
                    }

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
                let segments = code_segments(node.syntax(), &content);
                Block::CodeBlock {
                    language,
                    source: content,
                    source_segments: segments,
                    span,
                }
            }
            md::Block::IndentedCodeBlock(node) => {
                let source = node.content();
                let source_segments = code_segments(node.syntax(), &source);
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

    fn list_item(&mut self, item: md::ListItem) -> ListItem {
        use documentation::syntax::SyntaxKind as K;

        let range = item.syntax().text_range();
        self.record(range);
        let mut blocks = Vec::new();
        let mut paragraph = Vec::new();
        let mut native_blocks = item.blocks();
        // Fatou exposes inline nodes as blocks here and leaves continuation
        // newlines untyped. Walk both together to avoid duplication and lost spaces.
        for element in item.syntax().children_with_tokens() {
            match element {
                rowan::NodeOrToken::Node(node) => {
                    let block = native_blocks.next().unwrap();
                    if let Some(inline) = inline_child(node.into()) {
                        paragraph.push(inline);
                    } else {
                        self.list_paragraph(&mut paragraph, &mut blocks);
                        blocks.push(self.block(block));
                    }
                }
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    K::NEWLINE if !paragraph.is_empty() => {
                        paragraph.push(md::Inline::SoftBreak(token));
                    }
                    K::BLANK_LINE => self.list_paragraph(&mut paragraph, &mut blocks),
                    _ => {
                        if let Some(inline) = inline_child(token.into()) {
                            paragraph.push(inline);
                        }
                    }
                },
            }
        }
        self.list_paragraph(&mut paragraph, &mut blocks);
        ListItem {
            checked: None,
            blocks,
            span: span(range),
        }
    }

    fn list_paragraph(&mut self, paragraph: &mut Vec<md::Inline>, blocks: &mut Vec<Block>) {
        while matches!(paragraph.last(), Some(md::Inline::SoftBreak(_))) {
            paragraph.pop();
        }
        if let (Some(first), Some(last)) = (paragraph.first(), paragraph.last()) {
            let range = TextRange::new(inline_range(first).start(), inline_range(last).end());
            self.record(range);
            blocks.push(Block::Paragraph {
                inlines: self.inlines(paragraph.drain(..)),
                span: span(range),
            });
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
                        let label = self.inlines(node.label());
                        let inferred = node.label().find_map(|inline| {
                            if let md::Inline::Code(code) = inline {
                                Some((code.content(), code.syntax().text_range()))
                            } else {
                                None
                            }
                        });
                        let inferred = if self.authored {
                            Some((
                                if let [Inline::Code { value, .. }] = label.as_slice() {
                                    format!("`{value}`")
                                } else {
                                    format!(
                                        "\"{}\"",
                                        crate::documents::inline_text(&label)
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                    )
                                },
                                node.syntax().text_range(),
                            ))
                        } else {
                            inferred
                        };
                        let target = documenter
                            .target()
                            .map(|target| (target.to_owned(), documenter.target_range().unwrap()))
                            .or(inferred);
                        if let Some((target, range)) = target {
                            self.record(range);
                            return Inline::Link {
                                target: format!("@ref {target}"),
                                inlines: label,
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

// Fatou has typed emphasis wrappers but no child accessors for them.
// Keep this navigation bridge local; Julia Markdown parsing remains in Fatou.
fn inline_children(node: &SyntaxNode) -> Vec<md::Inline> {
    node.children_with_tokens()
        .filter_map(inline_child)
        .collect()
}

fn inline_child(element: SyntaxElement) -> Option<md::Inline> {
    use documentation::syntax::SyntaxKind as K;
    match element {
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
    }
}

pub(crate) fn code_segments(node: &SyntaxNode, source: &str) -> Vec<SourceSegment> {
    use documentation::syntax::SyntaxKind as K;

    let mut remaining = source.len();
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| {
            token.kind() == K::CODE_CONTENT
                || (node.kind() == K::INDENTED_CODE_BLOCK && token.kind() == K::NEWLINE)
        })
        .filter_map(|token| {
            // Container prefixes do not contribute code; Julia also chomps
            // trailing newlines, so retain only bytes present in the body.
            let count = remaining.min(token.text().len());
            remaining -= count;
            (count > 0).then(|| {
                let start = usize::from(token.text_range().start());
                SourceSegment {
                    text: token.text()[..count].into(),
                    span: SourceSpan {
                        start,
                        end: start + count,
                    },
                }
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use documentation::syntax::{DocumentationLanguage, SyntaxKind as K};
    use rowan::{GreenNodeBuilder, Language};

    #[test]
    fn fenced_code_segments_exclude_container_prefixes_and_chomped_newlines() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(DocumentationLanguage::kind_to_raw(K::FENCED_CODE_BLOCK));
        for (kind, text) in [
            (K::WHITESPACE, "  "),
            (K::FENCE, "```"),
            (K::INFO_STRING, "julia"),
            (K::NEWLINE, "\r\n"),
            (K::WHITESPACE, "  "),
            (K::CODE_CONTENT, "α = 1\r\n"),
            (K::WHITESPACE, "  "),
            (K::CODE_CONTENT, "  β = 2\r\n"),
            (K::WHITESPACE, "  "),
            (K::FENCE, "```"),
            (K::NEWLINE, "\r\n"),
        ] {
            builder.token(DocumentationLanguage::kind_to_raw(kind), text);
        }
        builder.finish_node();
        let node = SyntaxNode::new_root(builder.finish());
        let source = "α = 1\r\n  β = 2";
        let segments = code_segments(&node, source);
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>(),
            source
        );
        let raw = node.text().to_string();
        for segment in segments {
            assert_eq!(&raw[segment.span.start..segment.span.end], segment.text);
        }
    }
}
