//! Portable intermediate representation for extracted documentation.
//!
//! [`Workspace`] is the schema-versioned snapshot envelope. Map keys are the
//! authoritative entity IDs, with targets and items scoped to their package.
//! Maps and sets have deterministic order; vectors retain declaration, syntax,
//! or output-event order. [`SourcedDocument`] adds source context to the existing
//! [`Document`] tree, and signatures retain evidence independent of item prose.
//!
//! Paths name declared repositories and normalized relative locations, never
//! local checkout roots. Identity helpers assign canonical keys without extraction.
//! This model does not resolve source declarations, gather evidence,
//! or execute cells. Decoding output representations also grants no rendering
//! trust; serialized HTML remains an [`UnvalidatedHtml`] candidate.

use serde::{Deserialize, Serialize};

mod identity;
mod languages;
mod outputs;
mod package_metadata;
mod provenance;
mod signatures;
mod workspace;

pub use identity::*;
pub use languages::*;
pub use outputs::*;
pub use package_metadata::*;
pub use provenance::*;
pub use signatures::*;
pub use workspace::*;

/// A zero-based, half-open byte range in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// First byte included in the range.
    pub start: usize,
    /// First byte after the range.
    pub end: usize,
}

/// A string value and the source bytes that declared it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpannedString {
    /// Cooked semantic value.
    pub value: String,
    /// Source range of the value.
    pub span: SourceSpan,
}

/// Source-ordered attributes attached to a document construct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attributes {
    /// Optional identifier.
    pub identifier: Option<SpannedString>,
    /// Classes in source order.
    pub classes: Vec<SpannedString>,
    /// Key-value attributes in source order.
    pub key_values: Vec<AttributeKeyValue>,
}

/// One key-value attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeKeyValue {
    /// Attribute key.
    pub key: SpannedString,
    /// Attribute value.
    pub value: SpannedString,
}

/// Generic YAML value retained from document metadata or cell options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MetadataValue {
    /// An absent YAML value.
    Null {
        /// Source range of the empty value slot.
        span: SourceSpan,
    },
    /// A cooked scalar together with its raw spelling.
    Scalar {
        /// Raw scalar source.
        raw: String,
        /// Cooked scalar value.
        value: String,
        /// Source range of the scalar.
        span: SourceSpan,
    },
    /// A source-ordered mapping.
    Mapping {
        /// Mapping entries in source order, including duplicate keys.
        entries: Vec<MetadataEntry>,
        /// Source range of the mapping.
        span: SourceSpan,
    },
    /// A source-ordered sequence.
    Sequence {
        /// Sequence values in source order.
        items: Vec<MetadataValue>,
        /// Source range of the sequence.
        span: SourceSpan,
    },
    /// YAML syntax the portable model cannot interpret safely.
    Unsupported {
        /// Original source bytes.
        raw: String,
        /// Source range of the value.
        span: SourceSpan,
    },
}

/// One source-ordered metadata mapping entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// Cooked key and its range.
    pub key: SpannedString,
    /// Entry value.
    pub value: MetadataValue,
    /// Full declaration range.
    pub span: SourceSpan,
}

/// A parsed authored document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Full document range.
    pub span: SourceSpan,
    /// Optional authored-page YAML front matter.
    pub frontmatter: Option<MetadataValue>,
    /// Renderable blocks in source order.
    pub blocks: Vec<Block>,
}

/// A block in an authored document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Block {
    /// An explicitly declared browser component with ordinary Markdown fallback.
    Component {
        /// Component name and optional local anchor.
        attributes: Attributes,
        /// Content retained when scripting is unavailable.
        blocks: Vec<Block>,
        /// Range in the enclosing document.
        span: SourceSpan,
    },
    /// Heading-delimited panels presented as an interactive tabset in HTML.
    Tabset {
        /// Tabset identifier, classes, and optional synchronization group.
        attributes: Attributes,
        /// Tab headings and their content in source order.
        blocks: Vec<Block>,
        /// Range in the enclosing document.
        span: SourceSpan,
    },
    /// A semantic article container with an optional local anchor.
    Container {
        /// Presentation of the contained content.
        kind: DocumentContainerKind,
        /// Local anchor, when present.
        identifier: Option<String>,
        /// Contained document blocks.
        blocks: Vec<Block>,
        /// Range in the enclosing document.
        span: SourceSpan,
    },
    /// A display equation with an optional plain-text alternative.
    DisplayMath {
        /// Verbatim LaTeX source.
        latex: String,
        /// Author-supplied plain-text alternative.
        fallback: Option<String>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Paragraph content.
    Paragraph {
        /// Inline children.
        inlines: Vec<Inline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Heading content.
    Heading {
        /// Heading level from one through six.
        level: usize,
        /// Heading attributes.
        attributes: Attributes,
        /// Inline heading children.
        inlines: Vec<Inline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Nested quotation.
    BlockQuote {
        /// Nested blocks.
        blocks: Vec<Block>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Ordered, unordered, or task list.
    List {
        /// Whether the list uses ordered markers.
        ordered: bool,
        /// List items.
        items: Vec<ListItem>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Thematic break.
    ThematicBreak {
        /// Full source range.
        span: SourceSpan,
    },
    /// Non-executable fenced code.
    CodeBlock {
        /// Optional language identifier.
        language: Option<String>,
        /// Code with container prefixes removed.
        source: String,
        /// Source segments used to construct `source`.
        source_segments: Vec<SourceSegment>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Executable QMD cell, extracted but not executed.
    CodeCell(CodeCell),
    /// Pipe table.
    Table {
        /// Optional caption.
        caption: Vec<Inline>,
        /// Column alignments.
        alignments: Vec<TableAlignment>,
        /// Header and body rows.
        rows: Vec<TableRow>,
        /// Full source range.
        span: SourceSpan,
    },
    /// GFM alert or QMD callout.
    Callout {
        /// Callout category.
        kind: CalloutKind,
        /// Callout attributes.
        attributes: Attributes,
        /// Nested block content.
        blocks: Vec<Block>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Retained syntax outside Diplodocus's supported documentation profiles.
    Unsupported {
        /// Source syntax-kind name.
        source_kind: String,
        /// Original source fragment, or empty when exact node ranges are unavailable.
        /// The enclosing sourced document retains the complete input.
        raw: String,
        /// Full source range.
        span: SourceSpan,
    },
}

/// A term and its explanatory content before conversion to document blocks.
#[derive(Clone)]
pub(crate) struct DescriptionEntry {
    pub term: Vec<Block>,
    pub description: Vec<Block>,
    pub span: SourceSpan,
}

impl Block {
    pub(crate) fn description_list(entries: Vec<DescriptionEntry>, span: SourceSpan) -> Self {
        Self::Container {
            kind: DocumentContainerKind::DefinitionList,
            identifier: None,
            blocks: entries
                .into_iter()
                .flat_map(|entry| {
                    [
                        Self::Container {
                            kind: DocumentContainerKind::DefinitionTerm,
                            identifier: None,
                            blocks: entry.term,
                            span: entry.span,
                        },
                        Self::Container {
                            kind: DocumentContainerKind::Definition,
                            identifier: None,
                            blocks: entry.description,
                            span: entry.span,
                        },
                    ]
                })
                .collect(),
            span,
        }
    }
}

/// Semantic containers shared by extracted documentation and articles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentContainerKind {
    /// Terms and their definitions.
    DefinitionList,
    /// One definition-list term.
    DefinitionTerm,
    /// One definition-list body.
    Definition,
    /// An image or image group followed by caption content.
    Figure,
    /// The article's endnotes.
    Footnotes,
    /// One addressable endnote.
    Footnote,
    /// One addressable bibliography entry.
    BibliographyEntry,
    /// Inert content produced by one executable chunk.
    GeneratedChunk,
    /// Literal console output kept separate from its generating source.
    GeneratedOutput,
}

/// Inline formatting required by articles and citation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InlineStyle {
    /// Raised inline content.
    Superscript,
    /// Lowered inline content.
    Subscript,
    /// Small capital letters.
    SmallCaps,
}

/// One list item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    /// Task state when the item has a checkbox.
    pub checked: Option<bool>,
    /// Nested block content.
    pub blocks: Vec<Block>,
    /// Full source range.
    pub span: SourceSpan,
}

/// One source segment of code after container framing is removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSegment {
    /// Segment text.
    pub text: String,
    /// Exact range in the authored document.
    pub span: SourceSpan,
}

/// Extracted executable cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCell {
    /// Optional engine or language name.
    pub language: Option<String>,
    /// Optional fenced-cell identifier.
    pub identifier: Option<SpannedString>,
    /// Cell classes in source order.
    pub classes: Vec<SpannedString>,
    /// Cell labels in source order.
    pub labels: Vec<SpannedString>,
    /// Executable code with hashpipe and container framing removed.
    pub source: String,
    /// Exact source segments used to construct `source`.
    pub source_segments: Vec<SourceSegment>,
    /// Every inline and hashpipe option declaration.
    pub options: Vec<CellOption>,
    /// Resolutions for canonical option keys.
    pub resolved_options: Vec<ResolvedCellOption>,
    /// Execution outputs in event order; absent for unexecuted authored cells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<CellOutput>,
    /// Envelope around the executable source segments.
    pub code_span: Option<SourceSpan>,
    /// Full fenced-cell range.
    pub span: SourceSpan,
}

/// Origin of a cell-option declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellOptionSource {
    /// Fence info string.
    InlineInfo,
    /// Hashpipe YAML preamble.
    HashpipeYaml,
}

/// One cell-option declaration, including overridden declarations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellOption {
    /// Original key spelling.
    pub key: Option<String>,
    /// Canonical lower-case, hyphenated key.
    pub canonical_key: Option<String>,
    /// Raw value spelling when present.
    pub raw_value: Option<String>,
    /// Cooked scalar value when the value is scalar.
    pub cooked_value: Option<String>,
    /// Structured YAML value for hashpipe declarations.
    pub value: Option<MetadataValue>,
    /// Declaration origin.
    pub source: CellOptionSource,
    /// Key range.
    pub key_span: Option<SourceSpan>,
    /// Value range.
    pub value_span: Option<SourceSpan>,
    /// Full declaration range.
    pub span: SourceSpan,
}

/// Resolution of one canonical cell-option key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCellOption {
    /// Canonical key.
    pub key: String,
    /// Winning declaration or ambiguity.
    pub resolution: CellOptionResolution,
}

/// Whether cell-option precedence selected one declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CellOptionResolution {
    /// One declaration won precedence.
    Resolved {
        /// Index into [`CodeCell::options`].
        declaration: usize,
    },
    /// Multiple declarations occur in the winning precedence tier.
    Ambiguous {
        /// Indices into [`CodeCell::options`].
        declarations: Vec<usize>,
    },
}

/// Pipe-table column alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TableAlignment {
    /// Renderer default.
    Default,
    /// Left aligned.
    Left,
    /// Centered.
    Center,
    /// Right aligned.
    Right,
}

/// One table row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRow {
    /// Whether this is a header row.
    pub header: bool,
    /// Cells in source order.
    pub cells: Vec<TableCell>,
    /// Full source range.
    pub span: SourceSpan,
}

/// One table cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCell {
    /// Cell content as blocks.
    pub blocks: Vec<Block>,
    /// Full source range.
    pub span: SourceSpan,
}

/// Supported alert and callout category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalloutKind {
    /// Informational note.
    Note,
    /// Helpful tip.
    Tip,
    /// Important information.
    Important,
    /// Warning.
    Warning,
    /// Caution.
    Caution,
}

/// Inline authored content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Inline {
    /// Additional semantic inline formatting.
    Styled {
        /// Requested formatting.
        style: InlineStyle,
        /// Inline children.
        inlines: Vec<Inline>,
        /// Range in the enclosing document.
        span: SourceSpan,
    },
    /// A citation whose formatted children are resolved from the bibliography.
    Citation {
        /// Original citation spelling.
        raw: String,
        /// Referenced bibliography keys in source order.
        keys: Vec<String>,
        /// Formatted, linked citation content.
        inlines: Vec<Inline>,
        /// Range in the enclosing document.
        span: SourceSpan,
    },
    /// An inline equation with an optional plain-text alternative.
    Math {
        /// Verbatim LaTeX source.
        latex: String,
        /// Author-supplied plain-text alternative.
        fallback: Option<String>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Decoded text.
    Text {
        /// Decoded value.
        value: String,
        /// Source range.
        span: SourceSpan,
    },
    /// Collapsed whitespace.
    Space {
        /// Source range.
        span: SourceSpan,
    },
    /// Soft source line break.
    SoftBreak {
        /// Source range.
        span: SourceSpan,
    },
    /// Explicit hard line break.
    HardBreak {
        /// Source range.
        span: SourceSpan,
    },
    /// Nonbreaking space.
    NonbreakingSpace {
        /// Source range.
        span: SourceSpan,
    },
    /// Emphasized content.
    Emphasis {
        /// Nested content.
        inlines: Vec<Inline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Strongly emphasized content.
    Strong {
        /// Nested content.
        inlines: Vec<Inline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Struck-out content.
    Strikeout {
        /// Nested content.
        inlines: Vec<Inline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Inline code.
    Code {
        /// Literal code content.
        value: String,
        /// Full source range.
        span: SourceSpan,
    },
    /// Link.
    Link {
        /// Link label content.
        inlines: Vec<Inline>,
        /// Resolved target.
        target: String,
        /// Optional title.
        title: Option<String>,
        /// Link attributes.
        attributes: Attributes,
        /// Full source range.
        span: SourceSpan,
    },
    /// Image.
    Image {
        /// Alternative-text inlines.
        alt: Vec<Inline>,
        /// Resolved image target.
        target: String,
        /// Optional title.
        title: Option<String>,
        /// Image attributes.
        attributes: Attributes,
        /// Full source range.
        span: SourceSpan,
    },
    /// Automatic URL or email link.
    AutoLink {
        /// Target string.
        target: String,
        /// Full source range.
        span: SourceSpan,
    },
    /// Diplodocus's code-only unresolved-reference extension.
    SemanticReference {
        /// Package-qualified or unqualified semantic target.
        target: String,
        /// Full source range of the semantic reference.
        span: SourceSpan,
        /// Range of the code payload.
        target_span: SourceSpan,
    },
    /// Retained inline syntax outside the supported profile.
    Unsupported {
        /// Source syntax-kind name.
        source_kind: String,
        /// Original source fragment, or empty when exact node ranges are unavailable.
        /// The enclosing sourced document retains the complete input.
        raw: String,
        /// Full source range.
        span: SourceSpan,
    },
}
