//! Typed effective options; parsing and precedence resolution are separate.

use serde::{Deserialize, Serialize};

use crate::ir::SourceSpan;

/// An effective value and the declaration that supplied it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveOption<T> {
    /// Validated, normalized value.
    pub value: T,
    /// Winning declaration, with a page-relative UTF-8 range when authored.
    pub origin: OptionOrigin,
}

impl<T> EffectiveOption<T> {
    /// Record a policy default without inventing an authored source range.
    pub fn defaulted(value: T) -> Self {
        Self {
            value,
            origin: OptionOrigin::Default,
        }
    }
}

/// Origin of one effective option, independent of retained raw declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OptionOrigin {
    /// The active QMD policy supplies the value.
    Default,
    /// A document-level execution default supplies the value.
    Document {
        /// Full declaration range in the authored page.
        span: SourceSpan,
    },
    /// An inline fence option supplies the value.
    Inline {
        /// Full declaration range in the authored page.
        span: SourceSpan,
    },
    /// A hashpipe YAML option supplies the value.
    Hashpipe {
        /// Full declaration range in the authored page.
        span: SourceSpan,
    },
    /// A fence identifier supplies an otherwise absent cell label.
    FenceIdentifier {
        /// Identifier range in the authored page.
        span: SourceSpan,
    },
}

/// Presentation of collected output; hiding output never suppresses validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputVisibility {
    /// Present supported output with its ordinary MIME semantics.
    Show,
    /// Collect and validate output but omit it from presentation.
    Hide,
    /// Parse adjacent stdout runs as inert Markdown; retain other MIME semantics.
    AsIs,
}

/// The five options inherited from document execution defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionDefaults {
    /// Whether an eligible cell is submitted.
    pub eval: EffectiveOption<bool>,
    /// Whether cell source is presented in an authorized collection.
    pub echo: EffectiveOption<bool>,
    /// How collected output is presented.
    pub output: EffectiveOption<OutputVisibility>,
    /// Whether source and output are included in presentation.
    pub include: EffectiveOption<bool>,
    /// Whether a language exception permits later cells to run.
    pub error: EffectiveOption<bool>,
}

impl Default for ExecutionDefaults {
    fn default() -> Self {
        Self {
            eval: EffectiveOption::defaulted(true),
            echo: EffectiveOption::defaulted(true),
            output: EffectiveOption::defaulted(OutputVisibility::Show),
            include: EffectiveOption::defaulted(true),
            error: EffectiveOption::defaulted(false),
        }
    }
}

/// Complete effective cell options under `qmd-mvp-v1`.
///
/// Constructing or decoding these records does not validate author input or
/// resolve precedence. The preparer retains every original declaration in the
/// corresponding [`crate::ir::CodeCell`], including overridden declarations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveCellOptions {
    /// Effective inherited options after cell overrides.
    pub execution: ExecutionDefaults,
    /// Optional page-local cell anchor.
    pub label: EffectiveOption<Option<String>>,
    /// Optional plain-text alt text for selected figures.
    pub fig_alt: EffectiveOption<Option<String>>,
    /// Optional plain-text caption for the figure group.
    pub fig_cap: EffectiveOption<Option<String>>,
    /// Plain-text subcaptions in final figure order.
    pub fig_subcap: EffectiveOption<Vec<String>>,
}

impl Default for EffectiveCellOptions {
    fn default() -> Self {
        Self {
            execution: ExecutionDefaults::default(),
            label: EffectiveOption::defaulted(None),
            fig_alt: EffectiveOption::defaulted(None),
            fig_cap: EffectiveOption::defaulted(None),
            fig_subcap: EffectiveOption::defaulted(Vec::new()),
        }
    }
}
