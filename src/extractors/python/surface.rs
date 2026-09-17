//! Reconcile parsed Python modules into canonical package-scoped API items.
//!
//! Literal exports are authoritative. Without `__all__`, public definitions and
//! explicitly aliased imports (including redundant aliases) form the surface;
//! unaliased imports used for annotations do not become public accidentally.
//! Maintained stubs select declarations and signatures; implementation prose
//! remains independently attributable. This pass never evaluates Python code.

use super::model;
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::ir::*;
use model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[path = "surface/bindings.rs"]
mod bindings;
#[path = "surface/declarations.rs"]
mod declarations;
#[path = "surface/exports.rs"]
mod exports;
use bindings::Binding;

/// Reconciled items and unprocessed documentation keyed by canonical item ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonSurface {
    /// Canonical items, including separately addressable overloads.
    pub items: BTreeMap<String, Item>,
    /// Preferred implementation prose, or stub prose when no source exists.
    pub docstrings: BTreeMap<String, ParsedDocstring>,
    /// Source and surface diagnostics in the shared deterministic order.
    pub diagnostics: Vec<Diagnostic>,
}

struct Builder<'a> {
    package: &'a str,
    registry: IdentityRegistry,
    items: BTreeMap<String, Item>,
    docstrings: BTreeMap<String, ParsedDocstring>,
    diagnostics: Vec<Diagnostic>,
    direct: BTreeMap<String, BTreeSet<String>>,
    bindings: BTreeMap<String, Vec<Binding>>,
    module_members: BTreeMap<String, Vec<(String, String)>>,
    alias_conflicts: BTreeSet<String>,
}

struct ModuleSurface {
    name: String,
    id: String,
    exports: PythonExports,
    names: BTreeMap<String, (bool, bool, SourceLocation)>,
}

/// Resolve the public surface without importing or executing documented code.
///
/// A dynamic export list retains visible definitions with unknown visibility.
/// Cycles, unresolved exports, and ambiguous aliases yield diagnostics rather
/// than fallback identities. Module discovery order never selects a winner.
pub fn reconcile(package: &ParsedPythonPackage) -> PythonSurface {
    let Ok(registry) = IdentityRegistry::new(&package.package) else {
        let mut diagnostics = package.diagnostics.clone();
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::PythonInvalidIdentity,
            Severity::Error,
            "A nonempty package ID is required for Python identities.",
        ));
        diagnostics.sort();
        return PythonSurface {
            items: BTreeMap::new(),
            docstrings: BTreeMap::new(),
            diagnostics,
        };
    };
    let mut builder = Builder {
        package: &package.package,
        registry,
        items: BTreeMap::new(),
        docstrings: BTreeMap::new(),
        diagnostics: package.diagnostics.clone(),
        direct: BTreeMap::new(),
        bindings: BTreeMap::new(),
        module_members: BTreeMap::new(),
        alias_conflicts: BTreeSet::new(),
    };
    let mut modules: BTreeMap<&str, Vec<&ParsedModule>> = BTreeMap::new();
    for module in package.modules.iter().filter(|m| m.valid) {
        modules.entry(&module.name).or_default().push(module);
    }
    let mut surfaces = vec![];
    for (name, mut variants) in modules {
        variants.sort_by_key(|m| (&m.source.path, m.source.span.map(|s| (s.start, s.end))));
        let implementations: Vec<_> = variants
            .iter()
            .filter(|m| m.kind == PythonSourceKind::Source)
            .copied()
            .collect();
        let stubs: Vec<_> = variants
            .iter()
            .filter(|m| m.kind == PythonSourceKind::Stub)
            .copied()
            .collect();
        if implementations.len() > 1 || stubs.len() > 1 {
            builder.diagnostic(
                DiagnosticCode::PythonConflictingStub,
                Severity::Error,
                format!("Multiple maintained inputs declare module {name}."),
                &variants[0].source,
            );
            continue;
        }
        let source = implementations.first().copied();
        let stub = stubs.first().copied();
        let selected = stub.or(source).unwrap();
        let export_module = if selected.exports.is_empty() {
            source.unwrap_or(selected)
        } else {
            selected
        };
        let exports = exports::exports(export_module);
        if let PythonExports::Dynamic { sources, .. } = &exports {
            builder.diagnostic(
                DiagnosticCode::PythonDynamicExport,
                Severity::Warning,
                format!("The export list of {name} cannot be resolved statically."),
                sources
                    .last()
                    .map(|s| &s.source)
                    .unwrap_or(&selected.source),
            );
        }
        let Some(id) = builder.identity(name, PythonIdentityKind::Module, &selected.source) else {
            continue;
        };
        let module_source = match (source, stub) {
            (Some(_), Some(_)) => PythonModuleSource::ImplementationAndStub,
            (None, Some(_)) => PythonModuleSource::StubOnly,
            _ => PythonModuleSource::Implementation,
        };
        let mut item = new_item(
            ItemKind::Module,
            name.rsplit('.').next().unwrap_or(name),
            name,
            PythonDeclaration::Module {
                source: module_source,
                exports: exports.clone(),
            },
            &source.unwrap_or(selected).source,
        );
        item.provenance = variants.iter().map(|m| provenance(&m.source)).collect();
        if let Some(doc) = source
            .and_then(|m| m.docstring.clone())
            .or_else(|| selected.docstring.clone())
        {
            builder.docstrings.insert(id.clone(), doc);
        }
        item.children = builder.declarations(
            selected,
            source,
            name,
            &selected.declarations,
            source
                .map(|m| m.declarations.as_slice())
                .unwrap_or_default(),
            false,
        );
        builder
            .direct
            .entry(name.into())
            .or_default()
            .insert(id.clone());
        builder.items.insert(id.clone(), item);
        let mut names = BTreeMap::new();
        for declaration in &selected.declarations {
            if declaration.name != "__all__" {
                names.insert(
                    declaration.name.clone(),
                    (true, false, declaration.source.clone()),
                );
            }
        }
        let selected_imports = bindings::imports(selected);
        for (local, bindings) in selected_imports {
            if local == "*" {
                builder.diagnostic(
                    DiagnosticCode::PythonUnsupportedSurface,
                    Severity::Error,
                    format!("Wildcard imports cannot establish the public surface of {name}."),
                    &bindings[0].source,
                );
                continue;
            }
            names
                .entry(local.clone())
                .and_modify(|v| v.1 |= bindings.iter().any(|b| b.explicit))
                .or_insert((
                    false,
                    bindings.iter().any(|b| b.explicit),
                    bindings[0].source.clone(),
                ));
            let mut bindings = bindings;
            if let Some(source) = source.filter(|s| s.kind != selected.kind)
                && let Some(originals) = bindings::imports(source).get(&local)
            {
                for original in originals {
                    if bindings.iter().any(|b| b.target == original.target) {
                        bindings.push(original.clone());
                    } else {
                        builder.diagnostic(
                            DiagnosticCode::PythonConflictingStub,
                            Severity::Error,
                            format!(
                                "Source and stub reexport {name}.{local} from different targets."
                            ),
                            &original.source,
                        );
                    }
                }
            }
            builder
                .bindings
                .entry(format!("{name}.{local}"))
                .or_default()
                .extend(bindings);
        }
        if let PythonExports::Explicit {
            names: exported,
            sources,
        } = &exports
        {
            for name in exported {
                names.entry(name.clone()).or_insert((
                    false,
                    false,
                    sources
                        .last()
                        .map(|s| s.source.clone())
                        .unwrap_or_else(|| selected.source.clone()),
                ));
            }
        }
        surfaces.push(ModuleSurface {
            name: name.into(),
            id,
            exports,
            names,
        });
    }
    let mut visible = BTreeMap::new();
    let mut pending_aliases = Vec::new();
    let mut pending_modules: BTreeSet<_> = surfaces
        .iter()
        .enumerate()
        .filter(|(_, surface)| {
            !surface
                .name
                .split('.')
                .skip(1)
                .any(|part| part.starts_with('_'))
        })
        .map(|(index, _)| index)
        .collect();
    let mut handled_modules = BTreeSet::new();
    while let Some(index) = pending_modules.pop_first() {
        if !handled_modules.insert(index) {
            continue;
        }
        let surface = &surfaces[index];
        visible.insert(surface.id.clone(), PythonVisibility::Public);
        builder.items.get_mut(&surface.id).unwrap().children.clear();
        let mut names: Vec<_> = surface.names.iter().collect();
        if let PythonExports::Explicit {
            names: exported, ..
        } = &surface.exports
        {
            names.sort_by_key(|(name, _)| exported.iter().position(|n| n == *name));
        } else {
            names.sort_by_key(|(name, (_, _, source))| {
                (source.span.map(|s| (s.start, s.end)), *name)
            });
        }
        for (name, (definition, explicit, location)) in names {
            let Some(visibility) =
                exports::visibility(&surface.exports, name, *explicit, *definition)
            else {
                continue;
            };
            let qualified = format!("{}.{}", surface.name, name);
            let targets = bindings::resolve(
                &qualified,
                &builder.direct,
                &builder.bindings,
                &mut BTreeSet::new(),
            );
            if targets.len() != 1 {
                // A rejected declaration already has its precise diagnostic.
                if !definition || builder.bindings.contains_key(&qualified) {
                    let code = if targets.is_empty() {
                        DiagnosticCode::PythonUnresolvedReexport
                    } else {
                        DiagnosticCode::PythonConflictingAlias
                    };
                    builder.diagnostic(
                        code,
                        Severity::Error,
                        format!(
                            "Public name {qualified} resolves to {} canonical items.",
                            targets.len()
                        ),
                        location,
                    );
                }
                continue;
            }
            let target = targets.first().unwrap();
            builder.expose(target, visibility, &mut visible, &mut BTreeSet::new());
            if let Some(index) = surfaces.iter().position(|surface| &surface.id == target)
                && !handled_modules.contains(&index)
            {
                pending_modules.insert(index);
            }
            if let Some(bindings) = builder.bindings.get(&qualified).cloned() {
                for binding in bindings {
                    pending_aliases.push((target.clone(), qualified.clone(), binding));
                }
            }
            if let Some(module) = builder.items.get_mut(&surface.id)
                && target != &surface.id
                && !module.children.contains(target)
            {
                module.children.push(target.clone());
            }
            if visibility == PythonVisibility::Public {
                builder
                    .module_members
                    .entry(surface.id.clone())
                    .or_default()
                    .push((name.clone(), target.clone()));
            }
        }
    }
    // A public reexport keeps its canonical defining module addressable, but
    // does not promote that private module's unrelated declarations.
    for (id, visibility) in visible.clone() {
        if visibility == PythonVisibility::Private {
            continue;
        }
        let qualified = &builder.items[&id].qualified_name;
        for surface in &surfaces {
            if qualified.starts_with(&format!("{}.", surface.name)) {
                visible.insert(surface.id.clone(), PythonVisibility::Public);
            }
        }
    }
    for (name, targets) in &builder.direct {
        for target in targets {
            if let Err(IdentityError::ConflictingAlias { .. }) = builder.registry.bind_alias(
                name,
                &ItemReference {
                    package: package.package.clone(),
                    item: target.clone(),
                },
            ) {
                builder.alias_conflicts.insert(name.clone());
            }
        }
    }
    for (target, qualified, binding) in pending_aliases {
        builder.alias(
            &target,
            &qualified,
            binding.kind,
            &[evidence(&binding.source, SourceRole::Export)],
            &visible,
            &mut BTreeSet::new(),
        );
    }
    for name in builder.alias_conflicts.clone() {
        let mut sources: Vec<_> = builder
            .items
            .values()
            .flat_map(|item| {
                let mut sources = item
                    .aliases
                    .iter()
                    .filter(|a| a.qualified_name == name)
                    .flat_map(|alias| alias.sources.iter().map(|s| s.source.clone()))
                    .collect::<Vec<_>>();
                if item.qualified_name == name {
                    sources.extend(item.source_location.clone());
                }
                sources
            })
            .collect();
        sources.sort_by(|a, b| {
            (&a.repository, &a.path, a.span.map(|s| (s.start, s.end))).cmp(&(
                &b.repository,
                &b.path,
                b.span.map(|s| (s.start, s.end)),
            ))
        });
        if let Some(source) = sources.first() {
            let count = match builder.registry.resolve_alias(&name) {
                Err(IdentityError::ConflictingAlias { candidates, .. }) => candidates.len(),
                _ => 0,
            };
            builder.diagnostic(
                DiagnosticCode::PythonConflictingAlias,
                Severity::Error,
                format!("Lookup name {name} has {count} conflicting canonical targets."),
                source,
            );
        }
        for item in builder.items.values_mut() {
            item.aliases.retain(|alias| alias.qualified_name != name);
        }
    }
    for (id, visibility) in &visible {
        if let Some(item) = builder.items.get_mut(id) {
            python_mut(item).visibility = *visibility;
        }
    }
    builder.items.retain(|id, _| visible.contains_key(id));
    builder.docstrings.retain(|id, _| visible.contains_key(id));
    let retained: BTreeSet<_> = builder.items.keys().cloned().collect();
    for item in builder.items.values_mut() {
        item.children.retain(|id| retained.contains(id));
        item.aliases.sort_by(|a, b| {
            a.qualified_name
                .cmp(&b.qualified_name)
                .then_with(|| alias_rank(a.kind).cmp(&alias_rank(b.kind)))
        });
        for alias in &mut item.aliases {
            normalize_evidence(&mut alias.sources);
        }
    }
    builder.diagnostics.sort();
    builder.diagnostics.dedup();
    PythonSurface {
        items: builder.items,
        docstrings: builder.docstrings,
        diagnostics: builder.diagnostics,
    }
}

impl Builder<'_> {
    fn diagnostic(
        &mut self,
        code: DiagnosticCode,
        severity: Severity,
        message: String,
        source: &SourceLocation,
    ) {
        let mut diagnostic = Diagnostic::new(code, severity, message)
            .with_source(DiagnosticSource::Repository {
                repository: source.repository.clone(),
                path: source.path.clone(),
            })
            .with_entity(DiagnosticEntity::Package {
                id: self.package.into(),
            });
        diagnostic.span = source.span;
        self.diagnostics.push(diagnostic);
    }

    fn identity(
        &mut self,
        name: &str,
        kind: PythonIdentityKind,
        source: &SourceLocation,
    ) -> Option<String> {
        let result =
            SemanticIdentity::python(name, kind).and_then(|id| self.registry.register(&id));
        match result {
            Ok(reference) => Some(reference.item),
            Err(_) => {
                self.diagnostic(
                    DiagnosticCode::PythonInvalidIdentity,
                    Severity::Error,
                    format!("Cannot establish a unique Python identity for {name}."),
                    source,
                );
                None
            }
        }
    }

    fn expose(
        &self,
        id: &str,
        visibility: PythonVisibility,
        visible: &mut BTreeMap<String, PythonVisibility>,
        visited: &mut BTreeSet<String>,
    ) {
        if !visited.insert(id.into()) {
            return;
        }
        visible
            .entry(id.into())
            .and_modify(|old| {
                if visibility == PythonVisibility::Public || *old == PythonVisibility::Private {
                    *old = visibility;
                }
            })
            .or_insert(visibility);
        let item = &self.items[id];
        if let PythonDeclaration::Callable {
            role: PythonCallableRole::Family { overloads },
            ..
        } = &python(item).declaration
        {
            for overload in overloads {
                self.expose(&overload.item, visibility, visible, visited);
            }
        }
        if let PythonDeclaration::Class {
            constructor: PythonConstructor::Dataclass { fields, .. },
            ..
        } = &python(item).declaration
        {
            for field in fields {
                if self.items[&field.item].name.starts_with('_') {
                    self.expose(&field.item, PythonVisibility::Private, visible, visited);
                }
            }
        }
        if item.kind == ItemKind::Class {
            for child in &item.children {
                let member = &self.items[child];
                if !member.name.starts_with('_')
                    || matches!(
                        python(member).declaration,
                        PythonDeclaration::Callable {
                            binding: PythonCallableKind::Constructor,
                            ..
                        }
                    )
                {
                    self.expose(child, visibility, visible, visited);
                }
            }
        }
    }

    fn alias(
        &mut self,
        target: &str,
        name: &str,
        kind: ItemAliasKind,
        sources: &[SourceEvidence],
        visible: &BTreeMap<String, PythonVisibility>,
        visited: &mut BTreeSet<String>,
    ) {
        if !visited.insert(target.into()) {
            return;
        }
        let reference = ItemReference {
            package: self.package.into(),
            item: target.into(),
        };
        match self.registry.bind_alias(name, &reference) {
            Ok(()) => {}
            Err(IdentityError::ConflictingAlias { .. }) => {
                self.alias_conflicts.insert(name.into());
            }
            Err(error) => {
                if let Some(source) = sources.first() {
                    self.diagnostic(
                        DiagnosticCode::PythonInvalidIdentity,
                        Severity::Error,
                        error.to_string(),
                        &source.source,
                    );
                }
                return;
            }
        }
        let item = self.items.get_mut(target).unwrap();
        if item.qualified_name != name {
            if let Some(alias) = item
                .aliases
                .iter_mut()
                .find(|a| a.qualified_name == name && a.kind == kind)
            {
                alias.sources.extend_from_slice(sources);
            } else {
                item.aliases.push(ItemAlias {
                    qualified_name: name.into(),
                    kind,
                    sources: sources.into(),
                });
            }
        }
        let children = if item.kind == ItemKind::Class {
            item.children
                .clone()
                .into_iter()
                .map(|id| (self.items[&id].name.clone(), id))
                .collect()
        } else if item.kind == ItemKind::Module {
            self.module_members.get(target).cloned().unwrap_or_default()
        } else {
            vec![]
        };
        for (local, child) in children {
            if visible
                .get(&child)
                .is_some_and(|visibility| *visibility != PythonVisibility::Private)
            {
                let child_name = format!("{name}.{local}");
                self.alias(&child, &child_name, kind, sources, visible, visited);
            }
        }
        visited.remove(target);
    }
}

fn new_item(
    kind: ItemKind,
    name: &str,
    qualified_name: &str,
    declaration: PythonDeclaration,
    source: &SourceLocation,
) -> Item {
    Item {
        kind,
        name: name.into(),
        qualified_name: qualified_name.into(),
        language_data: Some(ItemLanguageData::Python(PythonItemData {
            visibility: PythonVisibility::Private,
            declaration,
            decorators: vec![],
        })),
        aliases: vec![],
        signatures: vec![],
        documentation: None,
        source_location: Some(source.clone()),
        children: vec![],
        provenance: vec![],
    }
}
fn python(item: &Item) -> &PythonItemData {
    let Some(ItemLanguageData::Python(data)) = &item.language_data else {
        unreachable!()
    };
    data
}
fn python_mut(item: &mut Item) -> &mut PythonItemData {
    let Some(ItemLanguageData::Python(data)) = &mut item.language_data else {
        unreachable!()
    };
    data
}
fn evidence(source: &SourceLocation, role: SourceRole) -> SourceEvidence {
    SourceEvidence {
        source: source.clone(),
        role,
        parsers: ["ruff_python_parser".into()].into(),
    }
}
fn provenance(source: &SourceLocation) -> Provenance {
    Provenance {
        activity: ProvenanceActivity::Declaration,
        source: Some(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        }),
        span: source.span,
        tools: [("ruff_python_parser".into(), "0.0.12".into())].into(),
    }
}
fn normalize_provenance(values: &mut Vec<Provenance>) {
    values.sort_by(|a, b| {
        a.source.cmp(&b.source).then_with(|| {
            a.span
                .map(|s| (s.start, s.end))
                .cmp(&b.span.map(|s| (s.start, s.end)))
        })
    });
    values.dedup();
}
fn normalize_evidence(values: &mut Vec<SourceEvidence>) {
    values.sort_by(|a, b| {
        (
            &a.source.repository,
            &a.source.path,
            a.source.span.map(|s| (s.start, s.end)),
        )
            .cmp(&(
                &b.source.repository,
                &b.source.path,
                b.source.span.map(|s| (s.start, s.end)),
            ))
    });
    values.dedup();
}
fn alias_rank(kind: ItemAliasKind) -> u8 {
    match kind {
        ItemAliasKind::PythonReexport => 0,
        ItemAliasKind::PythonAssignment => 1,
        ItemAliasKind::RdAlias => 2,
    }
}
