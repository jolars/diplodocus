//! Versioned semantic keys for future static extractors.
//!
//! Keys encode canonical names and declaration roles, never presentation names,
//! routes, file paths, spans, discovery order, or overload ordinals. Extractors
//! reconcile stubs and implementations before registering one canonical item.
//! Re-export and Rd names bind to that item through [`IdentityRegistry`].
//!
//! Overload keys encode normalized signature structure, including parameter
//! names, conventions, annotations, defaults, and returns. Name resolution
//! targets and diagnostic source spellings are excluded. Producers must supply
//! canonical literal spellings and semantic language-specific children; opaque
//! source-only expressions cannot establish stable overload identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ItemReference, ParameterKind, RGenericReference, Signature, SignatureExpression};

/// Python entity roles that can share the same canonical qualified name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonIdentityKind {
    /// Module declaration.
    Module,
    /// Module-level callable family.
    Function,
    /// Method callable family.
    Method,
    /// Explicit constructor callable family.
    Constructor,
    /// Class declaration.
    Class,
    /// Property declaration, distinct from a method with the same name.
    Property,
    /// Constant declaration.
    Constant,
    /// Field declaration.
    Field,
    /// A distinct named type alias.
    TypeAlias,
}

impl PythonIdentityKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Function => "function",
            Self::Method => "method",
            Self::Constructor => "constructor",
            Self::Class => "class",
            Self::Property => "property",
            Self::Constant => "constant",
            Self::Field => "field",
            Self::TypeAlias => "type-alias",
        }
    }
}

/// An item-map key built under the `sid1` contract, before package qualification.
///
/// This is not a URL or a display string. The encoding is injective over the
/// supported normalized inputs, so it needs no hash-collision fallback.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticIdentity(String);

impl SemanticIdentity {
    /// Build the canonical Python declaration or callable-family key.
    ///
    /// Use the defining module and lexical containers, not a re-export name.
    /// Changing an ordinary callable's signature preserves its family identity.
    pub fn python(name: &str, kind: PythonIdentityKind) -> Result<Self, IdentityError> {
        nonempty("canonical-name", name)?;
        Ok(Self(format!("sid1:python:{}:{}", kind.tag(), atom(name))))
    }

    /// Build an individually addressable overload under a canonical family.
    ///
    /// Reordering declarations or changing source formatting does not change
    /// keys when the normalized signatures are unchanged. Equal normalized
    /// signatures intentionally have equal keys and must be reconciled or
    /// diagnosed as duplicates, never suffixed with a source-order counter.
    pub fn python_overload(
        name: &str,
        kind: PythonIdentityKind,
        signature: &Signature,
    ) -> Result<Self, IdentityError> {
        if !matches!(
            kind,
            PythonIdentityKind::Function
                | PythonIdentityKind::Method
                | PythonIdentityKind::Constructor
        ) {
            return Err(invalid(
                "python-kind",
                "overloads require a callable family",
            ));
        }
        let family = Self::python(name, kind)?;
        let Signature::Callable {
            parameters,
            returns,
        } = signature
        else {
            return Err(invalid(
                "signature",
                "overloads require a callable signature",
            ));
        };
        let mut key = format!("{}:overload:call(", family.0);
        for parameter in parameters {
            nonempty("parameter-name", &parameter.name)?;
            key.push_str("parameter(");
            key.push_str(&atom(&parameter.name));
            key.push(',');
            match &parameter.kind {
                ParameterKind::PositionalOnly => key.push_str("positional-only"),
                ParameterKind::PositionalOrKeyword => key.push_str("positional-or-keyword"),
                ParameterKind::KeywordOnly => key.push_str("keyword-only"),
                ParameterKind::VariadicPositional => key.push_str("variadic-positional"),
                ParameterKind::VariadicKeyword => key.push_str("variadic-keyword"),
                ParameterKind::LanguageSpecific { language, name } => {
                    write!(key, "language-specific({},{})", atom(language), atom(name)).unwrap();
                }
            }
            key.push(',');
            expression_option(&mut key, parameter.annotation.as_ref())?;
            key.push(',');
            expression_option(&mut key, parameter.default.as_ref())?;
            key.push_str(");");
        }
        key.push_str(")returns(");
        expression_option(&mut key, returns.as_ref())?;
        key.push(')');
        Ok(Self(key))
    }

    /// Build a maintained R function's key, including S3 constructor functions.
    pub fn r_function(name: &str) -> Result<Self, IdentityError> {
        Self::r_named(name, "function")
    }

    /// Build an S3 generic's key; the generic is also its callable family.
    pub fn r_s3_generic(name: &str) -> Result<Self, IdentityError> {
        Self::r_named(name, "s3-generic")
    }

    /// Build an S3 method key from its binding, generic identity, and class.
    ///
    /// An explicitly registered method need not be named `generic.class`.
    /// Generic qualification prevents identical binding/class names in distinct
    /// generic families from colliding. External generics remain coordinates.
    pub fn r_s3_method(
        name: &str,
        generic: &RGenericReference,
        class: &str,
    ) -> Result<Self, IdentityError> {
        let mut identity = Self::r_named(name, "s3-method")?;
        nonempty("dispatch-class", class)?;
        let generic = match generic {
            RGenericReference::Workspace { item } => {
                nonempty("generic-package", &item.package)?;
                nonempty("generic-item", &item.item)?;
                format!("workspace({},{})", atom(&item.package), atom(&item.item))
            }
            RGenericReference::External { package, name } => {
                nonempty("generic-package", package)?;
                nonempty("generic-name", name)?;
                format!("external({},{})", atom(package), atom(name))
            }
        };
        write!(identity.0, ":{generic}:{}", atom(class)).unwrap();
        Ok(identity)
    }

    fn r_named(name: &str, kind: &str) -> Result<Self, IdentityError> {
        nonempty("canonical-name", name)?;
        Ok(Self(format!("sid1:r:{kind}:{}", atom(name))))
    }

    /// Borrow the package-local item-map key.
    pub fn item_id(&self) -> &str {
        &self.0
    }

    /// Qualify this key with a stable workspace package ID, never its URL slug.
    pub fn in_package(&self, package: &str) -> Result<ItemReference, IdentityError> {
        nonempty("package", package)?;
        Ok(ItemReference {
            package: package.into(),
            item: self.0.clone(),
        })
    }
}

/// Duplicate and ambiguous identities with stable, portable diagnostic data.
///
/// Producers may attach source evidence when translating these errors into the
/// common diagnostic model. No parser-native text or runtime paths are needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", rename_all = "kebab-case", deny_unknown_fields)]
pub enum IdentityError {
    /// A required semantic input cannot establish a stable identity.
    #[error("invalid semantic identity field {field}: {reason}")]
    InvalidIdentity {
        /// Stable field identifier, not the rejected raw value.
        field: String,
        /// Fixed explanation of the failed contract.
        reason: String,
    },
    /// A canonical key was registered twice instead of being reconciled first.
    #[error("duplicate canonical item {item:?}")]
    #[serde(rename = "duplicate-item-identity")]
    DuplicateIdentity {
        /// Duplicated package-qualified identity.
        item: ItemReference,
    },
    /// A lookup name denotes more than one distinct semantic item.
    #[error("conflicting item alias {package}::{alias}")]
    #[serde(rename = "conflicting-item-alias")]
    ConflictingAlias {
        /// Package containing the lookup name.
        package: String,
        /// Ambiguous name.
        alias: String,
        /// All candidates, sorted independently of registration order.
        candidates: BTreeSet<ItemReference>,
    },
    /// An alias points outside the package registry or to an unregistered item.
    #[error("unknown canonical target for alias {alias}: {target:?}")]
    UnknownAliasTarget {
        /// Unbound lookup name.
        alias: String,
        /// Claimed canonical target.
        target: ItemReference,
    },
}

impl IdentityError {
    /// Stable code for translation into the shared diagnostic layer.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity { .. } => "invalid-identity",
            Self::DuplicateIdentity { .. } => "duplicate-item-identity",
            Self::ConflictingAlias { .. } => "conflicting-item-alias",
            Self::UnknownAliasTarget { .. } => "unknown-alias-target",
        }
    }
}

/// Package-local construction guard, not an extractor or workspace resolver.
///
/// Register canonical entities after source/stub reconciliation. Registering an
/// existing key fails without mutation. Alias binding is idempotent for the same
/// target and never registers an entity. Conflicting bindings retain *all*
/// candidates so subsequent resolution fails instead of choosing the first one.
#[derive(Debug, Clone)]
pub struct IdentityRegistry {
    package: String,
    items: BTreeSet<String>,
    aliases: BTreeMap<String, BTreeSet<ItemReference>>,
}

impl IdentityRegistry {
    /// Start an empty registry under a stable workspace package ID.
    pub fn new(package: &str) -> Result<Self, IdentityError> {
        nonempty("package", package)?;
        Ok(Self {
            package: package.into(),
            items: BTreeSet::new(),
            aliases: BTreeMap::new(),
        })
    }

    /// Register one already-reconciled canonical entity.
    pub fn register(
        &mut self,
        identity: &SemanticIdentity,
    ) -> Result<ItemReference, IdentityError> {
        let item = identity.in_package(&self.package)?;
        if !self.items.insert(item.item.clone()) {
            return Err(IdentityError::DuplicateIdentity { item });
        }
        Ok(item)
    }

    /// Bind a re-export, Rd alias, or canonical lookup spelling to an item.
    ///
    /// Bind public names to a family, not to each individual overload. An
    /// unknown target is rejected without changing the alias table.
    pub fn bind_alias(&mut self, alias: &str, target: &ItemReference) -> Result<(), IdentityError> {
        nonempty("alias", alias)?;
        if target.package != self.package || !self.items.contains(&target.item) {
            return Err(IdentityError::UnknownAliasTarget {
                alias: alias.into(),
                target: target.clone(),
            });
        }
        self.aliases
            .entry(alias.into())
            .or_default()
            .insert(target.clone());
        self.resolve_alias(alias).map(|_| ())
    }

    /// Resolve an unambiguous lookup name; unknown names return `None`.
    pub fn resolve_alias(&self, alias: &str) -> Result<Option<&ItemReference>, IdentityError> {
        let Some(candidates) = self.aliases.get(alias) else {
            return Ok(None);
        };
        if candidates.len() > 1 {
            return Err(IdentityError::ConflictingAlias {
                package: self.package.clone(),
                alias: alias.into(),
                candidates: candidates.clone(),
            });
        }
        Ok(candidates.first())
    }

    /// Number of canonical entities, independent of aliases.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether any canonical entities have been registered.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn invalid(field: &str, reason: &str) -> IdentityError {
    IdentityError::InvalidIdentity {
        field: field.into(),
        reason: reason.into(),
    }
}

fn nonempty(field: &str, value: &str) -> Result<(), IdentityError> {
    if value.is_empty() || value.chars().all(char::is_whitespace) {
        Err(invalid(field, "a nonempty semantic value is required"))
    } else {
        Ok(())
    }
}

// Delimiters and '%' cannot occur in atoms, including non-ASCII UTF-8 bytes.
fn atom(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

fn expression_option(
    key: &mut String,
    value: Option<&SignatureExpression>,
) -> Result<(), IdentityError> {
    if let Some(value) = value {
        key.push_str("some(");
        expression(key, value)?;
        key.push(')');
    } else {
        key.push_str("none");
    }
    Ok(())
}

fn expression(key: &mut String, value: &SignatureExpression) -> Result<(), IdentityError> {
    match value {
        SignatureExpression::Name { name, .. } => {
            write!(key, "name({})", atom(name)).unwrap();
        }
        SignatureExpression::Literal { text } => {
            write!(key, "literal({})", atom(text)).unwrap();
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            key.push_str("apply(");
            expression(key, constructor)?;
            key.push(';');
            expressions(key, arguments)?;
            key.push(')');
        }
        SignatureExpression::Sequence { items } => {
            key.push_str("sequence(");
            expressions(key, items)?;
            key.push(')');
        }
        SignatureExpression::LanguageSpecific {
            language,
            name,
            children,
            source,
        } => {
            if children.is_empty() && source.is_some() {
                return Err(invalid(
                    "signature-expression",
                    "opaque source-only syntax cannot establish overload identity",
                ));
            }
            write!(key, "language-specific({},{},", atom(language), atom(name)).unwrap();
            expressions(key, children)?;
            key.push(')');
        }
    }
    Ok(())
}

fn expressions(key: &mut String, values: &[SignatureExpression]) -> Result<(), IdentityError> {
    for value in values {
        expression(key, value)?;
        key.push(';');
    }
    Ok(())
}
