//! Doxygen XML translation into portable C++ declarations and documentation.

use std::collections::{BTreeMap, BTreeSet};

use roxmltree::{Document as XmlDocument, Node};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticEntity, Severity};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};

pub(crate) mod configuration;
mod documentation;
pub(crate) mod signatures;

pub use configuration::DoxygenConfiguration;

/// API items and diagnostics decoded from one completed native XML run.
#[derive(Debug, Default)]
pub struct DoxygenExtraction {
    /// Canonical declarations keyed independently of native reference IDs.
    pub items: BTreeMap<String, Item>,
    /// Unsupported surfaces and malformed XML in source order.
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone)]
struct Declaration<'a> {
    xml: Node<'a, 'a>,
    name: String,
    kind: CppDeclarationKind,
    scope: Option<String>,
    parent: Option<String>,
    protection: CppProtection,
}

pub(super) fn child<'a>(xml: Node<'a, 'a>, tag: &str) -> Option<Node<'a, 'a>> {
    xml.children().find(|n| n.has_tag_name(tag))
}
pub(super) fn text(xml: Node<'_, '_>) -> String {
    xml.descendants()
        .filter(Node::is_text)
        .filter_map(|n| n.text())
        .collect()
}

fn diagnostic(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticCode::DoxygenExtraction, Severity::Error, message)
}

fn protection(xml: Node<'_, '_>) -> CppProtection {
    match xml.attribute("prot") {
        Some("private") => CppProtection::Private,
        Some("protected") => CppProtection::Protected,
        _ => CppProtection::Public,
    }
}

fn included(xml: Node<'_, '_>, settings: &DoxygenConfiguration, member: bool) -> bool {
    (protection(xml) != CppProtection::Private
        || settings.enabled("EXTRACT_PRIVATE")
        || (settings.enabled("EXTRACT_PRIV_VIRTUAL")
            && xml.attribute("virt").is_some_and(|v| v != "non-virtual")))
        && (member || xml.attribute("static") != Some("yes") || settings.enabled("EXTRACT_STATIC"))
}

fn kind_tag(kind: CppDeclarationKind) -> &'static str {
    use CppDeclarationKind as K;
    match kind {
        K::Namespace => "namespace",
        K::Class => "class",
        K::Struct => "struct",
        K::Union => "union",
        K::Enum => "enum",
        K::Enumerator => "enumerator",
        K::Field => "field",
        K::Variable => "variable",
        K::TypeAlias => "type-alias",
        K::Macro => "macro",
        _ => "callable",
    }
}

fn shared_kind(kind: CppDeclarationKind) -> ItemKind {
    use CppDeclarationKind as K;
    match kind {
        K::Namespace => ItemKind::Namespace,
        K::Class => ItemKind::Class,
        K::Struct | K::Enum | K::Union | K::TypeAlias => ItemKind::Type,
        K::Function | K::Macro => ItemKind::Function,
        K::Method | K::Constructor | K::Destructor | K::Operator => ItemKind::Method,
        K::Field | K::Variable => ItemKind::Field,
        K::Enumerator => ItemKind::Constant,
    }
}

/// Translate an XML index and its referenced compounds without running Doxygen.
/// Native IDs resolve links during translation and never become portable keys.
pub fn extract_doxygen(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    files: &BTreeMap<String, String>,
    settings: &DoxygenConfiguration,
) -> DoxygenExtraction {
    let mut result = DoxygenExtraction::default();
    let outcome = extract(repository, package, files, settings, &mut result);
    if let Err(message) = outcome {
        result.diagnostics.push(diagnostic(message));
    }
    for diagnostic in &mut result.diagnostics {
        diagnostic.related_entity = Some(DiagnosticEntity::Target {
            package: package.id.clone(),
            id: target.id.clone(),
        });
    }
    result
}

fn extract(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    files: &BTreeMap<String, String>,
    settings: &DoxygenConfiguration,
    result: &mut DoxygenExtraction,
) -> Result<(), String> {
    let index = XmlDocument::parse(
        files
            .get("index.xml")
            .ok_or("Doxygen XML has no index.xml.")?,
    )
    .map_err(|e| format!("Cannot decode Doxygen index: {e}."))?;
    if !index.root_element().has_tag_name("doxygenindex") {
        return Err("Doxygen XML has no doxygenindex root.".into());
    }
    let mut documents = Vec::new();
    for entry in index
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("compound"))
    {
        if matches!(
            entry.attribute("kind"),
            Some("group" | "page" | "dir" | "example")
        ) {
            continue;
        }
        let id = entry
            .attribute("refid")
            .ok_or("Doxygen compound has no refid.")?;
        let path = format!("{id}.xml");
        let source = files
            .get(&path)
            .ok_or_else(|| format!("Doxygen XML is missing compound `{path}`."))?;
        let document = XmlDocument::parse(source)
            .map_err(|e| format!("Cannot decode Doxygen compound `{id}`: {e}."))?;
        if !document.root_element().has_tag_name("doxygen")
            || child(document.root_element(), "compounddef").and_then(|n| n.attribute("id"))
                != Some(id)
        {
            return Err(format!("Doxygen compound `{id}` has an invalid identity."));
        }
        documents.push(document);
    }
    let mut declarations = BTreeMap::<String, Declaration<'_>>::new();
    let mut hidden = BTreeSet::new();
    for document in &documents {
        let compound = child(document.root_element(), "compounddef").unwrap();
        let name = signatures::text_child(compound, "compoundname");
        if !included(compound, settings, true) {
            hidden.insert(name);
        }
    }
    for document in &documents {
        let compound = child(document.root_element(), "compounddef").unwrap();
        let name = signatures::text_child(compound, "compoundname");
        if hidden
            .iter()
            .any(|hidden| name == *hidden || name.starts_with(&format!("{hidden}::")))
        {
            continue;
        }
        if compound
            .attribute("language")
            .is_some_and(|language| language != "C++" && language != "C")
        {
            continue;
        }
        let kind = match compound.attribute("kind") {
            Some("namespace") => Some(CppDeclarationKind::Namespace),
            Some("class") => Some(CppDeclarationKind::Class),
            Some("struct") => Some(CppDeclarationKind::Struct),
            Some("union") => Some(CppDeclarationKind::Union),
            _ => None,
        };
        let id = compound.attribute("id").unwrap();
        if let Some(kind) = kind {
            declarations.insert(
                id.into(),
                Declaration {
                    xml: compound,
                    name: name.clone(),
                    kind,
                    scope: None,
                    parent: None,
                    protection: protection(compound),
                },
            );
        }
        let type_member = matches!(
            kind,
            Some(
                CppDeclarationKind::Class | CppDeclarationKind::Struct | CppDeclarationKind::Union
            )
        );
        for member in compound
            .children()
            .filter(|n| n.has_tag_name("sectiondef"))
            .flat_map(|n| n.children().filter(|n| n.has_tag_name("memberdef")))
        {
            if !included(member, settings, type_member) {
                continue;
            }
            let Some(member_id) = member.attribute("id") else {
                return Err("Doxygen member has no id.".into());
            };
            let short = signatures::text_child(member, "name");
            let qualified = signatures::text_child(member, "qualifiedname");
            let qualified = if qualified.is_empty() {
                if kind.is_some() {
                    format!("{name}::{short}")
                } else {
                    short.clone()
                }
            } else {
                qualified
            };
            let member_kind = match member.attribute("kind") {
                Some("function" | "friend") => {
                    if short.starts_with("operator") {
                        CppDeclarationKind::Operator
                    } else if type_member && short == name.rsplit("::").next().unwrap_or("") {
                        CppDeclarationKind::Constructor
                    } else if short.starts_with('~') {
                        CppDeclarationKind::Destructor
                    } else if type_member {
                        CppDeclarationKind::Method
                    } else {
                        CppDeclarationKind::Function
                    }
                }
                Some("enum") => CppDeclarationKind::Enum,
                Some("variable") if type_member => CppDeclarationKind::Field,
                Some("variable") => CppDeclarationKind::Variable,
                Some("typedef") => CppDeclarationKind::TypeAlias,
                Some("define") => CppDeclarationKind::Macro,
                _ => {
                    result.diagnostics.push(Diagnostic::new(
                        DiagnosticCode::DoxygenUnsupported,
                        Severity::Warning,
                        format!("Unsupported C++ declaration `{qualified}`."),
                    ));
                    continue;
                }
            };
            let declaration = Declaration {
                xml: member,
                name: qualified.clone(),
                kind: member_kind,
                scope: kind.map(|_| id.to_owned()),
                parent: type_member.then(|| id.to_owned()),
                protection: protection(member),
            };
            // Namespace and class compounds retain ownership more precisely than file copies.
            if !declarations.contains_key(member_id) || kind.is_some() {
                declarations.insert(member_id.into(), declaration);
            }
            for value in member.children().filter(|n| n.has_tag_name("enumvalue")) {
                let value_id = value
                    .attribute("id")
                    .ok_or("Doxygen enum value has no id.")?;
                declarations.insert(
                    value_id.into(),
                    Declaration {
                        xml: value,
                        name: format!("{qualified}::{}", signatures::text_child(value, "name")),
                        kind: CppDeclarationKind::Enumerator,
                        scope: Some(member_id.into()),
                        parent: Some(member_id.into()),
                        protection: protection(member),
                    },
                );
            }
        }
    }
    let mut families = BTreeMap::<String, Vec<String>>::new();
    let mut references = BTreeMap::new();
    for (native, declaration) in &declarations {
        let tag = kind_tag(declaration.kind);
        if tag == "callable" {
            families
                .entry(declaration.name.clone())
                .or_default()
                .push(native.clone());
        }
        let key = SemanticIdentity::cpp(&declaration.name, tag)
            .map_err(|e| e.to_string())?
            .item_id()
            .to_owned();
        references.insert(
            native.clone(),
            ItemReference {
                package: package.id.clone(),
                item: key,
            },
        );
    }
    for (name, overloads) in &families {
        if overloads.len() > 1 {
            for native in overloads {
                references.get_mut(native).unwrap().item = SemanticIdentity::cpp_overload(
                    name,
                    &signatures::dispatch(declarations[native].xml),
                )
                .map_err(|e| e.to_string())?
                .item_id()
                .to_owned();
            }
        }
    }
    let mut type_references = references.clone();
    for (native, declaration) in &declarations {
        if declaration.kind == CppDeclarationKind::Constructor
            && let Some(parent) = declaration
                .parent
                .as_ref()
                .and_then(|id| references.get(id))
        {
            type_references.insert(native.clone(), parent.clone());
        }
    }
    for (native, declaration) in &declarations {
        let reference = &references[native];
        let family = families
            .get(&declaration.name)
            .filter(|ids| ids.len() > 1)
            .map(|_| ItemReference {
                package: package.id.clone(),
                item: SemanticIdentity::cpp(&declaration.name, "callable")
                    .unwrap()
                    .item_id()
                    .to_owned(),
            });
        let owner = family.clone().or_else(|| {
            (declaration.kind != CppDeclarationKind::Enum)
                .then_some(declaration.parent.as_ref())
                .flatten()
                .and_then(|id| references.get(id))
                .cloned()
        });
        let source = child(declaration.xml, "location")
            .or_else(|| {
                declaration
                    .scope
                    .as_ref()
                    .and_then(|id| declarations.get(id))
                    .and_then(|parent| child(parent.xml, "location"))
            })
            .and_then(|location| location.attribute("file"))
            .map(|file| {
                let path = package.path.join(file);
                repository.source_location(&path, None).map_err(|_| {
                    format!(
                        "C++ declaration `{}` has a source outside its repository.",
                        declaration.name
                    )
                })
            })
            .transpose()?;
        let signature = signatures::signature(declaration.xml, declaration.kind, &type_references);
        let data = CppItemData {
            declaration: declaration.kind,
            owner,
            family: family.clone(),
            overloads: vec![],
            protection: declaration.protection,
            bases: declaration
                .xml
                .children()
                .filter(|n| n.has_tag_name("basecompoundref"))
                .map(|base| SignatureExpression::Name {
                    name: text(base),
                    target: base
                        .attribute("refid")
                        .and_then(|id| references.get(id))
                        .cloned(),
                })
                .collect(),
        };
        let item = Item {
            kind: shared_kind(declaration.kind),
            name: signatures::text_child(declaration.xml, "name"),
            qualified_name: declaration.name.clone(),
            language_data: Some(ItemLanguageData::Cpp(data)),
            aliases: vec![],
            signatures: vec![SourcedSignature {
                signature,
                sources: source
                    .iter()
                    .map(|source| SourceEvidence {
                        source: source.clone(),
                        role: SourceRole::Signature,
                        parsers: BTreeSet::from(["doxygen".into()]),
                    })
                    .collect(),
            }],
            documentation: documentation::document(
                declaration.xml,
                &references,
                source.clone(),
                &mut result.diagnostics,
            ),
            source_location: source,
            children: vec![],
            provenance: vec![],
        };
        let mut item = item;
        if item.name.is_empty() {
            item.name = declaration
                .name
                .rsplit("::")
                .next()
                .unwrap_or(&declaration.name)
                .to_owned();
        }
        if result.items.insert(reference.item.clone(), item).is_some() {
            return Err(format!(
                "C++ declarations share the semantic identity for `{}`.",
                declaration.name
            ));
        }
        if let Some(family) = family {
            let overload_item = result.items[&reference.item].clone();
            let family_item = result.items.entry(family.item.clone()).or_insert_with(|| {
                let mut item = overload_item;
                item.signatures.clear();
                item.documentation = None;
                item.source_location = None;
                if let Some(ItemLanguageData::Cpp(data)) = &mut item.language_data {
                    data.family = None;
                    data.owner = declaration
                        .parent
                        .as_ref()
                        .and_then(|id| references.get(id))
                        .cloned();
                }
                item
            });
            if let Some(ItemLanguageData::Cpp(data)) = &mut family_item.language_data {
                data.overloads.push(reference.clone());
            }
            family_item.children.push(reference.item.clone());
        }
    }
    for (native, declaration) in &declarations {
        if let Some(parent) = declaration.scope.as_ref().and_then(|id| references.get(id)) {
            let reference = &references[native];
            let child = if kind_tag(declaration.kind) == "callable" {
                SemanticIdentity::cpp(&declaration.name, "callable")
                    .unwrap()
                    .item_id()
                    .to_owned()
            } else {
                reference.item.clone()
            };
            if let Some(item) = result.items.get_mut(&parent.item)
                && !item.children.contains(&child)
            {
                item.children.push(child);
            }
        }
    }
    for document in &documents {
        let compound = child(document.root_element(), "compounddef").unwrap();
        if let Some(parent) = compound.attribute("id").and_then(|id| references.get(id)) {
            for child in compound
                .children()
                .filter(|n| n.has_tag_name("innerclass") || n.has_tag_name("innernamespace"))
            {
                if let Some(reference) = child.attribute("refid").and_then(|id| references.get(id))
                    && let Some(item) = result.items.get_mut(&parent.item)
                    && !item.children.contains(&reference.item)
                {
                    item.children.push(reference.item.clone());
                }
            }
        }
    }
    for item in result.items.values_mut() {
        item.children.sort();
        if let Some(ItemLanguageData::Cpp(data)) = &mut item.language_data {
            data.overloads.sort();
        }
    }
    Ok(())
}
