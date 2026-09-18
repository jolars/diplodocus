use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    IdentityRegistry, Item, ItemAlias, ItemAliasKind, ItemKind, ItemLanguageData, RDeclaration,
    RGenericReference, RItemData, SemanticIdentity, SourceLocation, SourceRole,
};

use super::{
    diagnostic, evidence,
    namespace::Namespace,
    provenance,
    source::{Definition, DefinitionValue},
};

pub(super) fn reconcile(
    package: &str,
    definitions: &[Definition],
    namespace: &Namespace,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, Item> {
    let mut result = BTreeMap::new();
    if !namespace.valid {
        return result;
    }
    let mut bindings = BTreeMap::<&str, Vec<&Definition>>::new();
    for definition in definitions {
        bindings
            .entry(&definition.name)
            .or_default()
            .push(definition);
    }
    let mut selected = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, source) in namespace
        .exports
        .iter()
        .map(|(n, s)| (n, &s[0]))
        .chain(namespace.methods.iter().map(|m| (&m.binding, &m.source)))
    {
        match resolve(name, &bindings, &mut BTreeSet::new()) {
            Some(definition) => {
                selected
                    .entry(definition.name.clone())
                    .or_default()
                    .insert(name.clone());
            }
            None => diagnostics.push(diagnostic(
                DiagnosticCode::RUnresolvedDefinition,
                Severity::Error,
                format!("No unambiguous maintained function defines `{name}`."),
                source,
            )),
        }
    }
    // Local generics are needed for method identity even without a name export.
    for method in &namespace.methods {
        if let Some(definition) = resolve(&method.generic, &bindings, &mut BTreeSet::new()) {
            selected
                .entry(definition.name.clone())
                .or_default()
                .insert(method.generic.clone());
        }
    }
    let mut registry = IdentityRegistry::new(package).expect("configured package ID");
    let mut identities = BTreeMap::new();
    for name in selected.keys() {
        let definition = resolve(name, &bindings, &mut BTreeSet::new()).unwrap();
        let DefinitionValue::Function { declaration, .. } = &definition.value else {
            unreachable!()
        };
        if namespace.methods.iter().any(|m| {
            resolve(&m.binding, &bindings, &mut BTreeSet::new()).is_some_and(|d| d.name == *name)
        }) {
            continue;
        }
        let identity = if matches!(declaration, RDeclaration::S3Generic { .. }) {
            SemanticIdentity::r_s3_generic(name)
        } else {
            SemanticIdentity::r_function(name)
        };
        if let Ok(reference) = identity.and_then(|id| registry.register(&id)) {
            identities.insert(name.clone(), reference);
        }
    }
    let mut method_data = BTreeMap::<String, RDeclaration>::new();
    let mut conflicted = BTreeSet::new();
    let mut registrations = vec![];
    for method in &namespace.methods {
        let Some(definition) = resolve(&method.binding, &bindings, &mut BTreeSet::new()) else {
            continue;
        };
        let generic = if let Some((pkg, name)) = method.generic.split_once("::") {
            Some(RGenericReference::External {
                package: pkg.into(),
                name: name.into(),
            })
        } else if let Some(generic) = resolve(&method.generic, &bindings, &mut BTreeSet::new()) {
            if matches!(
                &generic.value,
                DefinitionValue::Function {
                    declaration: RDeclaration::S3Generic { .. },
                    ..
                }
            ) {
                identities
                    .get(&generic.name)
                    .cloned()
                    .map(|item| RGenericReference::Workspace { item })
            } else {
                None
            }
        } else {
            namespace
                .imports
                .get(&method.generic)
                .filter(|packages| packages.len() == 1)
                .map(|packages| RGenericReference::External {
                    package: packages.first().unwrap().clone(),
                    name: method.generic.clone(),
                })
        };
        let Some(generic) = generic else {
            diagnostics.push(diagnostic(DiagnosticCode::RUnresolvedDefinition, Severity::Error, format!("Cannot resolve the S3 generic `{}` statically; use an explicit import or qualified registration.", method.generic), &method.source));
            continue;
        };
        registrations.push((method, definition, generic));
    }
    let mut dispatch_bindings = BTreeMap::<_, BTreeSet<String>>::new();
    for (method, definition, generic) in &registrations {
        dispatch_bindings
            .entry(dispatch_key(generic, &method.class))
            .or_default()
            .insert(definition.name.clone());
    }
    for (method, definition, generic) in registrations {
        if dispatch_bindings[&dispatch_key(&generic, &method.class)].len() > 1 {
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                format!(
                    "Multiple bindings register S3 method `{}.{}`.",
                    method.generic, method.class
                ),
                &method.source,
            ));
            conflicted.insert(definition.name.clone());
            continue;
        }
        if let Some(previous) = method_data.get_mut(&definition.name) {
            if let RDeclaration::S3Method {
                generic: previous_generic,
                class,
                registration,
            } = previous
                && previous_generic == &generic
                && class == &method.class
            {
                registration.push(evidence(&method.source, SourceRole::Registration));
                continue;
            }
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                format!("Conflicting S3 registrations for `{}`.", definition.name),
                &method.source,
            ));
            conflicted.insert(definition.name.clone());
            continue;
        }
        let identity = SemanticIdentity::r_s3_method(&definition.name, &generic, &method.class)
            .and_then(|id| registry.register(&id));
        match identity {
            Ok(reference) => {
                identities.insert(definition.name.clone(), reference);
                method_data.insert(
                    definition.name.clone(),
                    RDeclaration::S3Method {
                        generic,
                        class: method.class.clone(),
                        registration: vec![evidence(&method.source, SourceRole::Registration)],
                    },
                );
            }
            Err(_) => diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                "Conflicting canonical R identity.",
                &method.source,
            )),
        }
    }
    for name in conflicted {
        identities.remove(&name);
        method_data.remove(&name);
    }
    for (name, names) in selected {
        let Some(reference) = identities.get(&name) else {
            continue;
        };
        let definition = resolve(&name, &bindings, &mut BTreeSet::new()).unwrap();
        let DefinitionValue::Function {
            signature,
            declaration,
        } = &definition.value
        else {
            unreachable!()
        };
        let mut declaration = method_data
            .remove(&name)
            .unwrap_or_else(|| declaration.clone());
        if let RDeclaration::S3Generic { methods, .. } = &mut declaration {
            for method in &namespace.methods {
                if resolve(&method.generic, &bindings, &mut BTreeSet::new())
                    .is_some_and(|d| d.name == name)
                    && let Some(definition) =
                        resolve(&method.binding, &bindings, &mut BTreeSet::new())
                    && let Some(reference) = identities.get(&definition.name)
                    && !methods.contains(reference)
                {
                    methods.push(reference.clone());
                }
            }
        }
        let mut item = Item {
            kind: if matches!(declaration, RDeclaration::S3Method { .. }) {
                ItemKind::Method
            } else {
                ItemKind::Function
            },
            name: name.clone(),
            qualified_name: name.clone(),
            language_data: Some(ItemLanguageData::R(RItemData {
                exported: names
                    .iter()
                    .any(|name| namespace.exports.contains_key(name)),
                declaration,
            })),
            aliases: vec![],
            signatures: vec![signature.as_ref().clone()],
            documentation: None,
            source_location: Some(definition.source.clone()),
            children: vec![],
            provenance: vec![provenance(&definition.source, false)],
        };
        registry
            .bind_alias(&name, reference)
            .expect("canonical spelling");
        for alias in names {
            if let Some(exports) = namespace.exports.get(&alias) {
                item.provenance
                    .extend(exports.iter().map(|source| provenance(source, false)));
            }
            if alias != name {
                let sources = alias_sources(&alias, &bindings);
                if registry.bind_alias(&alias, reference).is_err() {
                    diagnostics.push(diagnostic(
                        DiagnosticCode::RConflictingSurface,
                        Severity::Error,
                        format!("Ambiguous R alias `{alias}`."),
                        &definition.source,
                    ));
                }
                item.aliases.push(ItemAlias {
                    qualified_name: alias,
                    kind: ItemAliasKind::RAssignment,
                    sources: sources
                        .iter()
                        .map(|source| evidence(source, SourceRole::Definition))
                        .collect(),
                });
            }
        }
        result.insert(reference.item.clone(), item);
    }
    result
}

fn dispatch_key(generic: &RGenericReference, class: &str) -> (bool, String, String, String) {
    match generic {
        RGenericReference::Workspace { item } => {
            (false, item.package.clone(), item.item.clone(), class.into())
        }
        RGenericReference::External { package, name } => {
            (true, package.clone(), name.clone(), class.into())
        }
    }
}

fn resolve<'a>(
    name: &str,
    bindings: &BTreeMap<&str, Vec<&'a Definition>>,
    seen: &mut BTreeSet<String>,
) -> Option<&'a Definition> {
    if !seen.insert(name.into()) {
        return None;
    }
    let candidates = bindings.get(name)?;
    let [definition] = candidates.as_slice() else {
        return None;
    };
    match &definition.value {
        DefinitionValue::Function { .. } => Some(definition),
        DefinitionValue::Alias(target) => resolve(target, bindings, seen),
        DefinitionValue::Unsupported => None,
    }
}

fn alias_sources(name: &str, bindings: &BTreeMap<&str, Vec<&Definition>>) -> Vec<SourceLocation> {
    let mut sources = vec![];
    let mut current = name;
    while let Some(definitions) = bindings.get(current) {
        let [definition] = definitions.as_slice() else {
            break;
        };
        if let DefinitionValue::Alias(target) = &definition.value {
            sources.push(definition.source.clone());
            current = target;
        } else {
            break;
        }
    }
    sources
}
