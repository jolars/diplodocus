//! Portable intermediate representation for extracted documentation.

use serde::{Deserialize, Serialize};

/// A zero-based, half-open byte range in one authored source document.
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
    /// Optional QMD frontmatter.
    pub frontmatter: Option<MetadataValue>,
    /// Renderable blocks in source order.
    pub blocks: Vec<Block>,
}

/// A block in an authored document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Block {
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
    /// Retained syntax outside Polydoc's supported authored profile.
    Unsupported {
        /// Panache syntax-kind name.
        source_kind: String,
        /// Original source bytes.
        raw: String,
        /// Full source range.
        span: SourceSpan,
    },
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
    /// Polydoc's code-only unresolved-reference extension.
    SemanticReference {
        /// Package-qualified or unqualified semantic target.
        target: String,
        /// Full bracketed source range.
        span: SourceSpan,
        /// Range of the code payload.
        target_span: SourceSpan,
    },
    /// Retained inline syntax outside the supported profile.
    Unsupported {
        /// Panache syntax-kind name.
        source_kind: String,
        /// Original source bytes.
        raw: String,
        /// Full source range.
        span: SourceSpan,
    },
}
