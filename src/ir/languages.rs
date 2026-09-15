//! Typed static API semantics. Signatures, documents, and evidence remain shared.

use serde::{Deserialize, Serialize};

use super::{ItemReference, SignatureExpression, SourceEvidence};

/// Language-specific facts for one canonical item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "language",
    content = "data",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum ItemLanguageData {
    /// Python declaration semantics after source/stub reconciliation.
    Python(PythonItemData),
    /// R declaration semantics after namespace/source/Rd reconciliation.
    R(RItemData),
}

/// An additional lookup name for its owning canonical item, never another item.
///
/// A Python type alias is a distinct declaration represented by
/// [`PythonDeclaration::TypeAlias`], not by this record. Rd aliases must be
/// apportioned to their actual generic or method, even when they share a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemAlias {
    /// Package-scoped lookup spelling, including Python module qualification.
    pub qualified_name: String,
    /// Semantics of the declaration that introduced the name.
    pub kind: ItemAliasKind,
    /// Re-export, assignment, or Rd evidence in source order.
    pub sources: Vec<SourceEvidence>,
}

/// Why an additional name refers to an existing item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemAliasKind {
    /// A statically resolved import or re-export.
    PythonReexport,
    /// A statically proven assignment of the same object.
    PythonAssignment,
    /// An Rd lookup name reconciled with a maintained declaration.
    RdAlias,
}

/// Python facts independent of an item's structured signature and prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonItemData {
    /// Visibility established by the static export policy.
    pub visibility: PythonVisibility,
    /// Supported declaration semantics.
    pub declaration: PythonDeclaration,
    /// Decorators in source order, retained without evaluation.
    pub decorators: Vec<PythonDecorator>,
}

/// Visibility is unknown when dynamic exports prevent a static decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonVisibility {
    /// Statically public API.
    Public,
    /// Retained private implementation detail.
    Private,
    /// Retained declaration whose public visibility is not established.
    Unknown,
}

/// The supported static Python declaration shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PythonDeclaration {
    /// One module, possibly reconciled from both source and a stub.
    Module {
        /// Which maintained declarations contribute to this module.
        source: PythonModuleSource,
        /// Export authority, including an explicitly unknown dynamic surface.
        exports: PythonExports,
    },
    /// A function, method, constructor, or individually addressable overload.
    Callable {
        /// Binding and constructor behavior.
        binding: PythonCallableKind,
        /// Whether the declaration uses `async def`.
        is_async: bool,
        /// Family membership, separate from lexical child containment.
        role: PythonCallableRole,
    },
    /// A class with supported explicit or generated constructor semantics.
    Class {
        /// Base expressions in declaration order, never evaluated.
        bases: Vec<SignatureExpression>,
        /// Constructor facts that can be established statically.
        constructor: PythonConstructor,
    },
    /// A property whose getter signature remains on the owning item.
    Property {
        /// Whether a supported setter declaration is present.
        has_setter: bool,
        /// Whether a supported deleter declaration is present.
        has_deleter: bool,
    },
    /// An annotated or literal constant; syntax remains in `Item::signatures`.
    Constant,
    /// An instance or class field; syntax remains in `Item::signatures`.
    Field,
    /// A distinct named type alias, rather than another name for one item.
    TypeAlias {
        /// Unevaluated aliased type expression.
        target: SignatureExpression,
    },
}

/// Origin of the reconciled module surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonModuleSource {
    /// Maintained `.py` declarations.
    Implementation,
    /// Maintained `.pyi` declarations with no implementation required.
    StubOnly,
    /// Maintained `.py` and `.pyi` declarations reconciled field by field.
    ImplementationAndStub,
}

/// Static export authority; names are retained in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PythonExports {
    /// Public definitions and explicitly public imports determine the surface.
    Implicit,
    /// A supported literal `__all__` is authoritative, including an empty list.
    Explicit {
        /// Exported local names in declaration order.
        names: Vec<String>,
        /// Evidence for the declaration, separately from definitions.
        sources: Vec<SourceEvidence>,
    },
    /// A computed export expression must be accompanied by a diagnostic.
    Dynamic {
        /// Unevaluated expression, never guessed into an export list.
        expression: SignatureExpression,
        /// Evidence for the unsupported export declaration.
        sources: Vec<SourceEvidence>,
    },
}

/// Python callable binding semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonCallableKind {
    /// Module-level function.
    Function,
    /// Method bound to an instance.
    InstanceMethod,
    /// Method with no implicit receiver.
    StaticMethod,
    /// Method bound to a class.
    ClassMethod,
    /// Explicit `__init__` or `__new__` declaration.
    Constructor,
}

/// Every callable has one family item; overloads are separate member items.
///
/// A family with no overloads is an ordinary callable. Its signatures and
/// documentation remain on `Item`. An overloaded family holds the reconciled
/// public signatures; each overload also holds its own signature and evidence.
/// Stub and implementation declarations do not create extra family items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PythonCallableRole {
    /// Canonical public entry point and concept target.
    Family {
        /// Addressable overloads in declaration order.
        overloads: Vec<ItemReference>,
    },
    /// An individual overload declaration.
    Overload {
        /// Canonical callable family.
        family: ItemReference,
    },
}

/// Constructor semantics supported without executing class decorators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PythonConstructor {
    /// No constructor semantics have been established by the extractor.
    Unspecified,
    /// A maintained constructor declaration.
    Explicit {
        /// Constructor callable family.
        item: ItemReference,
    },
    /// Supported dataclass options, after applying known decorator defaults.
    Dataclass {
        /// Fields in declaration order.
        fields: Vec<ItemReference>,
        /// Whether the decorator generates `__init__`.
        init: bool,
        /// Whether the class is frozen.
        frozen: bool,
    },
}

/// Decorator syntax and any statically established meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonDecorator {
    /// Structured unevaluated syntax, including call arguments.
    pub expression: SignatureExpression,
    /// Meaning established by the supported static subset.
    pub semantics: PythonDecoratorSemantics,
    /// Exact decorator evidence when available.
    pub sources: Vec<SourceEvidence>,
}

/// Recognizing a decorator never authorizes evaluating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PythonDecoratorSemantics {
    /// `typing.overload`.
    Overload,
    /// A property getter.
    Property,
    /// A property setter.
    PropertySetter,
    /// A property deleter.
    PropertyDeleter,
    /// `staticmethod`.
    StaticMethod,
    /// `classmethod`.
    ClassMethod,
    /// Supported `dataclasses.dataclass` options.
    Dataclass,
    /// Syntax retained without claiming its runtime semantics.
    Unknown,
}

/// R facts independent of shared formals, documentation, and source evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RItemData {
    /// Whether `NAMESPACE` explicitly exports the declaration by name.
    /// A registered S3 method can be public through dispatch without this flag.
    pub exported: bool,
    /// Supported maintained-source and namespace semantics.
    pub declaration: RDeclaration,
}

/// Static R declarations supported by the MVP corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RDeclaration {
    /// An ordinary named function.
    Function,
    /// An S3 generic is itself the canonical callable family.
    S3Generic {
        /// Name passed to `UseMethod`, which need not equal the binding name.
        dispatch_name: String,
        /// Explicit second `UseMethod` argument when supplied.
        dispatch_object: Option<SignatureExpression>,
        /// Registered method identities in deterministic declaration order.
        methods: Vec<ItemReference>,
    },
    /// One addressable S3 method, independent of its Rd topic and alias names.
    S3Method {
        /// Resolved local or externally imported generic.
        generic: RGenericReference,
        /// Dispatch class, including the special `default` class.
        class: String,
        /// Namespace registration evidence, separate from the definition.
        registration: Vec<SourceEvidence>,
    },
    /// A function that statically constructs instances of known S3 classes.
    Constructor {
        /// Class vector in declared dispatch order.
        classes: Vec<String>,
    },
}

/// Generic identity without inventing an item for an unsupplied R package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RGenericReference {
    /// Generic represented in this workspace, possibly in another package.
    Workspace {
        /// Canonical generic identity.
        item: ItemReference,
    },
    /// Generic imported from an R package outside the supplied workspace.
    External {
        /// R package name, such as `stats`.
        package: String,
        /// Generic name, such as `predict`.
        name: String,
    },
}
