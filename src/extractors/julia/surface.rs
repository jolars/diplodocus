use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{DiagnosticCode as Code, Severity};
use crate::ir::*;

use super::source::{Declarations, Draft, DraftDeclaration, qualify};
use super::{JuliaExtraction, declaration_provenance, diagnostic, evidence, signatures};

pub(super) fn assemble(mut declarations: Declarations, result: &mut JuliaExtraction) {
    let package = result.package.clone();
    let modules: BTreeSet<_> = declarations
        .drafts
        .iter()
        .filter(|draft| {
            matches!(
                draft.declaration,
                DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Module,
                    ..
                }
            )
        })
        .map(|draft| qualify(&draft.module, &draft.name))
        .collect();
    let mut bindings = BTreeMap::new();
    let mut binding_sources = BTreeMap::new();
    for import in &declarations.imports {
        bind(
            &mut bindings,
            &mut binding_sources,
            qualify(&import.module, &import.name),
            import.target.clone(),
            &import.source,
            result,
        );
    }
    for draft in &declarations.drafts {
        if let DraftDeclaration::Alias { target } = &draft.declaration {
            let target = resolve(&draft.module, target, &bindings, &modules);
            bind(
                &mut bindings,
                &mut binding_sources,
                qualify(&draft.module, &draft.name),
                target,
                &draft.source,
                result,
            );
        }
    }
    // Expand only supplied local modules. An unavailable dependency's export
    // surface must never be inferred from a depot or from unrelated sources.
    for _ in 0..=declarations.imports.len() {
        let before = bindings.clone();
        for import in &declarations.imports {
            if !import.whole_using || !modules.contains(&import.target) {
                continue;
            }
            for export in declarations.exports.iter().filter(|export| {
                export.module == import.target && export.visibility == JuliaVisibility::Exported
            }) {
                let target = resolve(&export.module, &export.name, &bindings, &modules);
                bind(
                    &mut bindings,
                    &mut binding_sources,
                    qualify(&import.module, &export.name),
                    target,
                    &import.source,
                    result,
                );
            }
        }
        if bindings == before {
            break;
        }
    }
    let mut candidates = bindings.clone();
    for draft in &declarations.drafts {
        if let Some(target) = constant_alias(draft) {
            candidates.insert(
                qualify(&draft.module, &draft.name),
                resolve(&draft.module, &target, &bindings, &modules),
            );
        }
    }
    let callable_bindings: BTreeSet<_> = declarations
        .drafts
        .iter()
        .filter(|draft| {
            matches!(
                draft.declaration,
                DraftDeclaration::Callable { .. }
                    | DraftDeclaration::Generic
                    | DraftDeclaration::Named {
                        kind: JuliaIdentityKind::Type | JuliaIdentityKind::Module,
                        ..
                    }
            )
        })
        .map(|draft| canonical(draft, &bindings, &modules))
        .collect();
    for draft in &mut declarations.drafts {
        if let Some(target) = constant_alias(draft) {
            let target = resolve(&draft.module, &target, &candidates, &modules);
            if callable_bindings.contains(&target) {
                bind(
                    &mut bindings,
                    &mut binding_sources,
                    qualify(&draft.module, &draft.name),
                    target.clone(),
                    &draft.source,
                    result,
                );
                draft.declaration = DraftDeclaration::Alias { target };
            }
        }
    }
    let type_names: BTreeSet<_> = declarations
        .drafts
        .iter()
        .filter(|draft| {
            matches!(
                draft.declaration,
                DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Type,
                    ..
                }
            )
        })
        .map(|draft| qualify(&draft.module, &draft.name))
        .collect();
    let mut named = BTreeMap::new();
    let mut documented = BTreeSet::new();
    let mut containers = BTreeMap::new();
    for draft in &declarations.drafts {
        if matches!(
            draft.declaration,
            DraftDeclaration::Alias { .. } | DraftDeclaration::Documentation { .. }
        ) {
            continue;
        }
        let name = canonical(draft, &bindings, &modules);
        let kind = match &draft.declaration {
            DraftDeclaration::Named { kind, .. } => *kind,
            DraftDeclaration::Callable {
                macro_definition: true,
                ..
            } => JuliaIdentityKind::Macro,
            _ if type_names.contains(&name) => JuliaIdentityKind::Type,
            _ => JuliaIdentityKind::Function,
        };
        let identity = SemanticIdentity::julia(&name, kind).unwrap();
        if let Some(existing) = named.insert(name.clone(), identity.item_id().to_owned())
            && existing != identity.item_id()
        {
            conflict(result, &draft.source, &name);
        }
        containers
            .entry(name.clone())
            .or_insert(draft.module.clone());
        if draft.documented {
            documented.insert(name);
        }
    }
    for draft in &declarations.drafts {
        if matches!(
            draft.declaration,
            DraftDeclaration::Documentation { .. } | DraftDeclaration::Alias { .. }
        ) && draft.documented
        {
            documented.insert(resolve(&draft.module, &draft.name, &bindings, &modules));
        }
    }
    for draft in &declarations.drafts {
        if draft.documented && matches!(draft.declaration, DraftDeclaration::Alias { .. }) {
            let target = resolve(&draft.module, &draft.name, &bindings, &modules);
            if !named.contains_key(&target) {
                result.diagnostics.push(diagnostic(
                    Code::JuliaUnresolvedDefinition,
                    Severity::Error,
                    format!(
                        "Documented Julia alias `{}` has no supplied static definition.",
                        qualify(&draft.module, &draft.name)
                    ),
                    &draft.source,
                ));
            }
        }
    }
    let mut visible = documented;
    let mut visibility = BTreeMap::new();
    let mut export_sources: BTreeMap<String, Vec<SourceLocation>> = BTreeMap::new();
    for export in &declarations.exports {
        let name = resolve(&export.module, &export.name, &bindings, &modules);
        if named.contains_key(&name) {
            visible.insert(name.clone());
            let current = visibility
                .entry(name.clone())
                .or_insert(JuliaVisibility::Private);
            if *current != JuliaVisibility::Exported {
                *current = export.visibility;
            }
            export_sources
                .entry(name)
                .or_default()
                .push(export.source.clone());
        } else {
            result.diagnostics.push(diagnostic(
                Code::JuliaUnresolvedDefinition,
                Severity::Error,
                format!(
                    "Public Julia binding `{}` has no supplied static definition.",
                    qualify(&export.module, &export.name)
                ),
                &export.source,
            ));
        }
    }
    for module in &modules {
        if !module.contains('.') {
            visible.insert(module.clone());
        }
    }
    loop {
        let before = visible.clone();
        for name in &before {
            if let Some(module) = containers.get(name)
                && !module.is_empty()
            {
                visible.insert(module.clone());
            }
            if let Some((parent, _)) = name.rsplit_once('.')
                && type_names.contains(parent)
            {
                visible.insert(parent.into());
            }
        }
        for name in named.keys() {
            if let Some((parent, _)) = name.rsplit_once('.')
                && type_names.contains(parent)
                && visible.contains(parent)
            {
                visible.insert(name.clone());
            }
        }
        if before == visible {
            break;
        }
    }
    // Types must exist before their inner and outer constructor methods join.
    declarations
        .drafts
        .sort_by_key(|draft| !matches!(draft.declaration, DraftDeclaration::Named { .. }));
    for draft in &declarations.drafts {
        let name = canonical(draft, &bindings, &modules);
        if !visible.contains(&name) {
            continue;
        }
        let Some(id) = named.get(&name).cloned() else {
            continue;
        };
        match &draft.declaration {
            DraftDeclaration::Named {
                kind,
                declaration,
                signature,
            } => {
                let item_kind = match kind {
                    JuliaIdentityKind::Module => ItemKind::Module,
                    JuliaIdentityKind::Type => ItemKind::Type,
                    JuliaIdentityKind::Constant => ItemKind::Constant,
                    JuliaIdentityKind::Field => ItemKind::Field,
                    _ => ItemKind::Function,
                };
                let mut item = base(
                    draft,
                    &name,
                    item_kind,
                    declaration.clone(),
                    *visibility.get(&name).unwrap_or(&JuliaVisibility::Private),
                );
                if let Some(signature) = signature {
                    item.signatures.push(sourced(signature.clone(), draft));
                }
                if result.items.insert(id, item).is_some() {
                    conflict(result, &draft.source, &name);
                }
            }
            DraftDeclaration::Generic | DraftDeclaration::Callable { .. } => {
                let is_macro = matches!(
                    draft.declaration,
                    DraftDeclaration::Callable {
                        macro_definition: true,
                        ..
                    }
                );
                let constructor = type_names.contains(&name);
                let binding = if constructor {
                    JuliaCallableKind::Constructor
                } else if is_macro {
                    JuliaCallableKind::Macro
                } else {
                    JuliaCallableKind::Function
                };
                let owner = name.rsplit_once('.').map(|(module, _)| module);
                let external_owner = owner
                    .filter(|owner| !modules.contains(*owner))
                    .map(str::to_owned);
                if !result.items.contains_key(&id) {
                    result.items.insert(
                        id.clone(),
                        base(
                            draft,
                            &name,
                            ItemKind::Function,
                            JuliaDeclaration::Callable {
                                binding,
                                role: JuliaCallableRole::Family {
                                    methods: Vec::new(),
                                },
                                external_owner: external_owner.clone(),
                            },
                            *visibility.get(&name).unwrap_or(&JuliaVisibility::Private),
                        ),
                    );
                    // Method-specific prose stays on the method, never silently
                    // promoted to an unrelated generic declaration.
                    if matches!(
                        draft.declaration,
                        DraftDeclaration::Callable {
                            macro_definition: false,
                            ..
                        }
                    ) {
                        result.items.get_mut(&id).unwrap().documentation = None;
                    }
                }
                let DraftDeclaration::Callable { signature, .. } = &draft.declaration else {
                    let item = result.items.get_mut(&id).unwrap();
                    attach_document(item, draft, result.diagnostics.as_mut());
                    continue;
                };
                if is_macro {
                    let item = result.items.get_mut(&id).unwrap();
                    if !item.signatures.is_empty() {
                        conflict(result, &draft.source, &name);
                    } else {
                        item.signatures
                            .push(sourced(signature.signature.clone(), draft));
                    }
                    continue;
                }
                let identity = SemanticIdentity::julia_method(&name, &signature.dispatch).unwrap();
                let method_id = identity.item_id().to_owned();
                if result.items.contains_key(&method_id) {
                    conflict(result, &draft.source, &name);
                    continue;
                }
                let method_ref = identity.in_package(&package).unwrap();
                let family = ItemReference {
                    package: package.clone(),
                    item: id.clone(),
                };
                let mut method = base(
                    draft,
                    &name,
                    ItemKind::Method,
                    JuliaDeclaration::Callable {
                        binding,
                        role: JuliaCallableRole::Method {
                            family,
                            dispatch: signature.dispatch.clone(),
                        },
                        external_owner,
                    },
                    *visibility.get(&name).unwrap_or(&JuliaVisibility::Private),
                );
                method
                    .signatures
                    .push(sourced(signature.signature.clone(), draft));
                result.items.insert(method_id.clone(), method);
                let item = result.items.get_mut(&id).unwrap();
                item.children.push(method_id);
                item.signatures
                    .push(sourced(signature.signature.clone(), draft));
                if let Some(ItemLanguageData::Julia(data)) = &mut item.language_data {
                    match &mut data.declaration {
                        JuliaDeclaration::Callable {
                            role: JuliaCallableRole::Family { methods },
                            ..
                        } => methods.push(method_ref),
                        JuliaDeclaration::Type { constructors, .. } => {
                            constructors.push(method_ref)
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    for draft in &declarations.drafts {
        let selector = match &draft.declaration {
            DraftDeclaration::Documentation { selector } => selector.as_ref(),
            DraftDeclaration::Alias { .. } if draft.documented => None,
            _ => continue,
        };
        let name = resolve(&draft.module, &draft.name, &bindings, &modules);
        let id = if let Some(dispatch) = selector {
            SemanticIdentity::julia_method(&name, dispatch)
                .ok()
                .map(|id| id.item_id().to_owned())
        } else {
            named.get(&name).cloned()
        };
        if let Some(item) = id.and_then(|id| result.items.get_mut(&id)) {
            attach_document(item, draft, &mut result.diagnostics);
        } else {
            result.diagnostics.push(diagnostic(
                Code::JuliaUnresolvedDefinition,
                Severity::Error,
                format!("Documented Julia target `{name}` has no static declaration."),
                &draft.source,
            ));
        }
    }
    for (alias, target) in &bindings {
        let target = follow(target, &bindings);
        let Some(id) = named.get(&target) else {
            continue;
        };
        let Some(item) = result.items.get_mut(id) else {
            continue;
        };
        if alias == &target {
            continue;
        }
        if named.get(alias).is_some_and(|other| other != id) {
            if let Some(source) = binding_sources.get(alias) {
                result.diagnostics.push(diagnostic(
                    Code::JuliaConflictingSurface,
                    Severity::Error,
                    format!("Julia alias `{alias}` conflicts with a maintained declaration."),
                    source,
                ));
            }
            continue;
        }
        let is_assignment = declarations.drafts.iter().any(|draft| {
            matches!(draft.declaration, DraftDeclaration::Alias { .. })
                && qualify(&draft.module, &draft.name) == *alias
        });
        item.aliases.push(ItemAlias {
            qualified_name: alias.clone(),
            kind: if is_assignment {
                ItemAliasKind::JuliaAssignment
            } else {
                ItemAliasKind::JuliaImport
            },
            sources: binding_sources
                .get(alias)
                .map(|source| vec![evidence(source, SourceRole::Export)])
                .unwrap_or_default(),
        });
    }
    for (name, sources) in export_sources {
        if let Some(item) = named.get(&name).and_then(|id| result.items.get_mut(id)) {
            item.provenance
                .extend(sources.iter().map(declaration_provenance));
        }
    }
    let children: Vec<_> = result
        .items
        .iter()
        .filter(|(_, item)| item.kind != ItemKind::Method)
        .filter_map(|(id, item)| {
            let Some(ItemLanguageData::Julia(data)) = &item.language_data else {
                return None;
            };
            let parent = if item.kind == ItemKind::Field {
                item.qualified_name
                    .rsplit_once('.')
                    .map(|(parent, _)| parent.to_owned())
            } else {
                Some(data.defining_module.clone())
            };
            let parent_id = parent.and_then(|parent| named.get(&parent).cloned())?;
            (parent_id != *id).then(|| (parent_id, id.clone()))
        })
        .collect();
    for (parent, child) in children {
        if let Some(item) = result.items.get_mut(&parent) {
            item.children.push(child);
        }
    }
    let names: BTreeMap<_, _> = result
        .items
        .iter()
        .filter(|(_, item)| item.kind != ItemKind::Method)
        .flat_map(|(id, item)| {
            std::iter::once((item.qualified_name.clone(), id.clone())).chain(
                item.aliases
                    .iter()
                    .map(|alias| (alias.qualified_name.clone(), id.clone())),
            )
        })
        .collect();
    let existing: BTreeSet<_> = result.items.keys().cloned().collect();
    for item in result.items.values_mut() {
        let module = match &item.language_data {
            Some(ItemLanguageData::Julia(data)) => &data.defining_module,
            _ => continue,
        };
        if let Some(document) = &mut item.documentation {
            rewrite_references(&mut document.document.blocks, &mut |target| {
                if let Some((name, dispatch)) = signatures::selector(target) {
                    let name = resolve(module, &name, &bindings, &modules);
                    if let Ok(identity) = SemanticIdentity::julia_method(&name, &dispatch)
                        && existing.contains(identity.item_id())
                    {
                        return format!("{package}::{}", identity.item_id());
                    }
                }
                let name = resolve(module, target, &bindings, &modules);
                if let Some(id) = names.get(&name) {
                    format!("{package}::{id}")
                } else {
                    name
                }
            });
        }
    }
}

fn constant_alias(draft: &Draft) -> Option<String> {
    let DraftDeclaration::Named {
        kind: JuliaIdentityKind::Constant,
        signature: Some(Signature::Value {
            value: Some(value), ..
        }),
        ..
    } = &draft.declaration
    else {
        return None;
    };
    match value {
        SignatureExpression::Name { name, .. } => Some(name.clone()),
        SignatureExpression::LanguageSpecific {
            source: Some(source),
            ..
        } => {
            let parsed = fatou_parser::parser::parse(source);
            parsed
                .diagnostics
                .is_empty()
                .then(|| {
                    parsed
                        .cst
                        .children()
                        .next()
                        .and_then(|node| signatures::node_name(&node))
                })
                .flatten()
        }
        _ => None,
    }
}

fn base(
    draft: &Draft,
    name: &str,
    kind: ItemKind,
    declaration: JuliaDeclaration,
    visibility: JuliaVisibility,
) -> Item {
    Item {
        kind,
        name: draft.name.rsplit('.').next().unwrap().into(),
        qualified_name: name.into(),
        language_data: Some(ItemLanguageData::Julia(JuliaItemData {
            defining_module: draft.module.clone(),
            visibility,
            declaration,
        })),
        aliases: Vec::new(),
        signatures: Vec::new(),
        documentation: draft.documentation.clone(),
        source_location: Some(draft.source.clone()),
        children: Vec::new(),
        provenance: std::iter::once(&draft.source)
            .chain(&draft.wrappers)
            .map(declaration_provenance)
            .collect(),
    }
}

fn sourced(signature: Signature, draft: &Draft) -> SourcedSignature {
    SourcedSignature {
        signature,
        sources: std::iter::once(&draft.source)
            .chain(&draft.wrappers)
            .map(|source| evidence(source, SourceRole::Signature))
            .collect(),
    }
}

fn conflict(result: &mut JuliaExtraction, source: &SourceLocation, name: &str) {
    result.diagnostics.push(diagnostic(
        Code::JuliaConflictingSurface,
        Severity::Error,
        format!(
            "Julia declarations conflict at `{name}`; dispatch identity cannot distinguish them."
        ),
        source,
    ));
}

fn attach_document(
    item: &mut Item,
    draft: &Draft,
    diagnostics: &mut Vec<crate::diagnostics::Diagnostic>,
) {
    if let Some(document) = &draft.documentation {
        if item
            .documentation
            .as_ref()
            .is_some_and(|existing| existing != document)
        {
            diagnostics.push(diagnostic(
                Code::JuliaConflictingSurface,
                Severity::Error,
                "Multiple documentation attachments claim the same Julia declaration.",
                &draft.source,
            ));
        } else {
            item.documentation = Some(document.clone());
        }
    }
}

fn bind(
    bindings: &mut BTreeMap<String, String>,
    sources: &mut BTreeMap<String, SourceLocation>,
    name: String,
    target: String,
    source: &SourceLocation,
    result: &mut JuliaExtraction,
) {
    if bindings
        .get(&name)
        .is_some_and(|existing| existing != &target)
    {
        result.diagnostics.push(diagnostic(
            Code::JuliaConflictingSurface,
            Severity::Error,
            format!("Julia lookup name `{name}` has conflicting bindings."),
            source,
        ));
    } else {
        bindings.insert(name.clone(), target);
        sources.entry(name).or_insert(source.clone());
    }
}

fn follow(name: &str, bindings: &BTreeMap<String, String>) -> String {
    let mut current = name.to_owned();
    let mut seen = BTreeSet::new();
    while seen.insert(current.clone()) {
        let Some(next) = bindings.get(&current) else {
            break;
        };
        current = next.clone();
    }
    current
}

fn resolve(
    module: &str,
    name: &str,
    bindings: &BTreeMap<String, String>,
    modules: &BTreeSet<String>,
) -> String {
    if let Some(target) = bindings.get(&qualify(module, name)) {
        return follow(target, bindings);
    }
    if let Some((first, rest)) = name.split_once('.') {
        if let Some(target) = bindings.get(&qualify(module, first)) {
            return qualify(&follow(target, bindings), rest);
        }
        if modules.contains(&qualify(module, first)) {
            return qualify(module, name);
        }
        name.into()
    } else {
        qualify(module, name)
    }
}

fn canonical(
    draft: &Draft,
    bindings: &BTreeMap<String, String>,
    modules: &BTreeSet<String>,
) -> String {
    if matches!(draft.declaration, DraftDeclaration::Named { .. }) {
        qualify(&draft.module, &draft.name)
    } else {
        resolve(&draft.module, &draft.name, bindings, modules)
    }
}

fn rewrite_references(blocks: &mut [Block], rewrite: &mut impl FnMut(&str) -> String) {
    for block in blocks {
        match block {
            Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                rewrite_inlines(inlines, rewrite)
            }
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                rewrite_references(blocks, rewrite)
            }
            Block::List { items, .. } => {
                for item in items {
                    rewrite_references(&mut item.blocks, rewrite);
                }
            }
            Block::Table { caption, rows, .. } => {
                rewrite_inlines(caption, rewrite);
                for row in rows {
                    for cell in &mut row.cells {
                        rewrite_references(&mut cell.blocks, rewrite);
                    }
                }
            }
            _ => {}
        }
    }
}

fn rewrite_inlines(inlines: &mut [Inline], rewrite: &mut impl FnMut(&str) -> String) {
    for inline in inlines {
        match inline {
            Inline::SemanticReference { target, .. } => *target = rewrite(target),
            Inline::Link {
                target, inlines, ..
            } => {
                if let Some(spelling) = target.strip_prefix("@ref ") {
                    *target = format!("@ref {}", rewrite(spelling));
                }
                rewrite_inlines(inlines, rewrite);
            }
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. } => rewrite_inlines(inlines, rewrite),
            Inline::Image { alt, .. } => rewrite_inlines(alt, rewrite),
            _ => {}
        }
    }
}
