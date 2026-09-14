//! Structured signatures shared by extractors, renderers, and search.

use serde::{Deserialize, Serialize};

use super::{ItemReference, SourceEvidence};

/// A signature with its own evidence, which may come from a stub rather than
/// the source of the owning item's definition or documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcedSignature {
    /// Structured signature syntax.
    pub signature: Signature,
    /// Evidence in producer-defined source order.
    pub sources: Vec<SourceEvidence>,
}

/// A declaration's semantic signature, independent of display formatting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Signature {
    /// A callable's parameters and return annotation.
    Callable {
        /// Parameters in declaration order, including variadic parameters.
        parameters: Vec<Parameter>,
        /// Return annotation when declared.
        returns: Option<SignatureExpression>,
    },
    /// A field, constant, or other value declaration.
    Value {
        /// Declared type annotation.
        annotation: Option<SignatureExpression>,
        /// Declared value, retained without evaluating it.
        value: Option<SignatureExpression>,
    },
    /// Syntax without a language-neutral declaration shape.
    LanguageSpecific {
        /// Language defining the syntax.
        language: String,
        /// Structured syntax, not a formatted declaration string.
        syntax: SignatureExpression,
    },
}

/// A parameter with its annotation and unevaluated default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    /// Declared parameter name.
    pub name: String,
    /// Calling convention of this parameter.
    pub kind: ParameterKind,
    /// Annotation when declared.
    pub annotation: Option<SignatureExpression>,
    /// Default expression; `None` distinguishes a required parameter from a
    /// default whose literal spelling is `None`, `NULL`, or another null value.
    pub default: Option<SignatureExpression>,
}

/// How a parameter accepts arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ParameterKind {
    /// Accepts only a positional argument.
    PositionalOnly,
    /// Accepts a positional or named argument.
    PositionalOrKeyword,
    /// Accepts only a named argument.
    KeywordOnly,
    /// Collects additional positional arguments.
    VariadicPositional,
    /// Collects additional named arguments.
    VariadicKeyword,
    /// A language-specific calling convention.
    LanguageSpecific {
        /// Language defining the convention.
        language: String,
        /// Stable language-specific convention identifier.
        name: String,
    },
}

/// Recursive syntax used by annotations, defaults, and language-specific forms.
///
/// Expressions preserve semantic children. Literal or fallback source spelling
/// is never evaluated, and consumers need not reparse a display signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SignatureExpression {
    /// An identifier with an optional resolved semantic target.
    Name {
        /// Declared spelling.
        name: String,
        /// Package-scoped item identity when resolved.
        target: Option<ItemReference>,
    },
    /// A literal preserving its language's spelling.
    Literal {
        /// Literal source, such as `True`, `NULL`, or `"text"`.
        text: String,
    },
    /// A type application or call-like expression.
    Apply {
        /// Applied expression, such as `list` in `list[int]`.
        constructor: Box<SignatureExpression>,
        /// Arguments in declaration order.
        arguments: Vec<SignatureExpression>,
    },
    /// An ordered tuple or sequence of expressions.
    Sequence {
        /// Members in declaration order.
        items: Vec<SignatureExpression>,
    },
    /// A language-specific syntax node with semantic children.
    LanguageSpecific {
        /// Language defining this syntax.
        language: String,
        /// Stable node identifier, such as `union` or `formula`.
        name: String,
        /// Children in syntax order.
        children: Vec<SignatureExpression>,
        /// Optional original spelling for diagnostics or unsupported syntax.
        source: Option<String>,
    },
}
