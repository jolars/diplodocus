//! Portable C++ declaration semantics supplied by Doxygen.

use serde::{Deserialize, Serialize};

use super::{ItemReference, SignatureExpression};

/// C++ facts independent of signatures, documentation, and source evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CppItemData {
    /// Native declaration category.
    pub declaration: CppDeclarationKind,
    /// Containing type, enum, or callable family for an embedded declaration.
    pub owner: Option<ItemReference>,
    /// Callable family when this item represents an individual overload.
    pub family: Option<ItemReference>,
    /// Independently addressable overloads of a callable family.
    pub overloads: Vec<ItemReference>,
    /// Access reported by Doxygen, after applying extraction policy.
    pub protection: CppProtection,
    /// Base types in native declaration order.
    pub bases: Vec<SignatureExpression>,
}

/// Declaration categories that refine the shared API item classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CppDeclarationKind {
    /// Namespace.
    Namespace,
    /// Class.
    Class,
    /// Struct.
    Struct,
    /// Union.
    Union,
    /// Enumeration.
    Enum,
    /// Enumeration value.
    Enumerator,
    /// Free function.
    Function,
    /// Type member function.
    Method,
    /// Constructor.
    Constructor,
    /// Destructor.
    Destructor,
    /// Overloaded operator.
    Operator,
    /// Type member variable.
    Field,
    /// Namespace or file variable.
    Variable,
    /// Typedef or using declaration.
    TypeAlias,
    /// Documented preprocessor definition.
    Macro,
}

/// C++ access control, without inventing a visibility for file declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CppProtection {
    /// Public access, including namespace and file declarations.
    Public,
    /// Protected type member.
    Protected,
    /// Private type member explicitly requested by the Doxyfile.
    Private,
}
