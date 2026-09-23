//! Typed execution warnings; messages are a projection, never an encoding.
use super::{AuthoredOutputContext, FragmentIdentity, OutputOrigin};
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::ir::SourceSpan;

/// Portable attribution in either authored-page or generated-fragment coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticAttribution {
    /// Authored page for page/cell diagnostics, absent for generated fragments.
    pub source: Option<DiagnosticSource>,
    /// Producing cell when known.
    pub cell: Option<usize>,
    /// Producer slot when known.
    pub slot: Option<usize>,
    /// Generated fragment identity when present.
    pub fragment: Option<FragmentIdentity>,
    /// Primary range in the indicated coordinate space.
    pub span: Option<SourceSpan>,
    /// Additional ranges in that same coordinate space.
    pub related_spans: Vec<SourceSpan>,
}
impl DiagnosticAttribution {
    /// Attribute a warning to its producing cell or fragment.
    pub fn output(
        context: &AuthoredOutputContext,
        origin: &OutputOrigin,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            source: origin
                .fragment
                .is_none()
                .then(|| context.diagnostic_source()),
            cell: Some(origin.cell),
            slot: Some(origin.slot),
            fragment: origin.fragment,
            span: if origin.fragment.is_some() {
                span
            } else {
                Some(origin.cell_span)
            },
            related_spans: Vec::new(),
        }
    }
}

/// Why an HTML candidate failed the active policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlRejectionReason {
    /// Unsupported element or namespace.
    Element,
    /// Unsupported attribute or value.
    Attribute,
    /// Forbidden or malformed URL.
    Url,
    /// Images cannot fetch external resources.
    RemoteImage,
    /// Malformed or unsupported structure.
    Structure,
}
/// Why a Markdown candidate failed the active policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownRejectionReason {
    /// Forbidden or malformed URL.
    Url,
    /// Images cannot fetch external resources.
    RemoteImage,
    /// Invalid inert tree or attribution.
    Structure,
}

/// Closed producing-warning catalog shared by live output and cache projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionDiagnostic {
    /// The kernel sent an unsupported message.
    KernelMessageIgnored {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
    },
    /// The display update has no surviving target.
    UnknownDisplayUpdate {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
    },
    /// No supported output representation was accepted.
    NoSupportedRepresentation {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Offered MIME types in lexical order.
        mime_types: Vec<String>,
    },
    /// The output media type is unsupported.
    UnsupportedMedia {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Offered MIME type.
        media_type: String,
    },
    /// The output MIME bundle is malformed.
    InvalidMimeBundle {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
    },
    /// The text output payload is invalid.
    InvalidTextPayload {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Offered text MIME type.
        media_type: String,
    },
    /// The output image is invalid.
    InvalidImage {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Offered image MIME type.
        media_type: String,
    },
    /// The SVG output violates the inert image policy.
    SvgRejected {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
    },
    /// The HTML output violates the inert HTML policy.
    HtmlRejected {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Policy rejection category.
        reason: HtmlRejectionReason,
    },
    /// The Markdown output violates the generated-content policy.
    MarkdownRejected {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Policy rejection category.
        reason: MarkdownRejectionReason,
    },
    /// Unsupported generated Markdown syntax is retained as escaped text.
    FragmentUnsupported {
        /// Producing location and ranges.
        attribution: DiagnosticAttribution,
        /// Original parser node kind, never parsed from a message.
        source_kind: String,
    },
}
impl ExecutionDiagnostic {
    /// Stable warning code.
    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::KernelMessageIgnored { .. } => DiagnosticCode::UnsupportedKernelMessage,
            Self::UnknownDisplayUpdate { .. } => DiagnosticCode::UnsupportedCellOutput,
            Self::NoSupportedRepresentation { .. } => DiagnosticCode::UnsupportedCellOutput,
            Self::UnsupportedMedia { .. } => DiagnosticCode::UnsupportedCellOutput,
            Self::InvalidMimeBundle { .. } => DiagnosticCode::InvalidCellOutput,
            Self::InvalidTextPayload { .. } => DiagnosticCode::InvalidCellOutput,
            Self::InvalidImage { .. } => DiagnosticCode::InvalidCellOutput,
            Self::SvgRejected { .. } => DiagnosticCode::UnsafeKernelSvg,
            Self::HtmlRejected { .. } => DiagnosticCode::UnsafeKernelHtml,
            Self::MarkdownRejected { .. } => DiagnosticCode::InvalidCellOutput,
            Self::FragmentUnsupported { .. } => DiagnosticCode::UnsupportedAuthoredSyntax,
        }
    }
    /// Portable producing location.
    pub fn attribution(&self) -> &DiagnosticAttribution {
        match self {
            Self::KernelMessageIgnored { attribution, .. } => attribution,
            Self::UnknownDisplayUpdate { attribution, .. } => attribution,
            Self::NoSupportedRepresentation { attribution, .. } => attribution,
            Self::UnsupportedMedia { attribution, .. } => attribution,
            Self::InvalidMimeBundle { attribution, .. } => attribution,
            Self::InvalidTextPayload { attribution, .. } => attribution,
            Self::InvalidImage { attribution, .. } => attribution,
            Self::SvgRejected { attribution, .. } => attribution,
            Self::HtmlRejected { attribution, .. } => attribution,
            Self::MarkdownRejected { attribution, .. } => attribution,
            Self::FragmentUnsupported { attribution, .. } => attribution,
        }
    }
    /// Project the typed warning for display without parsing message strings.
    pub fn to_diagnostic(&self, collection: &str) -> Diagnostic {
        let message = match self {
            Self::KernelMessageIgnored { .. } => "The kernel sent an unsupported message.",
            Self::UnknownDisplayUpdate { .. } => "The display update has no surviving target.",
            Self::NoSupportedRepresentation { .. } => {
                "No supported output representation was accepted."
            }
            Self::UnsupportedMedia { .. } => "The output media type is unsupported.",
            Self::InvalidMimeBundle { .. } => "The output MIME bundle is malformed.",
            Self::InvalidTextPayload { .. } => "The text output payload is invalid.",
            Self::InvalidImage { .. } => "The output image is invalid.",
            Self::SvgRejected { .. } => "The SVG output violates the inert image policy.",
            Self::HtmlRejected { .. } => "The HTML output violates the inert HTML policy.",
            Self::MarkdownRejected { .. } => {
                "The Markdown output violates the generated-content policy."
            }
            Self::FragmentUnsupported { .. } => {
                "Unsupported generated Markdown syntax is retained as escaped text."
            }
        };
        let attribution = self.attribution();
        let mut diagnostic = Diagnostic::new(self.code(), Severity::Warning, message).with_entity(
            DiagnosticEntity::Content {
                id: collection.into(),
            },
        );
        diagnostic.source = attribution.source.clone();
        diagnostic.span = attribution.span;
        diagnostic.related_spans = attribution.related_spans.clone();
        diagnostic
    }
}
