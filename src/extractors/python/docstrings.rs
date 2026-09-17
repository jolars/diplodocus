//! Inert PEP 257 and NumPy docstrings with decoded and original attribution.

use serde::{Deserialize, Serialize};

use pydocstring::model::{FreeSectionKind, SectionKind};
use pydocstring::parse::{Section, parse_numpy};
use pydocstring::syntax::{Parsed, SyntaxElement, SyntaxKind, SyntaxNode};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSource, Severity};
use crate::ir::{
    Attributes, Block, Document, DocumentFormat, Inline, ListItem, Provenance, ProvenanceActivity,
    SourceLocation, SourceSegment, SourceSpan, SourcedDocument,
};

#[path = "docstrings/inline.rs"]
mod inline;
#[path = "docstrings/mapping.rs"]
mod mapping;

/// A caller-proven, byte-identical region of decoded text and original source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocstringSourceSegment {
    /// UTF-8 bytes within the decoded docstring.
    pub decoded: SourceSpan,
    /// Equal-length bytes within the original Python file.
    pub source: SourceSpan,
}

/// Structured documentation and deterministic source-file diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocstringParse {
    /// Inert document with decoded-document-relative spans.
    pub document: SourcedDocument,
    /// Diagnostics attributed to exact original ranges or the enclosing literal.
    pub diagnostics: Vec<Diagnostic>,
    /// Validated exact mappings; gaps make no claim about original offsets.
    pub source_map: Vec<DocstringSourceSegment>,
}

/// Parse decoded text without importing Python or evaluating documented code.
///
/// Spans in the document address `text`, including its original indentation.
/// Diagnostic spans address the original Python file. The caller must prove
/// that each supplied mapping contains identical bytes; this function validates
/// bounds, UTF-8 boundaries, lengths, and ordering, but does not read the file.
/// Gaps and absent maps retain the enclosing source and produce a warning for
/// nonempty documentation. Invalid maps are rejected as a whole.
///
/// Parameters, returns, exceptions, and references use headings and list entries
/// in the existing document vocabulary. NumPy examples are display code only.
/// Unsupported syntax remains a visible, escaped placeholder. Neither links nor
/// raw markup grant rendering trust or execution authority.
pub fn parse_docstring(
    text: &str,
    source: SourceLocation,
    source_map: Option<&[DocstringSourceSegment]>,
) -> DocstringParse {
    let source_map = mapping::validate(text, &source, source_map.unwrap_or_default());
    let mut context = Context {
        text,
        diagnostics: Vec::new(),
        panache_used: false,
    };
    let parsed = parse_numpy(text);
    context.missing(parsed.root());
    let mut blocks = Vec::new();
    for element in parsed.root().children() {
        match element {
            SyntaxElement::Node(node) if node.kind() == SyntaxKind::SECTION => {
                blocks.extend(context.section(&parsed, node));
            }
            SyntaxElement::Node(node) if is_prose(node.kind()) => {
                blocks.extend(context.prose(node));
            }
            SyntaxElement::Node(node) => blocks.push(context.unsupported(node)),
            SyntaxElement::Token(token) if !token.kind().is_trivia() && !token.is_missing() => {
                blocks.push(context.unsupported_range(token.kind().name(), span(token.range())));
            }
            _ => {}
        }
    }
    let diagnostic_source = DiagnosticSource::Repository {
        repository: source.repository.clone(),
        path: source.path.clone(),
    };
    let mut incomplete = !text.trim().is_empty() && !mapping::complete(text, &source_map);
    for diagnostic in &mut context.diagnostics {
        diagnostic.source = Some(diagnostic_source.clone());
        let mapped = diagnostic
            .span
            .and_then(|s| mapping::original(s, &source_map));
        incomplete |= mapped.is_none();
        diagnostic.span = mapped.or(source.span);
        diagnostic.related_spans = diagnostic
            .related_spans
            .iter()
            .filter_map(|span| {
                let mapped = mapping::original(*span, &source_map);
                incomplete |= mapped.is_none();
                mapped.or(source.span)
            })
            .collect();
    }
    if incomplete {
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::PythonDocstringSourceAttribution,
            Severity::Warning,
            "docstring offsets cannot all be mapped exactly to Python source; enclosing source retained",
        ).with_source(diagnostic_source.clone());
        diagnostic.span = source.span;
        context.diagnostics.push(diagnostic);
    }
    context.diagnostics.sort();
    context.diagnostics.dedup();
    let mut tools = std::collections::BTreeMap::from([
        ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
        ("pydocstring".into(), "0.4.1".into()),
    ]);
    if context.panache_used {
        tools.insert("panache-parser".into(), "0.29.0".into());
    }
    DocstringParse {
        document: SourcedDocument {
            document: Document {
                span: SourceSpan {
                    start: 0,
                    end: text.len(),
                },
                frontmatter: None,
                blocks,
            },
            source_format: DocumentFormat::Extracted {
                name: "numpy-docstring".into(),
            },
            source_location: Some(source.clone()),
            raw_source: Some(text.into()),
            provenance: vec![Provenance {
                activity: ProvenanceActivity::Declaration,
                source: Some(diagnostic_source),
                span: source.span,
                tools,
            }],
        },
        diagnostics: context.diagnostics,
        source_map,
    }
}

struct Context<'a> {
    text: &'a str,
    // These ranges stay decoded-relative until the final attribution pass.
    diagnostics: Vec<Diagnostic>,
    panache_used: bool,
}

impl Context<'_> {
    fn warn(&mut self, code: DiagnosticCode, message: impl Into<String>, span: SourceSpan) {
        let mut diagnostic = Diagnostic::new(code, Severity::Warning, message);
        diagnostic.span = Some(span);
        self.diagnostics.push(diagnostic);
    }

    fn missing(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            if child.range().is_empty() {
                self.warn(
                    DiagnosticCode::PythonIncompleteDocstring,
                    format!(
                        "incomplete docstring: missing {}",
                        child.kind().name().to_ascii_lowercase()
                    ),
                    span(child.range()),
                );
            } else if let SyntaxElement::Node(node) = child {
                self.missing(node);
            }
        }
    }

    fn section(&mut self, parsed: &Parsed, node: &SyntaxNode) -> Vec<Block> {
        let section = Section::cast(parsed, node).expect("section node");
        if !matches!(
            section.kind(),
            SectionKind::Parameters
                | SectionKind::Returns
                | SectionKind::Raises
                | SectionKind::References
                | SectionKind::FreeText(FreeSectionKind::Notes | FreeSectionKind::Examples)
        ) {
            return vec![self.unsupported(node)];
        }
        let Some(header) = node.find_node(SyntaxKind::SECTION_HEADER) else {
            return vec![self.unsupported(node)];
        };
        let Some(name) = header.find_token(SyntaxKind::NAME) else {
            return vec![self.unsupported(node)];
        };
        let mut blocks = vec![Block::Heading {
            level: 2,
            attributes: Attributes::default(),
            inlines: vec![Inline::Text {
                value: name.text(self.text).into(),
                span: span(name.range()),
            }],
            span: span(header.range()),
        }];
        let examples = section.kind() == SectionKind::FreeText(FreeSectionKind::Examples);
        let mut items = Vec::new();
        for element in node.children() {
            let SyntaxElement::Node(child) = element else {
                continue;
            };
            if child.kind() == SyntaxKind::SECTION_HEADER {
                continue;
            }
            if matches!(child.kind(), SyntaxKind::ENTRY | SyntaxKind::CITATION) {
                items.push(self.entry(child));
                continue;
            }
            flush_items(&mut blocks, &mut items);
            if is_prose(child.kind()) {
                if examples && !child.range().is_empty() {
                    blocks.extend(self.examples(child));
                } else {
                    blocks.extend(self.prose(child));
                }
            } else {
                blocks.push(self.unsupported(child));
            }
        }
        flush_items(&mut blocks, &mut items);
        blocks
    }

    fn entry(&mut self, node: &SyntaxNode) -> ListItem {
        let entry_span = span(node.range());
        let description = node.find_node(SyntaxKind::DESCRIPTION);
        let end = description.map_or(entry_span.end, |d| span(d.range()).start);
        let header = self.text[entry_span.start..end].trim_end();
        let header_span = SourceSpan {
            start: entry_span.start,
            end: entry_span.start + header.len(),
        };
        let mut blocks = vec![Block::Paragraph {
            inlines: vec![Inline::Code {
                value: header.into(),
                span: header_span,
            }],
            span: header_span,
        }];
        if let Some(description) = description {
            blocks.extend(self.prose(description));
        }
        ListItem {
            checked: None,
            blocks,
            span: entry_span,
        }
    }

    fn prose(&mut self, node: &SyntaxNode) -> Vec<Block> {
        self.prose_lines(
            node.tokens(SyntaxKind::TEXT_LINE)
                .map(|line| span(line.range())),
        )
    }

    fn prose_lines(&mut self, lines: impl IntoIterator<Item = SourceSpan>) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut inlines = Vec::new();
        let mut paragraph_span: Option<SourceSpan> = None;
        for range in lines {
            if range.start == range.end {
                continue;
            }
            if let Some(previous) = paragraph_span {
                let gap = &self.text[previous.end..range.start];
                if gap.bytes().filter(|b| *b == b'\n').count() > 1 {
                    blocks.push(Block::Paragraph {
                        inlines: std::mem::take(&mut inlines),
                        span: previous,
                    });
                    paragraph_span = None;
                } else {
                    inlines.push(Inline::SoftBreak {
                        span: SourceSpan {
                            start: previous.end,
                            end: range.start,
                        },
                    });
                }
            }
            paragraph_span = Some(SourceSpan {
                start: paragraph_span.map_or(range.start, |s| s.start),
                end: range.end,
            });
            inlines.extend(inline::parse_line(
                &self.text[range.start..range.end],
                range.start,
                &mut self.diagnostics,
                &mut self.panache_used,
            ));
        }
        if let Some(span) = paragraph_span {
            blocks.push(Block::Paragraph { inlines, span });
        }
        blocks
    }

    fn examples(&mut self, node: &SyntaxNode) -> Vec<Block> {
        let lines: Vec<_> = node
            .tokens(SyntaxKind::TEXT_LINE)
            .map(|line| span(line.range()))
            .filter(|span| span.start != span.end)
            .collect();
        let mut blocks = Vec::new();
        let mut start = 0;
        while start < lines.len() {
            let first = &self.text[lines[start].start..lines[start].end];
            let fence = if first.starts_with("```") {
                Some('`')
            } else if first.starts_with("~~~") {
                Some('~')
            } else {
                None
            };
            let fence_width =
                fence.map(|marker| first.chars().take_while(|ch| *ch == marker).count());
            let mut end = start + 1;
            if let (Some(marker), Some(width)) = (fence, fence_width) {
                while end < lines.len() {
                    let line = self.text[lines[end].start..lines[end].end].trim_end();
                    end += 1;
                    if line.chars().count() >= width && line.chars().all(|ch| ch == marker) {
                        break;
                    }
                }
            } else {
                while end < lines.len() {
                    let gap = &self.text[lines[end - 1].end..lines[end].start];
                    if gap.bytes().filter(|b| *b == b'\n').count() > 1 {
                        break;
                    }
                    end += 1;
                }
            }
            if fence.is_some() || first.starts_with(">>>") {
                blocks.push(self.example(SourceSpan {
                    start: lines[start].start,
                    end: lines[end - 1].end,
                }));
            } else {
                blocks.extend(self.prose_lines(lines[start..end].iter().copied()));
            }
            start = end;
        }
        blocks
    }

    fn example(&self, range: SourceSpan) -> Block {
        let line_start = self.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
        let indent = range.start - line_start;
        let mut offset = range.start;
        let mut segments = Vec::new();
        for (index, line) in self.text[range.start..range.end]
            .split_inclusive('\n')
            .enumerate()
        {
            let skip = if index == 0 {
                0
            } else {
                line.bytes()
                    .take(indent)
                    .take_while(|b| matches!(b, b' ' | b'\t'))
                    .count()
            };
            segments.push(SourceSegment {
                text: line[skip..].into(),
                span: SourceSpan {
                    start: offset + skip,
                    end: offset + line.len(),
                },
            });
            offset += line.len();
        }
        Block::CodeBlock {
            language: Some("python".into()),
            source: segments.iter().map(|s| s.text.as_str()).collect(),
            source_segments: segments,
            span: range,
        }
    }

    fn unsupported(&mut self, node: &SyntaxNode) -> Block {
        self.unsupported_range(node.kind().name(), span(node.range()))
    }

    fn unsupported_range(&mut self, kind: &str, range: SourceSpan) -> Block {
        self.warn(
            DiagnosticCode::PythonUnsupportedDocstring,
            format!("unsupported docstring {kind}; source retained"),
            range,
        );
        Block::Unsupported {
            source_kind: format!("numpy-{kind}"),
            raw: self.text[range.start..range.end].into(),
            span: range,
        }
    }
}

fn is_prose(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::SUMMARY
            | SyntaxKind::EXTENDED_SUMMARY
            | SyntaxKind::DESCRIPTION
            | SyntaxKind::PARAGRAPH
    )
}

fn flush_items(blocks: &mut Vec<Block>, items: &mut Vec<ListItem>) {
    if let (Some(first), Some(last)) = (items.first(), items.last()) {
        blocks.push(Block::List {
            ordered: false,
            span: SourceSpan {
                start: first.span.start,
                end: last.span.end,
            },
            items: std::mem::take(items),
        });
    }
}

fn span(range: pydocstring::text::TextRange) -> SourceSpan {
    SourceSpan {
        start: usize::from(range.start()),
        end: usize::from(range.end()),
    }
}
