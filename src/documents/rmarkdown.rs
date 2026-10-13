use super::*;
use crate::ir::{DocumentContainerKind, InlineStyle};

pub(super) fn math_environment(source: &str) -> bool {
    source
        .trim_start()
        .strip_prefix("\\begin{")
        .and_then(|s| s.split_once('}'))
        .is_some_and(|(name, _)| {
            matches!(
                name,
                "matrix"
                    | "pmatrix"
                    | "bmatrix"
                    | "Bmatrix"
                    | "vmatrix"
                    | "Vmatrix"
                    | "smallmatrix"
                    | "aligned"
                    | "alignedat"
                    | "align"
                    | "align*"
                    | "gathered"
                    | "cases"
                    | "array"
            )
        })
}

pub(super) fn math_delimiters(root: &panache_parser::syntax::SyntaxNode) -> Vec<SourceSpan> {
    let mut markers = Vec::new();
    for node in root
        .descendants()
        .filter_map(panache_parser::syntax::TexBlock::cast)
    {
        if !math_environment(&node.text()) {
            continue;
        }
        let Some(before) = node.syntax().prev_sibling() else {
            continue;
        };
        let Some(after) = node.syntax().next_sibling() else {
            continue;
        };
        let left = before.text().to_string();
        let right = after.text().to_string();
        if left.trim_end().ends_with("$$") && right.trim_start().starts_with("$$") {
            let opening = span(before.text_range()).start + left.trim_end().len() - 2;
            let closing =
                span(after.text_range()).start + right.find("$$").expect("math delimiter");
            markers.extend([
                SourceSpan {
                    start: opening,
                    end: opening + 2,
                },
                SourceSpan {
                    start: closing,
                    end: closing + 2,
                },
            ]);
        }
    }
    markers
}
use panache_parser::syntax::{Citation, InlineExecutable, InlineFootnote, UnknownNode};

/// Source ranges needed to reconcile native execution with an R Markdown input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmdPreparation {
    /// Original executable chunk ranges, including disabled chunks.
    pub chunks: Vec<SourceSpan>,
    /// Original inline expression ranges, including metadata expressions.
    pub inline: Vec<SourceSpan>,
    /// Local eval declarations in chunk order, without evaluating their expressions.
    pub eval_options: Vec<Option<String>>,
}

/// Inspect executable R Markdown syntax without loading R or evaluating options.
pub fn prepare_rmd_document(source: &str) -> (RmdPreparation, Vec<Diagnostic>) {
    let parsed =
        panache_parser::parse_document(source, Some(ParserOptions::for_flavor(Flavor::RMarkdown)));
    let mut chunks = Vec::new();
    let mut inline = Vec::new();
    let mut diagnostics = Vec::new();
    let mut eval_options = Vec::new();
    for node in parsed.document().syntax().descendants() {
        if let Some(code) = CodeBlock::cast(node.clone())
            && let Some(cell) = code.executable_cell()
        {
            chunks.push(span(node.text_range()));
            eval_options.push(
                cell.resolved_options()
                    .into_iter()
                    .find(|o| o.key() == "eval")
                    .and_then(|o| match o.resolution() {
                        panache_parser::syntax::CellOptionResolution::Resolved(option) => {
                            option.raw_value().map(str::to_owned)
                        }
                        _ => None,
                    }),
            );
            if cell
                .language()
                .is_none_or(|lang| !lang.eq_ignore_ascii_case("r"))
            {
                diagnostics.push(unsupported(
                    span(node.text_range()),
                    "R Markdown supports only R chunks.",
                ));
            }
        }
        if let Some(code) = InlineExecutable::cast(node) {
            inline.push(span(code.syntax().text_range()));
            if code
                .language()
                .is_none_or(|lang| !lang.eq_ignore_ascii_case("r"))
            {
                diagnostics.push(unsupported(
                    span(code.syntax().text_range()),
                    "R Markdown supports only inline R.",
                ));
            }
        }
    }
    if let Some(metadata) = parsed.document().frontmatter() {
        let mut options = ParserOptions::for_flavor(Flavor::RMarkdown);
        options.extensions.yaml_metadata_block = false;
        let range = span(metadata.syntax().text_range());
        let yaml = panache_parser::parse_document(&source[range.start..range.end], Some(options));
        inline.extend(
            yaml.document()
                .syntax()
                .descendants()
                .filter_map(InlineExecutable::cast)
                .map(|code| {
                    let local = span(code.syntax().text_range());
                    SourceSpan {
                        start: range.start + local.start,
                        end: range.start + local.end,
                    }
                }),
        );
    }
    inline.sort_by_key(|span| span.start);
    inline.dedup();
    (
        RmdPreparation {
            chunks,
            inline,
            eval_options,
        },
        diagnostics,
    )
}

/// Parse evaluated Markdown with executable syntax disabled.
pub fn parse_evaluated_rmd(source: &str) -> DocumentParse {
    parse_document(source, AuthoredFormat::Rmd, ParseMode::Evaluated)
}

impl AdapterContext {
    pub(super) fn rmd_table(&mut self, table: PanacheTable) -> Block {
        let range = span(table.syntax().text_range());
        let caption = table
            .caption()
            .map(|c| c.inline_nodes().map(|i| self.inline(i)).collect())
            .unwrap_or_default();
        let rows: Vec<_> = table
            .rows()
            .into_iter()
            .map(|row| TableRow {
                header: row.is_header(),
                cells: row
                    .cells()
                    .map(|cell| {
                        let range = span(cell.syntax().text_range());
                        TableCell {
                            blocks: vec![Block::Paragraph {
                                inlines: cell.inline_nodes().map(|i| self.inline(i)).collect(),
                                span: range,
                            }],
                            span: range,
                        }
                    })
                    .collect(),
                span: span(row.syntax().text_range()),
            })
            .collect();
        let alignments =
            vec![TableAlignment::Default; rows.iter().map(|r| r.cells.len()).max().unwrap_or(0)];
        Block::Table {
            caption,
            alignments,
            rows,
            span: range,
        }
    }

    pub(super) fn rmd_unknown(&mut self, unknown: &UnknownNode) -> Option<Inline> {
        if matches!(
            unknown.syntax_kind(),
            panache_parser::syntax::SyntaxKind::LINE_PREFIX
                | panache_parser::syntax::SyntaxKind::TABLE_CAPTION_PREFIX
        ) {
            return Some(Inline::Text {
                value: String::new(),
                span: span(unknown.text_range()),
            });
        }
        let node = unknown.syntax_element().as_node()?.clone();
        let range = span(node.text_range());
        if let Some(command) = panache_parser::syntax::LatexCommand::cast(node.clone())
            && let Some(code) = command
                .text()
                .strip_prefix("\\code{")
                .and_then(|code| code.strip_suffix('}'))
        {
            return Some(Inline::Code {
                value: code.replace("\\&", "&"),
                span: range,
            });
        }
        if let Some(math) = panache_parser::syntax::DisplayMath::cast(node.clone()) {
            return Some(Inline::Math {
                latex: math.content(),
                fallback: None,
                span: range,
            });
        }
        if let Some(citation) = Citation::cast(node.clone()) {
            return Some(Inline::Citation {
                raw: node.text().to_string(),
                keys: citation.key_texts(),
                inlines: Vec::new(),
                span: range,
            });
        }
        if let Some(reference) = panache_parser::syntax::FootnoteReference::cast(node.clone()) {
            let id = reference.id();
            let number = if let Some(number) = self.footnote_numbers.get(&id) {
                *number
            } else if let Some(note) = self.footnote_definitions.get(&id).cloned() {
                let number = self.footnotes.len() + 1;
                self.footnote_numbers.insert(id, number);
                let parsed = parse_evaluated_rmd(&note);
                self.diagnostics.extend(parsed.diagnostics);
                let blocks = parsed.document.blocks;
                self.footnotes.push(Block::Container {
                    kind: DocumentContainerKind::Footnote,
                    identifier: Some(format!("fn-{number}")),
                    blocks,
                    span: range,
                });
                number
            } else {
                self.diagnostics
                    .push(unsupported(range, "Undefined footnote."));
                return Some(Inline::Text {
                    value: unknown.source_text(),
                    span: range,
                });
            };
            return Some(Inline::Styled {
                style: InlineStyle::Superscript,
                inlines: vec![Inline::Link {
                    target: format!("#fn-{number}"),
                    inlines: vec![Inline::Text {
                        value: number.to_string(),
                        span: range,
                    }],
                    title: None,
                    attributes: Attributes::default(),
                    span: range,
                }],
                span: range,
            });
        }
        if let Some(footnote) = InlineFootnote::cast(node) {
            let number = self.footnotes.len() + 1;
            let parsed = parse_evaluated_rmd(&footnote.content());
            self.diagnostics.extend(parsed.diagnostics);
            self.footnotes.push(Block::Container {
                kind: DocumentContainerKind::Footnote,
                identifier: Some(format!("fn-{number}")),
                blocks: parsed.document.blocks,
                span: range,
            });
            return Some(Inline::Styled {
                style: InlineStyle::Superscript,
                inlines: vec![Inline::Link {
                    target: format!("#fn-{number}"),
                    inlines: vec![Inline::Text {
                        value: number.to_string(),
                        span: range,
                    }],
                    title: None,
                    attributes: Attributes::default(),
                    span: range,
                }],
                span: range,
            });
        }
        None
    }
}

fn unsupported(range: SourceSpan, message: &str) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::UnsupportedAuthoredSyntax,
        Severity::Error,
        message,
    );
    diagnostic.span = Some(range);
    diagnostic
}
