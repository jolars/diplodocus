//! Compiler-selected Rust declarations, stable identities, and inert Markdown.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use rustdoc_types as rd;

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticEntity, Severity};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};

mod documentation;
mod signatures;

use signatures::Signatures;

/// One compiler JSON graph translated into the shared portable API model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExtraction {
    /// Compiler-supplied package version, when present.
    pub version: Option<String>,
    /// Canonical items keyed independently of temporary rustdoc identifiers.
    pub items: BTreeMap<String, Item>,
    /// Unsupported surfaces and malformed compiler output.
    pub diagnostics: Vec<Diagnostic>,
}

/// Extract the public API of one library from rustdoc JSON format 61.
///
/// The caller owns compilation, input observation, and process supervision.
/// Public paths are reconciled before members so an export from a private module
/// remains reachable. Dependency declarations are used only for links and trait
/// implementation members, rather than imported as additional API roots.
pub fn extract_rustdoc(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    json: &[u8],
) -> RustExtraction {
    let mut result = RustExtraction {
        version: None,
        items: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    let format = serde_json::from_slice::<serde_json::Value>(json)
        .ok()
        .and_then(|value| value.get("format_version")?.as_u64());
    if format != Some(u64::from(rd::FORMAT_VERSION)) {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::RustExtraction,
            Severity::Error,
            format!(
                "Rust documentation requires JSON format {}; received {}.",
                rd::FORMAT_VERSION,
                format.map_or_else(|| "no valid format".into(), |format| format.to_string())
            ),
        ));
        return result;
    }
    let krate: rd::Crate = match serde_json::from_slice(json) {
        Ok(krate) => krate,
        Err(error) => {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RustExtraction,
                Severity::Error,
                format!("Cannot decode Rust documentation JSON: {error}."),
            ));
            return result;
        }
    };
    result.version = krate.crate_version.clone();
    let Some(root) = krate.index.get(&krate.root) else {
        result.diagnostics.push(Diagnostic::new(
            DiagnosticCode::RustExtraction,
            Severity::Error,
            "Rust documentation has no crate root.",
        ));
        return result;
    };
    let root_name = root
        .name
        .clone()
        .unwrap_or_else(|| package.id.replace('-', "_"));
    let mut graph = Graph {
        krate: &krate,
        local_crate: root.crate_id,
        defining: BTreeMap::new(),
        paths: BTreeMap::new(),
        parents: BTreeMap::new(),
        contexts: BTreeMap::new(),
        impls: BTreeMap::new(),
        implementation_docs: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    graph.defining_paths(krate.root, &root_name, &mut BTreeSet::new());
    graph.public(krate.root, &root_name, &mut BTreeSet::new());
    let roots: Vec<_> = graph.paths.keys().copied().collect();
    for id in roots {
        graph.members(id);
    }
    let mut references = BTreeMap::new();
    for id in graph.paths.keys() {
        let Some(item) = krate.index.get(id) else {
            continue;
        };
        let Some((_, _, kind)) = classification(item, graph.parents.contains_key(id)) else {
            continue;
        };
        let defining = graph.defining_name(*id);
        let identity = if let Some(parent) = graph.parents.get(id) {
            let context = match &item.inner {
                rd::ItemEnum::Impl(implementation) => graph.impl_identity(implementation),
                _ => graph.contexts.get(id).cloned().unwrap_or_default(),
            };
            SemanticIdentity::rust_member(
                &graph.defining_name(*parent),
                &context,
                item.name.as_deref().unwrap_or("impl"),
                kind,
            )
        } else {
            SemanticIdentity::rust(&defining, kind)
        };
        match identity.and_then(|identity| identity.in_package(&package.id)) {
            Ok(reference) => {
                if references.values().any(|previous| previous == &reference) {
                    graph.diagnostics.push(Diagnostic::new(
                        DiagnosticCode::RustExtraction,
                        Severity::Error,
                        format!("Rust declarations share the semantic identity for `{defining}`."),
                    ));
                } else {
                    references.insert(*id, reference);
                }
            }
            Err(error) => graph.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RustExtraction,
                Severity::Error,
                format!("Cannot identify Rust declaration `{defining}`: {error}."),
            )),
        }
    }
    let signatures = Signatures {
        references: &references,
        krate: &krate,
    };
    let documentation = documentation::DocumentationContext {
        references: &references,
        krate: &krate,
        package: &package.id,
    };
    for (id, reference) in &references {
        let native = &krate.index[id];
        let member = graph.parents.contains_key(id);
        let Some((kind, declaration, _)) = classification(native, member) else {
            continue;
        };
        let paths = &graph.paths[id];
        let public_path = paths
            .iter()
            .min_by_key(|path| (path.split("::").count(), path.as_str()))
            .expect("reachable path")
            .clone();
        let item_source = source_location(repository, package, target, native);
        let sources = item_source
            .iter()
            .map(|source| SourceEvidence {
                source: source.clone(),
                role: SourceRole::Signature,
                parsers: BTreeSet::from(["rustdoc".into()]),
            })
            .collect();
        let mut aliases: Vec<_> = paths
            .iter()
            .filter(|path| **path != public_path)
            .map(|path| ItemAlias {
                qualified_name: path.clone(),
                kind: ItemAliasKind::RustReexport,
                sources: Vec::new(),
            })
            .collect();
        let defining = graph.defining_name(*id);
        if defining != public_path && !aliases.iter().any(|alias| alias.qualified_name == defining)
        {
            aliases.push(ItemAlias {
                qualified_name: defining,
                kind: ItemAliasKind::RustReexport,
                sources: Vec::new(),
            });
        }
        aliases.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        let owner = graph
            .parents
            .get(id)
            .and_then(|id| references.get(id))
            .cloned();
        let mut implementations = BTreeMap::<String, RustImplementation>::new();
        let implementors = match &native.inner {
            rd::ItemEnum::Trait(trait_) => trait_.implementations.as_slice(),
            _ => &[],
        };
        let implementation_ids = graph
            .impls
            .get(id)
            .into_iter()
            .flatten()
            .map(|id| (id, false))
            .chain(implementors.iter().map(|id| (id, true)));
        for (implementation_id, trait_summary) in implementation_ids {
            let Some(implementation_item) = krate.index.get(implementation_id) else {
                continue;
            };
            let rd::ItemEnum::Impl(implementation) = &implementation_item.inner else {
                continue;
            };
            if trait_summary
                && (implementation_item.crate_id != graph.local_crate
                    || hidden(implementation_item)
                    || implementation
                        .trait_
                        .as_ref()
                        .is_none_or(|trait_| trait_.id != *id)
                    || !public_implementor(&implementation.for_, &references))
            {
                continue;
            }
            let signature = signatures.implementation(implementation);
            let identity = graph.impl_identity(implementation);
            let members: Vec<_> = implementation
                .items
                .iter()
                .filter(|member| !trait_summary && graph.parents.get(member) == Some(id))
                .filter_map(|member| references.get(member).cloned())
                .collect();
            let documentation_item = if trait_summary {
                None
            } else {
                graph
                    .implementation_docs
                    .iter()
                    .find(|(_, ids)| ids.contains(implementation_id))
                    .and_then(|(id, _)| references.get(id))
                    .cloned()
            };
            let entry =
                implementations
                    .entry(identity.clone())
                    .or_insert_with(|| RustImplementation {
                        identity,
                        signature,
                        trait_name: implementation
                            .trait_
                            .as_ref()
                            .map(|trait_| trait_.path.clone()),
                        trait_target: implementation
                            .trait_
                            .as_ref()
                            .and_then(|trait_| references.get(&trait_.id).cloned()),
                        members: Vec::new(),
                        is_synthetic: implementation.is_synthetic,
                        is_blanket: implementation.blanket_impl.is_some(),
                        documentation_item: None,
                        source_location: source_location(
                            repository,
                            package,
                            target,
                            implementation_item,
                        ),
                    });
            entry.members.extend(members);
            if documentation_item.is_some() {
                entry.documentation_item = documentation_item;
            }
            entry.members.sort();
            entry.members.dedup();
        }
        let implementations = implementations.into_values().collect();
        let mut item = Item {
            kind,
            name: native.name.clone().unwrap_or_else(|| match &native.inner {
                rd::ItemEnum::Impl(implementation) => {
                    signatures::spelling(&signatures.implementation(implementation))
                }
                _ => "_".into(),
            }),
            qualified_name: public_path.clone(),
            language_data: Some(ItemLanguageData::Rust(RustItemData {
                declaration,
                public_path,
                owner,
                implementation: graph.contexts.get(id).cloned(),
                implementations,
                attributes: native.attrs.iter().map(attribute).collect(),
            })),
            aliases,
            signatures: vec![SourcedSignature {
                signature: Signature::LanguageSpecific {
                    language: "rust".into(),
                    syntax: signatures.declaration(native),
                },
                sources,
            }],
            documentation: None,
            source_location: item_source.clone(),
            children: Vec::new(),
            provenance: Vec::new(),
        };
        if let Some(group) = graph
            .implementation_docs
            .get(id)
            .filter(|group| group.len() > 1)
        {
            let docs = group
                .iter()
                .filter_map(|id| krate.index[id].docs.as_deref())
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut merged = native.clone();
            for id in group {
                merged.links.extend(krate.index[id].links.clone());
            }
            item.documentation = Some(documentation::parse(
                documentation,
                &docs,
                &merged,
                None,
                &reference.item,
                &mut result.diagnostics,
            ));
        } else if let Some(docs) = &native.docs {
            item.documentation = Some(documentation::parse(
                documentation,
                docs,
                native,
                item_source,
                &reference.item,
                &mut result.diagnostics,
            ));
        }
        result.items.insert(reference.item.clone(), item);
    }
    // A member has one lexical owner even when it is reachable by several
    // reexports. Module edges may include the same declaration more than once.
    for (id, reference) in &references {
        let native = &krate.index[id];
        let children = if matches!(native.inner, rd::ItemEnum::Module(_)) {
            graph
                .paths
                .iter()
                .filter(|(child, paths)| {
                    *child != id
                        && !graph.parents.contains_key(child)
                        && paths.iter().any(|path| {
                            graph.paths[id].iter().any(|parent| {
                                path.strip_prefix(&format!("{parent}::"))
                                    .is_some_and(|suffix| !suffix.contains("::"))
                            })
                        })
                })
                .filter_map(|(child, _)| references.get(child).cloned())
                .collect::<Vec<_>>()
        } else {
            graph
                .parents
                .iter()
                .filter(|(_, parent)| *parent == id)
                .filter_map(|(child, _)| references.get(child).cloned())
                .collect()
        };
        if let Some(item) = result.items.get_mut(&reference.item) {
            item.children = children
                .into_iter()
                .map(|reference| reference.item)
                .collect();
            item.children.sort();
            item.children.dedup();
        }
    }
    result.diagnostics.extend(graph.diagnostics);
    for diagnostic in &mut result.diagnostics {
        if diagnostic.related_entity.is_none() {
            diagnostic.related_entity = Some(DiagnosticEntity::Package {
                id: package.id.clone(),
            });
        }
    }
    result.diagnostics.sort();
    result.diagnostics.dedup();
    result
}

fn public_implementor(type_: &rd::Type, references: &BTreeMap<rd::Id, ItemReference>) -> bool {
    match type_ {
        rd::Type::ResolvedPath(path) => references.contains_key(&path.id),
        rd::Type::BorrowedRef { type_, .. } | rd::Type::RawPointer { type_, .. } => {
            public_implementor(type_, references)
        }
        _ => false,
    }
}

struct Graph<'a> {
    krate: &'a rd::Crate,
    local_crate: u32,
    defining: BTreeMap<rd::Id, String>,
    paths: BTreeMap<rd::Id, BTreeSet<String>>,
    parents: BTreeMap<rd::Id, rd::Id>,
    contexts: BTreeMap<rd::Id, String>,
    impls: BTreeMap<rd::Id, Vec<rd::Id>>,
    implementation_docs: BTreeMap<rd::Id, Vec<rd::Id>>,
    diagnostics: Vec<Diagnostic>,
}

impl Graph<'_> {
    fn defining_name(&self, id: rd::Id) -> String {
        self.defining
            .get(&id)
            .cloned()
            .or_else(|| {
                self.krate
                    .paths
                    .get(&id)
                    .map(|summary| summary.path.join("::"))
            })
            .or_else(|| self.paths.get(&id)?.iter().next().cloned())
            .unwrap_or_else(|| "<unknown>".into())
    }

    fn defining_paths(&mut self, id: rd::Id, path: &str, seen: &mut BTreeSet<rd::Id>) {
        if !seen.insert(id) {
            return;
        }
        let Some(item) = self.krate.index.get(&id) else {
            return;
        };
        if item.crate_id != self.local_crate {
            return;
        }
        self.defining.insert(id, path.into());
        let children = match &item.inner {
            rd::ItemEnum::Module(module) => module.items.clone(),
            rd::ItemEnum::Trait(trait_) => trait_.items.clone(),
            rd::ItemEnum::Enum(enum_) => enum_.variants.clone(),
            _ => fields(item),
        };
        for child in children {
            if let Some(item) = self.krate.index.get(&child)
                && !matches!(item.inner, rd::ItemEnum::Use(_))
                && let Some(name) = &item.name
            {
                self.defining_paths(child, &format!("{path}::{name}"), seen);
            }
        }
    }

    fn public(&mut self, id: rd::Id, path: &str, lineage: &mut BTreeSet<rd::Id>) {
        if !lineage.insert(id) {
            return;
        }
        let Some(item) = self.krate.index.get(&id) else {
            lineage.remove(&id);
            return;
        };
        if item.crate_id != self.local_crate || hidden(item) {
            lineage.remove(&id);
            return;
        }
        if classification(item, false).is_some() {
            self.paths.entry(id).or_default().insert(path.into());
        } else if !matches!(item.inner, rd::ItemEnum::Use(_) | rd::ItemEnum::Impl(_)) {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RustUnsupportedSurface,
                Severity::Warning,
                format!("Rust declaration `{path}` uses an unsupported public item kind."),
            ));
        }
        if let rd::ItemEnum::Module(module) = &item.inner {
            self.public_module(&module.items, path, lineage);
        }
        lineage.remove(&id);
    }

    fn public_module(&mut self, children: &[rd::Id], path: &str, lineage: &mut BTreeSet<rd::Id>) {
        for child in children {
            let Some(item) = self.krate.index.get(child) else {
                continue;
            };
            if !matches!(item.visibility, rd::Visibility::Public) || hidden(item) {
                continue;
            }
            if let rd::ItemEnum::Use(use_) = &item.inner {
                if let Some(id) = use_.id {
                    if use_.is_glob {
                        if !lineage.contains(&id)
                            && let Some(rd::Item {
                                inner: rd::ItemEnum::Module(module),
                                ..
                            }) = self.krate.index.get(&id)
                        {
                            lineage.insert(id);
                            self.public_module(&module.items, path, lineage);
                            lineage.remove(&id);
                        }
                    } else {
                        self.public(id, &format!("{path}::{}", use_.name), lineage);
                    }
                }
            } else if let Some(name) = &item.name {
                self.public(*child, &format!("{path}::{name}"), lineage);
            }
        }
    }

    fn members(&mut self, id: rd::Id) {
        let Some(item) = self.krate.index.get(&id) else {
            return;
        };
        let mut direct = fields(item);
        let (implementations, inherited_visibility) = match &item.inner {
            rd::ItemEnum::Struct(value) => (value.impls.clone(), false),
            rd::ItemEnum::Union(value) => (value.impls.clone(), false),
            rd::ItemEnum::Enum(value) => {
                direct.extend(&value.variants);
                (value.impls.clone(), true)
            }
            rd::ItemEnum::Trait(value) => {
                direct.extend(&value.items);
                (Vec::new(), true)
            }
            rd::ItemEnum::Variant(_) => (Vec::new(), true),
            _ => (Vec::new(), false),
        };
        for member in direct {
            if self.member(id, member, "", inherited_visibility) {
                self.members(member);
            }
        }
        let mut impls = Vec::new();
        for implementation_id in implementations {
            let Some(native) = self.krate.index.get(&implementation_id) else {
                continue;
            };
            let rd::ItemEnum::Impl(implementation) = &native.inner else {
                continue;
            };
            if hidden(native) {
                continue;
            }
            let context = self.impl_identity(implementation);
            if implementation.blanket_impl.is_none() {
                for member in &implementation.items {
                    self.member(id, *member, &context, implementation.trait_.is_some());
                }
            }
            if native.docs.is_some() {
                let existing = self.implementation_docs.keys().find(|key| self.parents.get(key) == Some(&id) && matches!(&self.krate.index[key].inner, rd::ItemEnum::Impl(value) if self.impl_identity(value) == context)).copied();
                if let Some(existing) = existing {
                    self.implementation_docs
                        .get_mut(&existing)
                        .expect("existing group")
                        .push(implementation_id);
                } else {
                    let paths = self.paths.get(&id).cloned().unwrap_or_default();
                    self.paths
                        .entry(implementation_id)
                        .or_default()
                        .extend(paths.into_iter().map(|path| format!("{path}::{context}")));
                    self.parents.insert(implementation_id, id);
                    self.implementation_docs
                        .insert(implementation_id, vec![implementation_id]);
                }
            }
            impls.push(implementation_id);
        }
        impls.sort_by_key(|id| match &self.krate.index[id].inner {
            rd::ItemEnum::Impl(implementation) => self.impl_identity(implementation),
            _ => String::new(),
        });
        if !impls.is_empty() {
            self.impls.insert(id, impls);
        }
    }

    fn member(
        &mut self,
        owner: rd::Id,
        id: rd::Id,
        context: &str,
        inherited_visibility: bool,
    ) -> bool {
        let Some(item) = self.krate.index.get(&id) else {
            return false;
        };
        if hidden(item)
            || (!matches!(item.visibility, rd::Visibility::Public)
                && !(inherited_visibility && matches!(item.visibility, rd::Visibility::Default)))
        {
            return false;
        }
        if classification(item, true).is_none() {
            return false;
        }
        let Some(name) = &item.name else { return false };
        let paths = self.paths.get(&owner).cloned().unwrap_or_default();
        self.paths
            .entry(id)
            .or_default()
            .extend(paths.into_iter().map(|path| format!("{path}::{name}")));
        self.parents.insert(id, owner);
        if !context.is_empty() {
            self.contexts.insert(id, context.into());
        }
        let defining = format!("{}::{name}", self.defining_name(owner));
        self.defining.entry(id).or_insert(defining);
        true
    }

    fn impl_identity(&self, implementation: &rd::Impl) -> String {
        // Replace compiler-selected paths with defining paths before producing
        // the discriminator so aliases and rustdoc's numeric IDs cannot leak.
        let mut normalized = implementation.clone();
        normalize_type_paths(&mut normalized.for_, self);
        if let Some(receiver) = &mut normalized.blanket_impl {
            normalize_type_paths(receiver, self);
        }
        if let Some(path) = &mut normalized.trait_ {
            path.path = self.defining_name(path.id);
        }
        let empty = BTreeMap::new();
        signatures::spelling(
            &Signatures {
                references: &empty,
                krate: self.krate,
            }
            .implementation(&normalized),
        )
    }
}

fn normalize_type_paths(type_: &mut rd::Type, graph: &Graph<'_>) {
    match type_ {
        rd::Type::ResolvedPath(path) => {
            path.path = graph.defining_name(path.id);
        }
        rd::Type::BorrowedRef { type_, .. }
        | rd::Type::RawPointer { type_, .. }
        | rd::Type::Array { type_, .. }
        | rd::Type::Pat { type_, .. }
        | rd::Type::Slice(type_) => normalize_type_paths(type_, graph),
        rd::Type::Tuple(types) => {
            for type_ in types {
                normalize_type_paths(type_, graph);
            }
        }
        _ => {}
    }
}

fn fields(item: &rd::Item) -> Vec<rd::Id> {
    match &item.inner {
        rd::ItemEnum::Struct(value) => match &value.kind {
            rd::StructKind::Unit => Vec::new(),
            rd::StructKind::Tuple(fields) => fields.iter().flatten().copied().collect(),
            rd::StructKind::Plain { fields, .. } => fields.clone(),
        },
        rd::ItemEnum::Union(value) => value.fields.clone(),
        rd::ItemEnum::Variant(value) => match &value.kind {
            rd::VariantKind::Plain => Vec::new(),
            rd::VariantKind::Tuple(fields) => fields.iter().flatten().copied().collect(),
            rd::VariantKind::Struct { fields, .. } => fields.clone(),
        },
        _ => Vec::new(),
    }
}

fn hidden(item: &rd::Item) -> bool {
    item.attrs.iter().any(|attribute| matches!(attribute, rd::Attribute::Other(value) if value.replace(' ', "").contains("doc(hidden)")))
}

fn classification(
    item: &rd::Item,
    member: bool,
) -> Option<(ItemKind, RustDeclarationKind, &'static str)> {
    Some(match item.inner {
        rd::ItemEnum::Module(_) => (ItemKind::Module, RustDeclarationKind::Module, "module"),
        rd::ItemEnum::Struct(_) => (ItemKind::Type, RustDeclarationKind::Struct, "struct"),
        rd::ItemEnum::Enum(_) => (ItemKind::Type, RustDeclarationKind::Enum, "enum"),
        rd::ItemEnum::Union(_) => (ItemKind::Type, RustDeclarationKind::Union, "union"),
        rd::ItemEnum::Trait(_) | rd::ItemEnum::TraitAlias(_) => {
            (ItemKind::Type, RustDeclarationKind::Trait, "trait")
        }
        rd::ItemEnum::Variant(_) => (ItemKind::Type, RustDeclarationKind::Variant, "variant"),
        rd::ItemEnum::StructField(_) => (ItemKind::Field, RustDeclarationKind::Field, "field"),
        rd::ItemEnum::Function(_) if member => {
            (ItemKind::Method, RustDeclarationKind::Method, "method")
        }
        rd::ItemEnum::Function(_) => (
            ItemKind::Function,
            RustDeclarationKind::Function,
            "function",
        ),
        rd::ItemEnum::Constant { .. } | rd::ItemEnum::AssocConst { .. } => (
            ItemKind::Constant,
            RustDeclarationKind::Constant,
            "constant",
        ),
        rd::ItemEnum::Static(_) => (ItemKind::Constant, RustDeclarationKind::Static, "static"),
        rd::ItemEnum::TypeAlias(_) => {
            (ItemKind::Type, RustDeclarationKind::TypeAlias, "type-alias")
        }
        rd::ItemEnum::AssocType { .. } => (
            ItemKind::Type,
            RustDeclarationKind::AssociatedType,
            "associated-type",
        ),
        rd::ItemEnum::Macro(_) | rd::ItemEnum::ProcMacro(_) => {
            (ItemKind::Function, RustDeclarationKind::Macro, "macro")
        }
        rd::ItemEnum::Impl(_) => (
            ItemKind::Namespace,
            RustDeclarationKind::Implementation,
            "implementation",
        ),
        _ => return None,
    })
}

fn attribute(attribute: &rd::Attribute) -> String {
    match attribute {
        rd::Attribute::Other(value) => value.clone(),
        rd::Attribute::NonExhaustive => "#[non_exhaustive]".into(),
        rd::Attribute::MustUse { reason: None } => "#[must_use]".into(),
        rd::Attribute::MustUse {
            reason: Some(reason),
        } => format!("#[must_use = {reason:?}]"),
        rd::Attribute::MacroExport => "#[macro_export]".into(),
        rd::Attribute::AutomaticallyDerived => "#[automatically_derived]".into(),
        rd::Attribute::NoMangle => "#[no_mangle]".into(),
        attribute => serde_json::to_string(attribute).expect("Rust attributes are serializable"),
    }
}

fn source_location(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    item: &rd::Item,
) -> Option<SourceLocation> {
    let native = item.span.as_ref()?;
    let candidates = if native.filename.is_absolute() {
        vec![native.filename.clone()]
    } else {
        vec![
            repository.path.join(&native.filename),
            package.path.join(&native.filename),
            target.path.parent()?.join(&native.filename),
        ]
    };
    for path in candidates {
        let Ok(mut source) = repository.source_location(&path, None) else {
            continue;
        };
        if let Ok(text) = fs::read_to_string(&path) {
            let offset = |(line, column): (usize, usize)| -> Option<usize> {
                if line == 0 || column == 0 {
                    return None;
                }
                let start = if line == 1 {
                    0
                } else {
                    text.match_indices('\n').nth(line - 2)?.0 + 1
                };
                let line_end = text[start..]
                    .find('\n')
                    .map_or(text.len(), |end| start + end);
                // Rustdoc uses one-based Unicode character columns, whereas
                // the portable IR uses half-open UTF-8 byte offsets.
                text[start..line_end]
                    .char_indices()
                    .map(|(offset, _)| start + offset)
                    .chain(std::iter::once(line_end))
                    .nth(column - 1)
            };
            source.span = offset(native.begin)
                .zip(offset(native.end))
                .filter(|(start, end)| start <= end)
                .map(|(start, end)| SourceSpan { start, end });
        }
        return Some(source);
    }
    None
}

#[cfg(test)]
mod tests;
