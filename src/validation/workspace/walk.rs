use super::*;

pub(crate) fn references(
    blocks: &[Block],
    generated: Option<SourceSpan>,
    visit: &mut impl FnMut(ReferenceKind, &str, SourceSpan, bool),
) {
    for block in blocks {
        match block {
            Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                inlines_references(inlines, generated, visit)
            }
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                references(blocks, generated, visit)
            }
            Block::List { items, .. } => {
                for item in items {
                    references(&item.blocks, generated, visit);
                }
            }
            Block::Table { caption, rows, .. } => {
                inlines_references(caption, generated, visit);
                for row in rows {
                    for cell in &row.cells {
                        references(&cell.blocks, generated, visit);
                    }
                }
            }
            Block::CodeCell(cell) => {
                for output in &cell.outputs {
                    let mut origins = output.provenance.iter().filter(|p| {
                        matches!(p.activity, ProvenanceActivity::GeneratedMarkdown { .. })
                    });
                    for representation in &output.representations {
                        if let OutputRepresentation::MarkdownBlocks { blocks, .. } = representation
                        {
                            let origin = origins.next().and_then(|p| p.span).unwrap_or(cell.span);
                            references(blocks, Some(origin), visit);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn inlines_references(
    inlines: &[Inline],
    generated: Option<SourceSpan>,
    visit: &mut impl FnMut(ReferenceKind, &str, SourceSpan, bool),
) {
    for inline in inlines {
        match inline {
            Inline::SemanticReference { target, span, .. } => visit(
                ReferenceKind::Semantic,
                target,
                generated.unwrap_or(*span),
                generated.is_some(),
            ),
            Inline::AutoLink { target, span } => visit(
                ReferenceKind::Link,
                target,
                generated.unwrap_or(*span),
                generated.is_some(),
            ),
            Inline::Link {
                target,
                inlines,
                span,
                ..
            } => {
                visit(
                    ReferenceKind::Link,
                    target,
                    generated.unwrap_or(*span),
                    generated.is_some(),
                );
                inlines_references(inlines, generated, visit);
            }
            Inline::Image {
                target, alt, span, ..
            } => {
                visit(
                    ReferenceKind::Image,
                    target,
                    generated.unwrap_or(*span),
                    generated.is_some(),
                );
                inlines_references(alt, generated, visit);
            }
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. } => inlines_references(inlines, generated, visit),
            _ => {}
        }
    }
}

pub(crate) fn anchors(blocks: &[Block]) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    collect_anchors(blocks, &mut anchors);
    anchors
}
fn collect_anchors(blocks: &[Block], anchors: &mut BTreeSet<String>) {
    for block in blocks {
        match block {
            Block::Heading {
                attributes,
                inlines,
                ..
            } => {
                attribute(attributes, anchors);
                inline_anchors(inlines, anchors);
            }
            Block::Paragraph { inlines, .. } => inline_anchors(inlines, anchors),
            Block::Callout {
                attributes, blocks, ..
            } => {
                attribute(attributes, anchors);
                collect_anchors(blocks, anchors);
            }
            Block::BlockQuote { blocks, .. } => collect_anchors(blocks, anchors),
            Block::List { items, .. } => {
                for item in items {
                    collect_anchors(&item.blocks, anchors);
                }
            }
            Block::Table { caption, rows, .. } => {
                inline_anchors(caption, anchors);
                for row in rows {
                    for cell in &row.cells {
                        collect_anchors(&cell.blocks, anchors);
                    }
                }
            }
            Block::CodeCell(cell) => {
                // Preparation resolves hashpipe labels and fence identifiers.
                if let Some(id) = &cell.identifier {
                    anchors.insert(id.value.clone());
                }
                for label in &cell.labels {
                    anchors.insert(label.value.clone());
                }
                for option in &cell.resolved_options {
                    if option.key == "label"
                        && let CellOptionResolution::Resolved { declaration } = option.resolution
                        && let Some(label) = &cell.options[declaration].cooked_value
                    {
                        anchors.insert(label.clone());
                    }
                }
            }
            _ => {}
        }
    }
}
fn attribute(attributes: &Attributes, anchors: &mut BTreeSet<String>) {
    if let Some(id) = &attributes.identifier {
        anchors.insert(id.value.clone());
    }
}
fn inline_anchors(inlines: &[Inline], anchors: &mut BTreeSet<String>) {
    for inline in inlines {
        match inline {
            Inline::Link {
                attributes,
                inlines,
                ..
            } => {
                attribute(attributes, anchors);
                inline_anchors(inlines, anchors);
            }
            Inline::Image {
                attributes, alt, ..
            } => {
                attribute(attributes, anchors);
                inline_anchors(alt, anchors);
            }
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. } => inline_anchors(inlines, anchors),
            _ => {}
        }
    }
}
