//! Authored documents and their semantic structure.

use std::collections::HashMap;

use panache_parser::syntax::{
    AlertKind as PanacheAlertKind, AstNode, AttributeEntry, AttributeNode, BlockNode,
    CalloutKind as PanacheCalloutKind, CellOptionResolution as PanacheOptionResolution,
    ChunkOptionSource, CodeBlock, InlineNode, ListKind, Table as PanacheTable,
    TableAlignment as PanacheTableAlignment, TextRange, YamlBlockMap, YamlBlockMapEntry,
    YamlBlockSequence, YamlFlowMap, YamlFlowMapEntry, YamlFlowSequence, YamlNode,
};
use panache_parser::{Flavor, ParserOptions, normalize_reference_label};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    AttributeKeyValue, Attributes, Block, CalloutKind, CellOption, CellOptionResolution, CodeCell,
    Document, Inline, ListItem, MetadataEntry, MetadataValue, ResolvedCellOption, SourceSegment,
    SourceSpan, SpannedString, TableAlignment, TableCell, TableRow,
};

/// Authored Markdown profile selected by a content collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthoredFormat {
    /// Polydoc's safe GitHub Flavored Markdown profile.
    Gfm,
    /// Polydoc's documented Quarto Markdown profile.
    Qmd,
}

/// A retained document plus diagnostics produced during translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentParse {
    /// Portable authored-document IR.
    pub document: Document,
    /// Parser and adapter diagnostics in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse authored Markdown in-process and translate it into Polydoc's IR.
pub fn parse_authored_document(source: &str, format: AuthoredFormat) -> DocumentParse {
    let flavor = match format {
        AuthoredFormat::Gfm => Flavor::Gfm,
        AuthoredFormat::Qmd => Flavor::Quarto,
    };
    let mut options = ParserOptions::for_flavor(flavor);
    options.preserve_unresolved_references = true;
    let parsed = panache_parser::parse_document(source, Some(options));

    let references = parsed
        .document()
        .reference_definitions()
        .filter_map(|definition| {
            let url = definition.url()?;
            Some((
                normalize_reference_label(&definition.label()),
                (url, definition.title()),
            ))
        })
        .collect();

    let mut context = AdapterContext {
        format,
        references,
        diagnostics: parsed
            .errors()
            .iter()
            .map(|error| Diagnostic {
                code: DiagnosticCode::InvalidEmbeddedYaml,
                severity: Severity::Error,
                message: error.message.clone(),
                span: Some(span(error.range)),
            })
            .collect(),
    };

    let frontmatter = parsed
        .document()
        .frontmatter()
        .and_then(|metadata| metadata.document())
        .map(|document| {
            document.as_node().map_or_else(
                || MetadataValue::Null {
                    span: span(document.syntax().text_range()),
                },
                |value| yaml_value(&value),
            )
        });
    let blocks = parsed
        .document()
        .block_nodes()
        .filter_map(|block| context.block(block))
        .collect();
    context.diagnostics.sort_by_key(|diagnostic| {
        diagnostic
            .span
            .map_or((usize::MAX, usize::MAX), |span| (span.start, span.end))
    });

    DocumentParse {
        document: Document {
            span: span(parsed.document().syntax().text_range()),
            frontmatter,
            blocks,
        },
        diagnostics: context.diagnostics,
    }
}

struct AdapterContext {
    format: AuthoredFormat,
    references: HashMap<String, (String, Option<String>)>,
    diagnostics: Vec<Diagnostic>,
}

impl AdapterContext {
    fn block(&mut self, block: BlockNode) -> Option<Block> {
        let fallback_kind = format!("{:?}", block.syntax_kind());
        let fallback_raw = block.source_text();
        let fallback_range = block.text_range();
        match block {
            BlockNode::YamlMetadata(_)
            | BlockNode::ReferenceDefinition(_)
            | BlockNode::Trivia(_) => None,
            BlockNode::Paragraph(paragraph) => Some(Block::Paragraph {
                inlines: paragraph
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(paragraph.syntax().text_range()),
            }),
            BlockNode::Plain(plain) => Some(Block::Paragraph {
                inlines: plain
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(plain.syntax().text_range()),
            }),
            BlockNode::Heading(heading) => Some(Block::Heading {
                level: heading.level(),
                attributes: heading.attributes().map(attributes).unwrap_or_default(),
                inlines: heading
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(heading.syntax().text_range()),
            }),
            BlockNode::BlockQuote(quote) => {
                if let Some(alert) = quote.alert() {
                    return Some(self.alert(alert));
                }
                Some(Block::BlockQuote {
                    blocks: quote
                        .block_nodes()
                        .filter_map(|block| self.block(block))
                        .collect(),
                    span: span(quote.syntax().text_range()),
                })
            }
            BlockNode::Alert(alert) => Some(self.alert(alert)),
            BlockNode::List(list) => {
                let ordered = matches!(list.kind(), Some(ListKind::Ordered));
                let list_span = span(list.syntax().text_range());
                let items = list
                    .items()
                    .map(|item| ListItem {
                        checked: item.task_checked(),
                        blocks: item
                            .block_nodes()
                            .filter_map(|block| self.block(block))
                            .collect(),
                        span: span(item.syntax().text_range()),
                    })
                    .collect();
                Some(Block::List {
                    ordered,
                    items,
                    span: list_span,
                })
            }
            BlockNode::CodeBlock(code) => Some(self.code_block(code)),
            BlockNode::FencedDiv(div) => {
                let Some(callout) = div.quarto_callout() else {
                    return Some(self.unsupported(
                        format!("{:?}", div.syntax().kind()),
                        div.syntax().text().to_string(),
                        div.syntax().text_range(),
                    ));
                };
                Some(Block::Callout {
                    kind: callout_kind(callout.kind()),
                    attributes: callout.attributes().map(attributes).unwrap_or_default(),
                    blocks: callout
                        .block_nodes()
                        .filter_map(|block| self.block(block))
                        .collect(),
                    span: span(callout.syntax().text_range()),
                })
            }
            BlockNode::Table(table) => Some(self.table(table)),
            BlockNode::ThematicBreak(rule) => Some(Block::ThematicBreak {
                span: span(rule.text_range()),
            }),
            BlockNode::Figure(figure) => match figure.image() {
                Some(image) => Some(Block::Paragraph {
                    inlines: vec![self.image(image)],
                    span: span(figure.syntax().text_range()),
                }),
                None => Some(self.unsupported(
                    format!("{:?}", figure.syntax().kind()),
                    figure.syntax().text().to_string(),
                    figure.syntax().text_range(),
                )),
            },
            BlockNode::PandocTitleBlock(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::MmdTitleBlock(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::DefinitionList(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::LineBlock(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::FootnoteDefinition(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::DisplayMath(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::TexBlock(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::MystDirective(node) => Some(self.unsupported_syntax(node.syntax())),
            BlockNode::Unknown(node) => {
                Some(self.unsupported(node.syntax_name(), node.source_text(), node.text_range()))
            }
            _ => Some(self.unsupported(fallback_kind, fallback_raw, fallback_range)),
        }
    }

    fn alert(&mut self, alert: panache_parser::syntax::Alert) -> Block {
        let Some(kind) = alert.kind().map(alert_kind) else {
            return self.unsupported(
                format!("{:?}", alert.syntax().kind()),
                alert.syntax().text().to_string(),
                alert.syntax().text_range(),
            );
        };
        Block::Callout {
            kind,
            attributes: Attributes::default(),
            blocks: alert
                .block_nodes()
                .filter_map(|block| self.block(block))
                .collect(),
            span: span(alert.syntax().text_range()),
        }
    }

    fn code_block(&mut self, code: CodeBlock) -> Block {
        if self.format == AuthoredFormat::Qmd
            && let Some(cell) = code.executable_cell()
        {
            let declarations = cell.option_declarations();
            let options = declarations
                .iter()
                .map(|option| CellOption {
                    key: option.key().map(ToOwned::to_owned),
                    canonical_key: option.canonical_key(),
                    raw_value: option.raw_value().map(ToOwned::to_owned),
                    cooked_value: option.cooked_value().map(ToOwned::to_owned),
                    value: option.yaml_value().map(yaml_value),
                    source: match option.source() {
                        ChunkOptionSource::InlineInfo => crate::ir::CellOptionSource::InlineInfo,
                        ChunkOptionSource::HashpipeYaml => {
                            crate::ir::CellOptionSource::HashpipeYaml
                        }
                    },
                    key_span: option.key_range().map(span),
                    value_span: option.value_range().map(span),
                    span: span(option.declaration_range()),
                })
                .collect::<Vec<_>>();
            let resolved_options = cell
                .resolved_options()
                .into_iter()
                .map(|resolved| {
                    let resolution = match resolved.resolution() {
                        PanacheOptionResolution::Resolved(winner) => {
                            let declaration = declaration_index(&declarations, winner);
                            CellOptionResolution::Resolved { declaration }
                        }
                        PanacheOptionResolution::Ambiguous(winners) => {
                            let declarations = winners
                                .iter()
                                .map(|winner| declaration_index(&declarations, winner))
                                .collect::<Vec<_>>();
                            let first = declarations
                                .first()
                                .and_then(|index| options.get(*index))
                                .map(|option| option.span);
                            self.diagnostics.push(Diagnostic {
                                code: DiagnosticCode::AmbiguousCellOption,
                                severity: Severity::Warning,
                                message: format!(
                                    "cell option `{}` has multiple declarations at the winning precedence",
                                    resolved.key()
                                ),
                                span: first,
                            });
                            CellOptionResolution::Ambiguous { declarations }
                        }
                    };
                    ResolvedCellOption {
                        key: resolved.key().to_string(),
                        resolution,
                    }
                })
                .collect();
            return Block::CodeCell(CodeCell {
                language: cell.language(),
                identifier: cell.identifier().map(|(value, range)| SpannedString {
                    value,
                    span: span(range),
                }),
                classes: cell
                    .classes()
                    .into_iter()
                    .map(|(value, range)| SpannedString {
                        value,
                        span: span(range),
                    })
                    .collect(),
                labels: cell
                    .labels()
                    .into_iter()
                    .map(|label| SpannedString {
                        value: label.value().to_string(),
                        span: span(label.value_range()),
                    })
                    .collect(),
                source: cell.code_source(),
                source_segments: source_segments(cell.code_source_segments()),
                options,
                resolved_options,
                code_span: cell.code_range().map(span),
                span: span(cell.text_range()),
            });
        }

        Block::CodeBlock {
            language: code.language(),
            source: code.code_source(),
            source_segments: source_segments(code.code_source_segments()),
            span: span(code.syntax().text_range()),
        }
    }

    fn table(&mut self, table: PanacheTable) -> Block {
        let PanacheTable::Pipe(table) = table else {
            return self.unsupported_syntax(table.syntax());
        };
        let caption = table
            .caption()
            .map(|caption| {
                caption
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect()
            })
            .unwrap_or_default();
        let alignments = table
            .alignments()
            .into_iter()
            .map(table_alignment)
            .collect();
        let rows = table
            .all_rows()
            .map(|row| TableRow {
                header: row.is_header(),
                cells: row
                    .cells()
                    .map(|cell| {
                        let cell_span = span(cell.syntax().text_range());
                        let typed_blocks = cell.block_nodes().collect::<Vec<_>>();
                        let has_semantic_blocks = typed_blocks.iter().any(|block| {
                            matches!(
                                block,
                                BlockNode::Paragraph(_)
                                    | BlockNode::Plain(_)
                                    | BlockNode::BlockQuote(_)
                                    | BlockNode::List(_)
                                    | BlockNode::CodeBlock(_)
                            )
                        });
                        let blocks = if has_semantic_blocks {
                            typed_blocks
                                .into_iter()
                                .filter_map(|block| self.block(block))
                                .collect()
                        } else {
                            vec![Block::Paragraph {
                                inlines: cell
                                    .inline_nodes()
                                    .map(|inline| self.inline(inline))
                                    .collect(),
                                span: cell_span,
                            }]
                        };
                        TableCell {
                            blocks,
                            span: cell_span,
                        }
                    })
                    .collect(),
                span: span(row.syntax().text_range()),
            })
            .collect();
        Block::Table {
            caption,
            alignments,
            rows,
            span: span(table.syntax().text_range()),
        }
    }

    fn inline(&mut self, inline: InlineNode) -> Inline {
        let fallback_kind = format!("{:?}", inline.syntax_kind());
        let fallback_raw = inline.source_text();
        let fallback_range = inline.text_range();
        match inline {
            InlineNode::Text(text) => Inline::Text {
                value: text.decoded(),
                span: span(text.text_range()),
            },
            InlineNode::Space(text) => Inline::Space {
                span: span(text.text_range()),
            },
            InlineNode::SoftBreak(text) => Inline::SoftBreak {
                span: span(text.text_range()),
            },
            InlineNode::HardBreak(text) => Inline::HardBreak {
                span: span(text.text_range()),
            },
            InlineNode::NonbreakingSpace(text) => Inline::NonbreakingSpace {
                span: span(text.text_range()),
            },
            InlineNode::Emphasis(container) => Inline::Emphasis {
                inlines: container
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(container.syntax().text_range()),
            },
            InlineNode::Strong(container) => Inline::Strong {
                inlines: container
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(container.syntax().text_range()),
            },
            InlineNode::Strikeout(container) => Inline::Strikeout {
                inlines: container
                    .inline_nodes()
                    .map(|inline| self.inline(inline))
                    .collect(),
                span: span(container.syntax().text_range()),
            },
            InlineNode::Code(code) => Inline::Code {
                value: code.content(),
                span: span(code.syntax().text_range()),
            },
            InlineNode::Link(link) => self.link(link),
            InlineNode::Image(image) => self.image(image),
            InlineNode::AutoLink(link) => Inline::AutoLink {
                target: link.target(),
                span: span(link.syntax().text_range()),
            },
            InlineNode::UnresolvedReference(reference) => {
                let mut children = reference.inline_nodes().collect::<Vec<_>>();
                if children.len() == 1
                    && let InlineNode::Code(code) = children.remove(0)
                {
                    return Inline::SemanticReference {
                        target: code.content(),
                        span: span(reference.syntax().text_range()),
                        target_span: span(
                            code.content_range()
                                .unwrap_or_else(|| code.syntax().text_range()),
                        ),
                    };
                }
                Inline::Text {
                    value: reference.syntax().text().to_string(),
                    span: span(reference.syntax().text_range()),
                }
            }
            InlineNode::Mark(container)
            | InlineNode::Superscript(container)
            | InlineNode::Subscript(container) => self.unsupported_inline(
                format!("{:?}", container.syntax().kind()),
                container.syntax().text().to_string(),
                container.syntax().text_range(),
            ),
            InlineNode::Math(math) => self.unsupported_inline(
                format!("{:?}", math.syntax().kind()),
                math.syntax().text().to_string(),
                math.syntax().text_range(),
            ),
            InlineNode::Unknown(node) => {
                self.unsupported_inline(node.syntax_name(), node.source_text(), node.text_range())
            }
            _ => self.unsupported_inline(fallback_kind, fallback_raw, fallback_range),
        }
    }

    fn link(&mut self, link: panache_parser::syntax::Link) -> Inline {
        let (target, title) = self.link_target(
            link.dest()
                .map(|destination| (destination.url_content(), destination.title())),
            link.reference().map(|reference| reference.label()),
            link.text().map(|text| text.raw_label()),
        );
        Inline::Link {
            inlines: link
                .inline_nodes()
                .map(|inline| self.inline(inline))
                .collect(),
            target,
            title,
            attributes: link.attributes().map(attributes).unwrap_or_default(),
            span: span(link.syntax().text_range()),
        }
    }

    fn image(&mut self, image: panache_parser::syntax::ImageLink) -> Inline {
        let (target, title) = self.link_target(
            image
                .dest()
                .map(|destination| (destination.url_content(), destination.title())),
            image.reference().map(|reference| reference.label()),
            image.alt().map(|alt| alt.syntax().text().to_string()),
        );
        Inline::Image {
            alt: image
                .inline_nodes()
                .map(|inline| self.inline(inline))
                .collect(),
            target,
            title,
            attributes: image.attributes().map(attributes).unwrap_or_default(),
            span: span(image.syntax().text_range()),
        }
    }

    fn link_target(
        &self,
        inline: Option<(String, Option<String>)>,
        reference: Option<String>,
        fallback_label: Option<String>,
    ) -> (String, Option<String>) {
        if let Some(target) = inline {
            return target;
        }
        let label = reference
            .filter(|label| !label.is_empty())
            .or(fallback_label)
            .unwrap_or_default();
        self.references
            .get(&normalize_reference_label(&label))
            .cloned()
            .unwrap_or((String::new(), None))
    }

    fn unsupported_syntax(&mut self, syntax: &panache_parser::syntax::SyntaxNode) -> Block {
        self.unsupported(
            format!("{:?}", syntax.kind()),
            syntax.text().to_string(),
            syntax.text_range(),
        )
    }

    fn unsupported(&mut self, source_kind: String, raw: String, range: TextRange) -> Block {
        let source_span = span(range);
        self.diagnostics.push(Diagnostic {
            code: DiagnosticCode::UnsupportedAuthoredSyntax,
            severity: Severity::Warning,
            message: format!("unsupported authored syntax: {source_kind}"),
            span: Some(source_span),
        });
        Block::Unsupported {
            source_kind,
            raw,
            span: source_span,
        }
    }

    fn unsupported_inline(&mut self, source_kind: String, raw: String, range: TextRange) -> Inline {
        let source_span = span(range);
        self.diagnostics.push(Diagnostic {
            code: DiagnosticCode::UnsupportedAuthoredSyntax,
            severity: Severity::Warning,
            message: format!("unsupported authored syntax: {source_kind}"),
            span: Some(source_span),
        });
        Inline::Unsupported {
            source_kind,
            raw,
            span: source_span,
        }
    }
}

fn attributes(node: AttributeNode) -> Attributes {
    let mut attributes = Attributes::default();
    for entry in node.entries() {
        match entry {
            AttributeEntry::Identifier(value) => {
                attributes.identifier = Some(SpannedString {
                    value: value.value().to_string(),
                    span: span(value.text_range()),
                });
            }
            AttributeEntry::Class(value) => attributes.classes.push(SpannedString {
                value: value.value().to_string(),
                span: span(value.text_range()),
            }),
            AttributeEntry::KeyValue { key, value } => {
                attributes.key_values.push(AttributeKeyValue {
                    key: SpannedString {
                        value: key.value().to_string(),
                        span: span(key.text_range()),
                    },
                    value: SpannedString {
                        value: value.value().to_string(),
                        span: span(value.text_range()),
                    },
                });
            }
        }
    }
    attributes
}

fn yaml_value(value: &YamlNode) -> MetadataValue {
    match value {
        YamlNode::Scalar(scalar) => MetadataValue::Scalar {
            raw: scalar.raw(),
            value: scalar.value(),
            span: span(scalar.text_range()),
        },
        YamlNode::BlockMap(map) => yaml_block_map(map),
        YamlNode::FlowMap(map) => yaml_flow_map(map),
        YamlNode::BlockSequence(sequence) => yaml_block_sequence(sequence),
        YamlNode::FlowSequence(sequence) => yaml_flow_sequence(sequence),
    }
}

fn yaml_block_map(map: &YamlBlockMap) -> MetadataValue {
    MetadataValue::Mapping {
        entries: map.entries().map(yaml_block_entry).collect(),
        span: span(map.syntax().text_range()),
    }
}

fn yaml_block_entry(entry: YamlBlockMapEntry) -> MetadataEntry {
    let key_scalar = entry.key().and_then(|key| key.scalar());
    let key_span = key_scalar.as_ref().map_or_else(
        || span(entry.syntax().text_range()),
        |key| span(key.text_range()),
    );
    let value = entry.value();
    MetadataEntry {
        key: SpannedString {
            value: entry.key_text().unwrap_or_default(),
            span: key_span,
        },
        value: value
            .as_ref()
            .and_then(|value| value.as_node())
            .as_ref()
            .map_or_else(
                || MetadataValue::Null {
                    span: value
                        .as_ref()
                        .map_or(key_span, |value| span(value.syntax().text_range())),
                },
                yaml_value,
            ),
        span: span(entry.syntax().text_range()),
    }
}

fn yaml_flow_map(map: &YamlFlowMap) -> MetadataValue {
    MetadataValue::Mapping {
        entries: map.entries().map(yaml_flow_entry).collect(),
        span: span(map.syntax().text_range()),
    }
}

fn yaml_flow_entry(entry: YamlFlowMapEntry) -> MetadataEntry {
    let key_scalar = entry.key().and_then(|key| key.scalar());
    let key_span = key_scalar.as_ref().map_or_else(
        || span(entry.syntax().text_range()),
        |key| span(key.text_range()),
    );
    let value = entry.value();
    MetadataEntry {
        key: SpannedString {
            value: entry.key_text().unwrap_or_default(),
            span: key_span,
        },
        value: value
            .as_ref()
            .and_then(|value| value.as_node())
            .as_ref()
            .map_or_else(
                || MetadataValue::Null {
                    span: value
                        .as_ref()
                        .map_or(key_span, |value| span(value.syntax().text_range())),
                },
                yaml_value,
            ),
        span: span(entry.syntax().text_range()),
    }
}

fn yaml_block_sequence(sequence: &YamlBlockSequence) -> MetadataValue {
    MetadataValue::Sequence {
        items: sequence
            .items()
            .map(|item| {
                item.as_node().as_ref().map_or_else(
                    || MetadataValue::Null {
                        span: span(item.syntax().text_range()),
                    },
                    yaml_value,
                )
            })
            .collect(),
        span: span(sequence.syntax().text_range()),
    }
}

fn yaml_flow_sequence(sequence: &YamlFlowSequence) -> MetadataValue {
    MetadataValue::Sequence {
        items: sequence
            .items()
            .map(|item| {
                item.as_node().as_ref().map_or_else(
                    || MetadataValue::Null {
                        span: span(item.syntax().text_range()),
                    },
                    yaml_value,
                )
            })
            .collect(),
        span: span(sequence.syntax().text_range()),
    }
}

fn source_segments(segments: Vec<panache_parser::syntax::CodeSourceSegment>) -> Vec<SourceSegment> {
    segments
        .into_iter()
        .map(|segment| SourceSegment {
            text: segment.text().to_string(),
            span: span(segment.text_range()),
        })
        .collect()
}

fn declaration_index(
    declarations: &[panache_parser::syntax::CellOptionDeclaration],
    needle: &panache_parser::syntax::CellOptionDeclaration,
) -> usize {
    declarations
        .iter()
        .position(|candidate| candidate.declaration_range() == needle.declaration_range())
        .expect("resolved declaration belongs to the cell")
}

fn alert_kind(kind: PanacheAlertKind) -> CalloutKind {
    match kind {
        PanacheAlertKind::Note => CalloutKind::Note,
        PanacheAlertKind::Tip => CalloutKind::Tip,
        PanacheAlertKind::Important => CalloutKind::Important,
        PanacheAlertKind::Warning => CalloutKind::Warning,
        PanacheAlertKind::Caution => CalloutKind::Caution,
    }
}

fn callout_kind(kind: PanacheCalloutKind) -> CalloutKind {
    match kind {
        PanacheCalloutKind::Note => CalloutKind::Note,
        PanacheCalloutKind::Tip => CalloutKind::Tip,
        PanacheCalloutKind::Important => CalloutKind::Important,
        PanacheCalloutKind::Warning => CalloutKind::Warning,
        PanacheCalloutKind::Caution => CalloutKind::Caution,
    }
}

fn table_alignment(alignment: PanacheTableAlignment) -> TableAlignment {
    match alignment {
        PanacheTableAlignment::Default => TableAlignment::Default,
        PanacheTableAlignment::Left => TableAlignment::Left,
        PanacheTableAlignment::Center => TableAlignment::Center,
        PanacheTableAlignment::Right => TableAlignment::Right,
    }
}

fn span(range: TextRange) -> SourceSpan {
    SourceSpan {
        start: range.start().into(),
        end: range.end().into(),
    }
}
