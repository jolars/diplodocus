//! Storage-independent, untrusted generated Markdown trees.
use super::AssetUse;
use crate::ir::{Attributes, CalloutKind, SourceSegment, SourceSpan, TableAlignment};

/// Decoded canonical Markdown content; construction grants no rendering trust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedMarkdown {
    /// Inert output blocks with typed, still unverified image uses.
    pub blocks: Vec<FragmentBlock>,
}

/// A block in an authored document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentBlock {
    /// Paragraph content.
    Paragraph {
        /// FragmentInline children.
        inlines: Vec<FragmentInline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Heading content.
    Heading {
        /// Heading level from one through six.
        level: usize,
        /// Heading attributes.
        attributes: Attributes,
        /// FragmentInline heading children.
        inlines: Vec<FragmentInline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Nested quotation.
    BlockQuote {
        /// Nested blocks.
        blocks: Vec<FragmentBlock>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Ordered, unordered, or task list.
    List {
        /// Whether the list uses ordered markers.
        ordered: bool,
        /// List items.
        items: Vec<FragmentListItem>,
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
    /// Pipe table.
    Table {
        /// Optional caption.
        caption: Vec<FragmentInline>,
        /// Column alignments.
        alignments: Vec<TableAlignment>,
        /// Header and body rows.
        rows: Vec<FragmentTableRow>,
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
        blocks: Vec<FragmentBlock>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Retained syntax outside Diplodocus's supported authored profile.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentListItem {
    /// Task state when the item has a checkbox.
    pub checked: Option<bool>,
    /// Nested block content.
    pub blocks: Vec<FragmentBlock>,
    /// Full source range.
    pub span: SourceSpan,
}

/// One table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentTableRow {
    /// Whether this is a header row.
    pub header: bool,
    /// Cells in source order.
    pub cells: Vec<FragmentTableCell>,
    /// Full source range.
    pub span: SourceSpan,
}

/// One table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentTableCell {
    /// Cell content as blocks.
    pub blocks: Vec<FragmentBlock>,
    /// Full source range.
    pub span: SourceSpan,
}

/// FragmentInline authored content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentInline {
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
        inlines: Vec<FragmentInline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Strongly emphasized content.
    Strong {
        /// Nested content.
        inlines: Vec<FragmentInline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// Struck-out content.
    Strikeout {
        /// Nested content.
        inlines: Vec<FragmentInline>,
        /// Full source range.
        span: SourceSpan,
    },
    /// FragmentInline code.
    Code {
        /// Literal code content.
        value: String,
        /// Full source range.
        span: SourceSpan,
    },
    /// Link.
    Link {
        /// Link label content.
        inlines: Vec<FragmentInline>,
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
        alt: Vec<FragmentInline>,
        /// Untrusted image metadata, verified by the active restore constructor.
        asset: AssetUse,
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
        /// Panache syntax-kind name.
        source_kind: String,
        /// Original source bytes.
        raw: String,
        /// Full source range.
        span: SourceSpan,
    },
}
