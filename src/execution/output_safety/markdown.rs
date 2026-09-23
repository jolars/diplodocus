//! Live fragments and decoded cache trees converge on one structural validator.
use super::*;
use crate::diagnostics::Severity;
use crate::documents::MarkdownFragmentParse;
use crate::ir::{
    self, Attributes, Block, Inline, OutputRepresentation, Provenance, ProvenanceActivity,
    SourceSpan,
};

/// Named array edges in the fragment tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeEdge {
    /// Block array.
    Blocks,
    /// Inline array.
    Inlines,
    /// List items.
    Items,
    /// Table rows.
    Rows,
    /// Table cells.
    Cells,
    /// Table caption inlines.
    Caption,
    /// Image alternative-text inlines.
    Alt,
}
/// A structural image address, independent of URL, span, or content digest.
pub type NodeAddress = Vec<(NodeEdge, usize)>;

/// One exact image node and its actively validated image record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBinding {
    address: NodeAddress,
    asset: ExecutionAsset,
}
impl ImageBinding {
    /// Ordered named edges from the fragment root to this occurrence.
    pub fn address(&self) -> &[(NodeEdge, usize)] {
        &self.address
    }
    /// Immutable image metadata; publication resolves the current output URL.
    pub fn asset(&self) -> &ExecutionAsset {
        &self.asset
    }
}

/// Immutable inert blocks and complete structural image bindings.
///
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedMarkdown;
/// let _: ValidatedMarkdown = serde_json::from_str("{}").unwrap();
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedMarkdown;
/// fn require_serializable<T: serde::Serialize>() {}
/// require_serializable::<ValidatedMarkdown>();
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedMarkdown;
/// fn mutate(value: &mut ValidatedMarkdown) { value.canonical_content().blocks.clear(); }
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::{ValidatedMarkdown, DecodedMarkdown};
/// fn construct(provenance: diplodocus::ir::Provenance) -> ValidatedMarkdown {
///     ValidatedMarkdown { content: DecodedMarkdown { blocks: vec![] }, blocks: vec![], bindings: vec![], provenance }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMarkdown {
    content: DecodedMarkdown,
    blocks: Vec<Block>,
    bindings: Vec<ImageBinding>,
    provenance: Provenance,
}
impl ValidatedMarkdown {
    /// Canonical typed content shared by live and cached digest projection.
    pub fn canonical_content(&self) -> &DecodedMarkdown {
        &self.content
    }
    /// Inert workspace projection; image target strings carry no independent trust.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
    /// Every image occurrence, even when spans and targets repeat.
    pub fn image_bindings(&self) -> &[ImageBinding] {
        &self.bindings
    }
    /// Complete image references, including nested alternatives.
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset> {
        self.bindings.iter().map(|binding| &binding.asset)
    }
    /// Fragment parser evidence and original producing cell attribution.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

#[derive(Debug)]
enum LiveError {
    Policy(MarkdownRejectionReason),
    Asset(AssetError),
}
impl From<AssetError> for LiveError {
    fn from(value: AssetError) -> Self {
        Self::Asset(value)
    }
}

struct Live<'a> {
    assets: &'a mut PageAssetStore,
    verified: VerifiedAssets,
}
impl Live<'_> {
    fn image(&mut self, target: &str) -> Result<AssetUse, LiveError> {
        let target = urls::image(target).map_err(|error| {
            LiveError::Policy(match error {
                urls::UrlError::Url => MarkdownRejectionReason::Url,
                urls::UrlError::RemoteImage => MarkdownRejectionReason::RemoteImage,
            })
        })?;
        let asset = self.assets.stage_local(&target)?;
        self.verified
            .assets
            .insert(asset.reference.fingerprint.value.clone(), asset.clone());
        Ok(AssetUse::from(&asset))
    }
}

fn expected_provenance(origin: &OutputOrigin, context: &AuthoredOutputContext) -> Provenance {
    Provenance {
        activity: ProvenanceActivity::GeneratedMarkdown {
            collection: context.collection.clone(),
            cell: origin.cell,
            output: origin.slot,
        },
        source: Some(context.diagnostic_source()),
        span: Some(origin.cell_span),
        tools: crate::provenance::builtin_tools(),
    }
}

/// Stage live fragment images and validate all inert structure and decoded links.
pub fn validate_markdown_live(
    fragment: MarkdownFragmentParse,
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &mut PageAssetStore,
) -> Result<Validation<ValidatedMarkdown>, ExecutionFailure> {
    let attribution = DiagnosticAttribution::output(context, origin, None);
    let rejected = |mut diagnostics: Vec<ExecutionDiagnostic>, reason| {
        diagnostics.push(ExecutionDiagnostic::MarkdownRejected {
            attribution: attribution.clone(),
            reason,
        });
        Ok(Validation::Rejected { diagnostics })
    };
    if origin.fragment.is_none()
        || fragment.provenance != expected_provenance(origin, context)
        || origin.cell_span.start > origin.cell_span.end
    {
        return rejected(Vec::new(), MarkdownRejectionReason::Structure);
    }
    if fragment
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(ExecutionFailure {
            kind: ExecutionFailureKind::OutputValidation,
            diagnostics: fragment.diagnostics,
            cleanup_diagnostics: Vec::new(),
        });
    }
    let OutputRepresentation::MarkdownBlocks { media_type, blocks } = fragment.representation
    else {
        return rejected(Vec::new(), MarkdownRejectionReason::Structure);
    };
    if media_type != "text/markdown" {
        return rejected(Vec::new(), MarkdownRejectionReason::Structure);
    }
    let mut diagnostics =
        match super::warnings::project(&blocks, &fragment.diagnostics, origin, context) {
            Ok(diagnostics) => diagnostics,
            Err(_) => return rejected(Vec::new(), MarkdownRejectionReason::Structure),
        };
    let mut live = Live {
        assets,
        verified: VerifiedAssets::new(),
    };
    let content = match live.blocks(blocks, 0) {
        Ok(blocks) => DecodedMarkdown { blocks },
        Err(LiveError::Policy(reason)) => return rejected(diagnostics, reason),
        Err(LiveError::Asset(error)) => {
            if let Some(kind) = error.failure_kind() {
                let mut failure = failure(kind, context, origin);
                let mut preceding: Vec<_> = diagnostics
                    .iter()
                    .map(|d| d.to_diagnostic(context.collection()))
                    .collect();
                preceding.append(&mut failure.diagnostics);
                failure.diagnostics = preceding;
                return Err(failure);
            }
            diagnostics.push(if error == AssetError::UnsafeSvg {
                ExecutionDiagnostic::SvgRejected { attribution }
            } else {
                ExecutionDiagnostic::InvalidImage {
                    attribution,
                    media_type: "image/*".into(),
                }
            });
            return Ok(Validation::Rejected { diagnostics });
        }
    };
    let (value, _) = match restore(content, origin, context, &live.verified) {
        Ok(value) => value,
        Err(RestoreRejection::Url) => return rejected(diagnostics, MarkdownRejectionReason::Url),
        Err(_) => return rejected(diagnostics, MarkdownRejectionReason::Structure),
    };
    Ok(Validation::Accepted { value, diagnostics })
}

/// Restore generated Markdown without resolving any original image source paths.
pub fn restore_markdown(
    decoded: DecodedMarkdown,
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &VerifiedAssets,
) -> Result<ValidatedMarkdown, RestoreRejection> {
    restore(decoded, origin, context, assets).map(|(value, _)| value)
}

type UnsupportedNodes = Vec<(SourceSpan, String)>;
fn restore(
    decoded: DecodedMarkdown,
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &VerifiedAssets,
) -> Result<(ValidatedMarkdown, UnsupportedNodes), RestoreRejection> {
    let fragment = origin.fragment.ok_or(RestoreRejection::Structure)?;
    if origin.cell_span.start > origin.cell_span.end {
        return Err(RestoreRejection::Structure);
    }
    let mut check = Check {
        context,
        assets,
        bindings: Vec::new(),
        unsupported: Vec::new(),
        address: Vec::new(),
    };
    let blocks = check.blocks(
        &decoded.blocks,
        SourceSpan {
            start: 0,
            end: fragment.byte_length,
        },
        NodeEdge::Blocks,
        0,
    )?;
    Ok((
        ValidatedMarkdown {
            content: decoded,
            blocks,
            bindings: check.bindings,
            provenance: expected_provenance(origin, context),
        },
        check.unsupported,
    ))
}

fn span_in(span: SourceSpan, parent: SourceSpan) -> Result<(), RestoreRejection> {
    if span.start > span.end || span.start < parent.start || span.end > parent.end {
        Err(RestoreRejection::Structure)
    } else {
        Ok(())
    }
}
fn check_attributes(attributes: &Attributes) -> Result<(), RestoreRejection> {
    // The GFM fragment adapter disables generated identifiers and attribute syntax.
    // Do not let a cache tree reintroduce attributes the active reader cannot emit.
    if attributes != &Attributes::default() {
        Err(RestoreRejection::Structure)
    } else {
        Ok(())
    }
}
struct Check<'a> {
    context: &'a AuthoredOutputContext,
    assets: &'a VerifiedAssets,
    bindings: Vec<ImageBinding>,
    unsupported: UnsupportedNodes,
    address: NodeAddress,
}

impl Live<'_> {
    fn blocks(&mut self, nodes: Vec<Block>, depth: usize) -> Result<Vec<FragmentBlock>, LiveError> {
        if depth > 128 {
            return Err(LiveError::Policy(MarkdownRejectionReason::Structure));
        }
        nodes
            .into_iter()
            .map(|node| {
                Ok(match node {
                    Block::Paragraph { inlines, span } => FragmentBlock::Paragraph {
                        inlines: self.inlines(inlines, depth + 1)?,
                        span,
                    },
                    Block::Heading {
                        level,
                        attributes,
                        inlines,
                        span,
                    } => FragmentBlock::Heading {
                        level,
                        attributes,
                        inlines: self.inlines(inlines, depth + 1)?,
                        span,
                    },
                    Block::BlockQuote { blocks, span } => FragmentBlock::BlockQuote {
                        blocks: self.blocks(blocks, depth + 1)?,
                        span,
                    },
                    Block::List {
                        ordered,
                        items,
                        span,
                    } => FragmentBlock::List {
                        ordered,
                        items: items
                            .into_iter()
                            .map(|item| {
                                Ok(FragmentListItem {
                                    checked: item.checked,
                                    blocks: self.blocks(item.blocks, depth + 1)?,
                                    span: item.span,
                                })
                            })
                            .collect::<Result<_, LiveError>>()?,
                        span,
                    },
                    Block::ThematicBreak { span } => FragmentBlock::ThematicBreak { span },
                    Block::CodeBlock {
                        language,
                        source,
                        source_segments,
                        span,
                    } => FragmentBlock::CodeBlock {
                        language,
                        source,
                        source_segments,
                        span,
                    },
                    Block::Table {
                        caption,
                        alignments,
                        rows,
                        span,
                    } => FragmentBlock::Table {
                        caption: self.inlines(caption, depth + 1)?,
                        alignments,
                        rows: rows
                            .into_iter()
                            .map(|row| {
                                Ok(FragmentTableRow {
                                    header: row.header,
                                    cells: row
                                        .cells
                                        .into_iter()
                                        .map(|cell| {
                                            Ok(FragmentTableCell {
                                                blocks: self.blocks(cell.blocks, depth + 1)?,
                                                span: cell.span,
                                            })
                                        })
                                        .collect::<Result<_, LiveError>>()?,
                                    span: row.span,
                                })
                            })
                            .collect::<Result<_, LiveError>>()?,
                        span,
                    },
                    Block::Callout {
                        kind,
                        attributes,
                        blocks,
                        span,
                    } => FragmentBlock::Callout {
                        kind,
                        attributes,
                        blocks: self.blocks(blocks, depth + 1)?,
                        span,
                    },
                    Block::Unsupported {
                        source_kind,
                        raw,
                        span,
                    } => FragmentBlock::Unsupported {
                        source_kind,
                        raw,
                        span,
                    },
                    Block::CodeCell(_) => {
                        return Err(LiveError::Policy(MarkdownRejectionReason::Structure));
                    }
                })
            })
            .collect()
    }
    fn inlines(
        &mut self,
        nodes: Vec<Inline>,
        depth: usize,
    ) -> Result<Vec<FragmentInline>, LiveError> {
        if depth > 128 {
            return Err(LiveError::Policy(MarkdownRejectionReason::Structure));
        }
        nodes
            .into_iter()
            .map(|node| {
                Ok(match node {
                    Inline::Text { value, span } => FragmentInline::Text { value, span },
                    Inline::Space { span } => FragmentInline::Space { span },
                    Inline::SoftBreak { span } => FragmentInline::SoftBreak { span },
                    Inline::HardBreak { span } => FragmentInline::HardBreak { span },
                    Inline::NonbreakingSpace { span } => FragmentInline::NonbreakingSpace { span },
                    Inline::Emphasis { inlines, span } => FragmentInline::Emphasis {
                        inlines: self.inlines(inlines, depth + 1)?,
                        span,
                    },
                    Inline::Strong { inlines, span } => FragmentInline::Strong {
                        inlines: self.inlines(inlines, depth + 1)?,
                        span,
                    },
                    Inline::Strikeout { inlines, span } => FragmentInline::Strikeout {
                        inlines: self.inlines(inlines, depth + 1)?,
                        span,
                    },
                    Inline::Code { value, span } => FragmentInline::Code { value, span },
                    Inline::Link {
                        inlines,
                        target,
                        title,
                        attributes,
                        span,
                    } => FragmentInline::Link {
                        inlines: self.inlines(inlines, depth + 1)?,
                        target,
                        title,
                        attributes,
                        span,
                    },
                    Inline::Image {
                        alt,
                        target,
                        title,
                        attributes,
                        span,
                    } => FragmentInline::Image {
                        alt: self.inlines(alt, depth + 1)?,
                        asset: self.image(&target)?,
                        title,
                        attributes,
                        span,
                    },
                    Inline::AutoLink { target, span } => FragmentInline::AutoLink { target, span },
                    Inline::SemanticReference {
                        target,
                        target_span,
                        span,
                    } => FragmentInline::SemanticReference {
                        target,
                        target_span,
                        span,
                    },
                    Inline::Unsupported {
                        source_kind,
                        raw,
                        span,
                    } => FragmentInline::Unsupported {
                        source_kind,
                        raw,
                        span,
                    },
                })
            })
            .collect()
    }
}

impl Check<'_> {
    fn blocks(
        &mut self,
        nodes: &[FragmentBlock],
        parent: SourceSpan,
        edge: NodeEdge,
        depth: usize,
    ) -> Result<Vec<Block>, RestoreRejection> {
        if depth > 128 {
            return Err(RestoreRejection::Structure);
        }
        let mut output = Vec::new();
        let mut previous_start = parent.start;
        for (index, node) in nodes.iter().enumerate() {
            self.address.push((edge, index));
            let span = match node {
                FragmentBlock::Paragraph { span, .. } => *span,
                FragmentBlock::Heading { span, .. } => *span,
                FragmentBlock::BlockQuote { span, .. } => *span,
                FragmentBlock::List { span, .. } => *span,
                FragmentBlock::ThematicBreak { span, .. } => *span,
                FragmentBlock::CodeBlock { span, .. } => *span,
                FragmentBlock::Table { span, .. } => *span,
                FragmentBlock::Callout { span, .. } => *span,
                FragmentBlock::Unsupported { span, .. } => *span,
            };
            span_in(span, parent)?;
            if span.start < previous_start {
                return Err(RestoreRejection::Structure);
            }
            previous_start = span.start;
            output.push(match node {
                FragmentBlock::Paragraph { inlines, .. } => Block::Paragraph {
                    inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                    span,
                },
                FragmentBlock::Heading {
                    level,
                    attributes,
                    inlines,
                    ..
                } => {
                    check_attributes(attributes)?;
                    if !(1..=6).contains(level) {
                        return Err(RestoreRejection::Structure);
                    }
                    Block::Heading {
                        level: *level,
                        attributes: attributes.clone(),
                        inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                        span,
                    }
                }
                FragmentBlock::BlockQuote { blocks, .. } => Block::BlockQuote {
                    blocks: self.blocks(blocks, span, NodeEdge::Blocks, depth + 1)?,
                    span,
                },
                FragmentBlock::List { ordered, items, .. } => {
                    let mut list = Vec::new();
                    let mut previous = span.start;
                    for (index, item) in items.iter().enumerate() {
                        span_in(item.span, span)?;
                        if item.span.start < previous {
                            return Err(RestoreRejection::Structure);
                        }
                        previous = item.span.start;
                        self.address.push((NodeEdge::Items, index));
                        list.push(ir::ListItem {
                            checked: item.checked,
                            blocks: self.blocks(
                                &item.blocks,
                                item.span,
                                NodeEdge::Blocks,
                                depth + 1,
                            )?,
                            span: item.span,
                        });
                        self.address.pop();
                    }
                    Block::List {
                        ordered: *ordered,
                        items: list,
                        span,
                    }
                }
                FragmentBlock::ThematicBreak { .. } => Block::ThematicBreak { span },
                FragmentBlock::CodeBlock {
                    language,
                    source,
                    source_segments,
                    ..
                } => {
                    let mut previous_end = span.start;
                    let mut reconstructed = String::new();
                    for segment in source_segments {
                        span_in(segment.span, span)?;
                        if segment.span.start < previous_end
                            || segment.text.len() != segment.span.end - segment.span.start
                        {
                            return Err(RestoreRejection::Structure);
                        }
                        previous_end = segment.span.end;
                        reconstructed.push_str(&segment.text);
                    }
                    if reconstructed != *source {
                        return Err(RestoreRejection::Structure);
                    }
                    Block::CodeBlock {
                        language: language.clone(),
                        source: source.clone(),
                        source_segments: source_segments.clone(),
                        span,
                    }
                }
                FragmentBlock::Table {
                    caption,
                    alignments,
                    rows,
                    ..
                } => {
                    let caption = self.inlines(caption, span, NodeEdge::Caption, depth + 1)?;
                    let mut table = Vec::new();
                    let mut previous_row = span.start;
                    for (row_index, row) in rows.iter().enumerate() {
                        span_in(row.span, span)?;
                        if row.span.start < previous_row {
                            return Err(RestoreRejection::Structure);
                        }
                        previous_row = row.span.start;
                        self.address.push((NodeEdge::Rows, row_index));
                        let mut cells = Vec::new();
                        let mut previous_cell = row.span.start;
                        for (cell_index, cell) in row.cells.iter().enumerate() {
                            span_in(cell.span, row.span)?;
                            if cell.span.start < previous_cell {
                                return Err(RestoreRejection::Structure);
                            }
                            previous_cell = cell.span.start;
                            self.address.push((NodeEdge::Cells, cell_index));
                            cells.push(ir::TableCell {
                                blocks: self.blocks(
                                    &cell.blocks,
                                    cell.span,
                                    NodeEdge::Blocks,
                                    depth + 1,
                                )?,
                                span: cell.span,
                            });
                            self.address.pop();
                        }
                        table.push(ir::TableRow {
                            header: row.header,
                            cells,
                            span: row.span,
                        });
                        self.address.pop();
                    }
                    Block::Table {
                        caption,
                        alignments: alignments.clone(),
                        rows: table,
                        span,
                    }
                }
                FragmentBlock::Callout {
                    kind,
                    attributes,
                    blocks,
                    ..
                } => {
                    check_attributes(attributes)?;
                    Block::Callout {
                        kind: *kind,
                        attributes: attributes.clone(),
                        blocks: self.blocks(blocks, span, NodeEdge::Blocks, depth + 1)?,
                        span,
                    }
                }
                FragmentBlock::Unsupported {
                    source_kind, raw, ..
                } => {
                    if source_kind.is_empty() || raw.len() != span.end - span.start {
                        return Err(RestoreRejection::Structure);
                    }
                    self.unsupported.push((span, source_kind.clone()));
                    Block::Unsupported {
                        source_kind: source_kind.clone(),
                        raw: raw.clone(),
                        span,
                    }
                }
            });
            self.address.pop();
        }
        Ok(output)
    }
    fn inlines(
        &mut self,
        nodes: &[FragmentInline],
        parent: SourceSpan,
        edge: NodeEdge,
        depth: usize,
    ) -> Result<Vec<Inline>, RestoreRejection> {
        if depth > 128 {
            return Err(RestoreRejection::Structure);
        }
        let mut output = Vec::new();
        let mut previous_start = parent.start;
        for (index, node) in nodes.iter().enumerate() {
            self.address.push((edge, index));
            let span = match node {
                FragmentInline::Text { span, .. } => *span,
                FragmentInline::Space { span, .. } => *span,
                FragmentInline::SoftBreak { span, .. } => *span,
                FragmentInline::HardBreak { span, .. } => *span,
                FragmentInline::NonbreakingSpace { span, .. } => *span,
                FragmentInline::Emphasis { span, .. } => *span,
                FragmentInline::Strong { span, .. } => *span,
                FragmentInline::Strikeout { span, .. } => *span,
                FragmentInline::Code { span, .. } => *span,
                FragmentInline::Link { span, .. } => *span,
                FragmentInline::Image { span, .. } => *span,
                FragmentInline::AutoLink { span, .. } => *span,
                FragmentInline::SemanticReference { span, .. } => *span,
                FragmentInline::Unsupported { span, .. } => *span,
            };
            span_in(span, parent)?;
            if span.start < previous_start {
                return Err(RestoreRejection::Structure);
            }
            previous_start = span.start;
            output.push(match node {
                FragmentInline::Text { value, .. } => Inline::Text {
                    value: value.clone(),
                    span,
                },
                FragmentInline::Space { .. } => Inline::Space { span },
                FragmentInline::SoftBreak { .. } => Inline::SoftBreak { span },
                FragmentInline::HardBreak { .. } => Inline::HardBreak { span },
                FragmentInline::NonbreakingSpace { .. } => Inline::NonbreakingSpace { span },
                FragmentInline::Emphasis { inlines, .. } => Inline::Emphasis {
                    inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                    span,
                },
                FragmentInline::Strong { inlines, .. } => Inline::Strong {
                    inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                    span,
                },
                FragmentInline::Strikeout { inlines, .. } => Inline::Strikeout {
                    inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                    span,
                },
                FragmentInline::Code { value, .. } => Inline::Code {
                    value: value.clone(),
                    span,
                },
                FragmentInline::Link {
                    inlines,
                    target,
                    title,
                    attributes,
                    ..
                } => {
                    check_attributes(attributes)?;
                    urls::link(target, self.context).map_err(|_| RestoreRejection::Url)?;
                    Inline::Link {
                        inlines: self.inlines(inlines, span, NodeEdge::Inlines, depth + 1)?,
                        target: target.clone(),
                        title: title.clone(),
                        attributes: attributes.clone(),
                        span,
                    }
                }
                FragmentInline::Image {
                    alt,
                    asset,
                    title,
                    attributes,
                    ..
                } => {
                    check_attributes(attributes)?;
                    let image = self.assets.resolve(asset, self.context)?;
                    self.bindings.push(ImageBinding {
                        address: self.address.clone(),
                        asset: image.clone(),
                    });
                    Inline::Image {
                        alt: self.inlines(alt, span, NodeEdge::Alt, depth + 1)?,
                        target: image.reference.path.as_str().into(),
                        title: title.clone(),
                        attributes: attributes.clone(),
                        span,
                    }
                }
                FragmentInline::AutoLink { target, .. } => {
                    urls::link(target, self.context).map_err(|_| RestoreRejection::Url)?;
                    Inline::AutoLink {
                        target: target.clone(),
                        span,
                    }
                }
                FragmentInline::SemanticReference {
                    target,
                    target_span,
                    ..
                } => {
                    span_in(*target_span, span)?;
                    Inline::SemanticReference {
                        target: target.clone(),
                        target_span: *target_span,
                        span,
                    }
                }
                FragmentInline::Unsupported {
                    source_kind, raw, ..
                } => {
                    if source_kind.is_empty() || raw.len() != span.end - span.start {
                        return Err(RestoreRejection::Structure);
                    }
                    self.unsupported.push((span, source_kind.clone()));
                    Inline::Unsupported {
                        source_kind: source_kind.clone(),
                        raw: raw.clone(),
                        span,
                    }
                }
            });
            self.address.pop();
        }
        Ok(output)
    }
}
