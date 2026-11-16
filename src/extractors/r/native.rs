//! Native namespace facts supply semantics; source parsing supplies proven locations.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::ir::{
    ExtractionMode, IdentityRegistry, ItemAlias, ItemAliasKind, ItemKind, ItemLanguageData,
    Parameter, ParameterKind, RDeclaration, RGenericReference, RItemData, SemanticIdentity,
    Signature, SignatureExpression, SourcedSignature,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeObservation {
    pub(crate) r_version: String,
    pub(crate) pkgload_version: String,
    pub(crate) package_name: String,
    pub(crate) package_version: String,
    pub(crate) dependencies: BTreeMap<String, String>,
    pub(crate) exports: Vec<String>,
    pub(crate) bindings: Vec<NativeBinding>,
    pub(crate) methods: Vec<NativeMethod>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeBinding {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) formals: Vec<NativeFormal>,
    pub(crate) generic: Option<String>,
    #[serde(default)]
    pub(crate) same_as: Vec<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeFormal {
    pub(crate) name: String,
    pub(crate) required: bool,
    pub(crate) default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeMethod {
    pub(crate) generic: String,
    pub(crate) class: String,
    pub(crate) binding: String,
    pub(crate) package: Option<String>,
}

fn failure(result: &mut RExtraction, message: impl Into<String>) {
    result.diagnostics.push(
        Diagnostic::new(
            DiagnosticCode::RConflictingSurface,
            Severity::Error,
            message,
        )
        .with_entity(DiagnosticEntity::Target {
            package: result.package.clone(),
            id: result.target.target.clone(),
        }),
    );
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(char::is_control)
}

fn binding_failed(result: &mut RExtraction, binding: &NativeBinding) -> bool {
    if let Some(error) = &binding.error {
        failure(
            result,
            format!(
                "Cannot inspect native R binding `{}`: {error}",
                binding.name
            ),
        );
        true
    } else {
        false
    }
}

fn validate(observation: &NativeObservation, result: &RExtraction) -> Result<(), String> {
    let metadata = result
        .metadata
        .as_ref()
        .ok_or("R metadata is unavailable.")?;
    if observation.package_name != metadata.name
        || !crate::validation::r_versions_equal(&observation.package_version, &metadata.version)
        || observation.r_version.is_empty()
        || observation.pkgload_version.is_empty()
        || observation.dependencies.is_empty()
        || observation
            .dependencies
            .iter()
            .any(|(name, version)| !valid_name(name) || version.is_empty())
    {
        return Err("Native R package identity conflicts with DESCRIPTION.".into());
    }
    let mut exports = BTreeSet::new();
    if observation
        .exports
        .iter()
        .any(|name| !valid_name(name) || !exports.insert(name))
    {
        return Err("Native R exports contain invalid or duplicate names.".into());
    }
    let mut bindings = BTreeSet::new();
    for binding in &observation.bindings {
        if !valid_name(&binding.name)
            || !bindings.insert(&binding.name)
            || !matches!(
                binding.kind.as_str(),
                "function"
                    | "value"
                    | "unsupported-s4"
                    | "unsupported-r6"
                    | "unsupported-s7"
                    | "unavailable"
            )
            || (binding.kind == "unavailable") != binding.error.is_some()
            || (binding.kind != "function"
                && (!binding.formals.is_empty() || binding.generic.is_some()))
            || binding
                .generic
                .as_deref()
                .is_some_and(|name| !valid_name(name))
        {
            return Err("Native R bindings contain invalid or duplicate declarations.".into());
        }
        let mut formals = BTreeSet::new();
        for formal in &binding.formals {
            if !valid_name(&formal.name)
                || !formals.insert(&formal.name)
                || formal.required != formal.default.is_none()
                || (formal.name == "..." && !formal.required)
                || formal
                    .default
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
            {
                return Err(format!("Invalid native R formals for `{}`.", binding.name));
            }
        }
    }
    for binding in &observation.bindings {
        let mut aliases = BTreeSet::new();
        if binding
            .same_as
            .iter()
            .any(|name| name == &binding.name || !bindings.contains(name) || !aliases.insert(name))
        {
            return Err("Native R binding identity evidence contains invalid aliases.".into());
        }
    }
    for method in &observation.methods {
        if !valid_name(&method.generic)
            || !valid_name(&method.class)
            || !valid_name(&method.binding)
            || method
                .package
                .as_deref()
                .is_some_and(|name| !valid_name(name))
        {
            return Err("Native R S3 registrations contain invalid coordinates.".into());
        }
    }
    Ok(())
}

fn default_expression(text: &str) -> SignatureExpression {
    let parsed = arity_parser::parser::parse(text);
    if parsed.diagnostics.is_empty()
        && let Some(element) = parsed.cst.children_with_tokens().find(|element| {
            !arity_parser::ast::kinds::is_trivia(element.kind())
                && element.kind() != arity_parser::syntax::SyntaxKind::COMMENT
        })
    {
        return source::expression(element);
    }
    SignatureExpression::LanguageSpecific {
        language: "r".into(),
        name: "native-default".into(),
        children: vec![],
        source: Some(text.into()),
    }
}

fn signature(binding: &NativeBinding) -> Signature {
    if binding.kind != "function" {
        return Signature::Value {
            annotation: None,
            value: None,
        };
    }
    let mut after_dots = false;
    let parameters = binding
        .formals
        .iter()
        .map(|formal| {
            let kind = if formal.name == "..." {
                after_dots = true;
                ParameterKind::LanguageSpecific {
                    language: "r".into(),
                    name: "dots".into(),
                }
            } else if after_dots {
                ParameterKind::KeywordOnly
            } else {
                ParameterKind::PositionalOrKeyword
            };
            Parameter {
                name: formal.name.clone(),
                kind,
                annotation: None,
                default: formal.default.as_deref().map(default_expression),
            }
        })
        .collect();
    Signature::Callable {
        parameters,
        returns: None,
    }
}

fn definition<'a>(
    name: &str,
    definitions: &BTreeMap<&str, Vec<&'a source::Definition>>,
) -> Option<&'a source::Definition> {
    match definitions.get(name)?.as_slice() {
        [definition] => Some(*definition),
        _ => None,
    }
}

fn canonical_name<'a>(
    name: &'a str,
    definitions: &BTreeMap<&str, Vec<&'a source::Definition>>,
    bindings: &BTreeMap<&str, &'a NativeBinding>,
) -> &'a str {
    let initial = name;
    let Some(binding) = bindings.get(name) else {
        return initial;
    };
    let original = signature(binding);
    let mut seen = BTreeSet::new();
    let mut current = name;
    while seen.insert(current) {
        match definition(current, definitions).map(|definition| &definition.value) {
            Some(source::DefinitionValue::Alias(target)) => {
                let Some(target_binding) = bindings.get(target.as_str()) else {
                    return initial;
                };
                if signature(target_binding) != original
                    || target_binding.generic != binding.generic
                    || !binding.same_as.iter().any(|name| name == target)
                {
                    return initial;
                }
                current = target;
            }
            Some(source::DefinitionValue::Function { .. }) if binding.kind == "function" => {
                return current;
            }
            Some(source::DefinitionValue::Constant(_)) if binding.kind == "value" => {
                return current;
            }
            _ => return initial,
        }
    }
    initial
}

fn alias_sources(
    name: &str,
    canonical: &str,
    definitions: &BTreeMap<&str, Vec<&source::Definition>>,
) -> Vec<SourceEvidence> {
    let mut result = vec![];
    let mut name = name;
    let mut seen = BTreeSet::new();
    while name != canonical && seen.insert(name) {
        let Some(definition) = definition(name, definitions) else {
            break;
        };
        let source::DefinitionValue::Alias(target) = &definition.value else {
            break;
        };
        result.push(evidence(&definition.source, SourceRole::Definition));
        name = target;
    }
    result
}

pub(super) fn retain_generated_sources(
    result: &mut RExtraction,
    repository: &ResolvedRepositoryPaths,
    target: &ResolvedTargetPath,
) {
    for item in result.items.values_mut() {
        let Some(document) = &mut item.documentation else {
            continue;
        };
        let Some(raw) = &document.raw_source else {
            continue;
        };
        for name in raw
            .lines()
            .filter_map(|line| line.strip_prefix("% Please edit documentation in "))
            .flat_map(|names| names.split(','))
        {
            let name = Path::new(name.trim());
            if !name
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
            {
                continue;
            }
            let Ok(source) = repository.source_location(target.path.join(name), None) else {
                continue;
            };
            if result
                .inputs
                .get(&source.path)
                .is_some_and(|input| input.kind == "r-source")
            {
                let evidence = provenance(&source, false);
                if !document.provenance.contains(&evidence) {
                    document.provenance.push(evidence);
                }
            }
        }
    }
}

pub(super) fn reconcile(
    result: &mut RExtraction,
    definitions: &[source::Definition],
    namespace: &mut namespace::Namespace,
    topics: &[rd::Topic],
    libraries: &libraries::Libraries,
    observation: &NativeObservation,
) -> BTreeMap<String, Item> {
    if let Err(message) = validate(observation, result) {
        failure(result, message);
        namespace.valid = false;
        return BTreeMap::new();
    }
    namespace.valid = true;
    let old_exports = std::mem::take(&mut namespace.exports);
    for name in &observation.exports {
        namespace.exports.insert(
            name.clone(),
            old_exports.get(name).cloned().unwrap_or_default(),
        );
    }
    if let ProvenanceActivity::Extraction {
        mode,
        capabilities,
        parsers,
        ..
    } = &mut result.provenance.activity
    {
        *mode = ExtractionMode::Native;
        capabilities.extend(
            [
                "r.namespace.native",
                "r.formals.native",
                "r.generated-bindings",
            ]
            .map(str::to_owned),
        );
        for (name, version) in [
            ("R", &observation.r_version),
            ("pkgload", &observation.pkgload_version),
        ] {
            parsers.insert(
                name.into(),
                ParserProvenance {
                    version: version.clone(),
                    role: "r-native-discovery".into(),
                    settings: BTreeMap::new(),
                },
            );
            result.provenance.tools.insert(name.into(), version.clone());
        }
    }
    let bindings: BTreeMap<_, _> = observation
        .bindings
        .iter()
        .map(|binding| (binding.name.as_str(), binding))
        .collect();
    let mut source_bindings = BTreeMap::<&str, Vec<&source::Definition>>::new();
    for definition in definitions {
        source_bindings
            .entry(&definition.name)
            .or_default()
            .push(definition);
    }
    let canonical: BTreeMap<_, _> = bindings
        .keys()
        .map(|name| (*name, canonical_name(name, &source_bindings, &bindings)))
        .collect();
    let dataset_names: BTreeSet<_> = topics
        .iter()
        .filter(|topic| topic.valid && topic.dataset)
        .flat_map(|topic| &topic.aliases)
        .collect();
    let mut selected = BTreeMap::<&str, BTreeSet<&str>>::new();
    for name in observation.exports.iter().chain(
        topics
            .iter()
            .filter(|topic| topic.valid && !topic.package && !topic.dataset)
            .flat_map(|topic| &topic.aliases),
    ) {
        if dataset_names.contains(name) {
            continue;
        }
        if let Some(binding) = bindings.get(name.as_str())
            && binding_failed(result, binding)
        {
            continue;
        }
        if let Some(binding) = bindings.get(name.as_str())
            && let Some(system) = binding.kind.strip_prefix("unsupported-")
        {
            result.diagnostics.push(Diagnostic::new(
                DiagnosticCode::RUnsupportedSurface,
                Severity::Error,
                format!("R binding `{name}` uses the unsupported {} class system; native R extraction currently supports ordinary functions, values, and S3 dispatch.", system.to_uppercase()),
            ).with_entity(DiagnosticEntity::Target { package: result.package.clone(), id: result.target.target.clone() }));
            continue;
        }
        if let Some(canonical) = canonical.get(name.as_str()) {
            selected.entry(canonical).or_default().insert(name);
        } else if namespace.exports.contains_key(name) {
            failure(
                result,
                format!("Exported R binding `{name}` was not observed in the loaded namespace."),
            );
        }
    }
    for method in &observation.methods {
        if let Some(binding) = bindings.get(method.binding.as_str())
            && binding_failed(result, binding)
        {
            continue;
        }
        if let Some(canonical) = canonical.get(method.binding.as_str()) {
            if bindings[method.binding.as_str()].kind != "function" {
                failure(
                    result,
                    format!(
                        "Registered R S3 method `{}` is not callable.",
                        method.binding
                    ),
                );
                continue;
            }
            selected
                .entry(canonical)
                .or_default()
                .insert(&method.binding);
        } else {
            failure(
                result,
                format!(
                    "Registered R S3 method `{}` has no loaded binding.",
                    method.binding
                ),
            );
        }
        if method
            .package
            .as_ref()
            .is_none_or(|package| package == &observation.package_name)
            && let Some(canonical) = canonical.get(method.generic.as_str())
            && !binding_failed(result, bindings[method.generic.as_str()])
            && bindings[method.generic.as_str()].kind == "function"
        {
            selected
                .entry(canonical)
                .or_default()
                .insert(&method.generic);
        }
    }
    let method_names: BTreeSet<_> = observation
        .methods
        .iter()
        .filter_map(|method| canonical.get(method.binding.as_str()).copied())
        .collect();
    let mut registry = IdentityRegistry::new(&result.package).expect("configured package ID");
    let mut identities = BTreeMap::new();
    let mut declarations = BTreeMap::new();
    for name in selected
        .keys()
        .copied()
        .filter(|name| !method_names.contains(name))
    {
        let binding = bindings[name];
        let maintained = definition(name, &source_bindings);
        let declaration = if binding.kind == "value" {
            RDeclaration::Constant
        } else if let Some(dispatch_name) = &binding.generic {
            let dispatch_object = maintained.and_then(|definition| match &definition.value {
                source::DefinitionValue::Function {
                    declaration:
                        RDeclaration::S3Generic {
                            dispatch_name: old_name,
                            dispatch_object,
                            ..
                        },
                    ..
                } if old_name == dispatch_name => dispatch_object.clone(),
                _ => None,
            });
            RDeclaration::S3Generic {
                dispatch_name: dispatch_name.clone(),
                dispatch_object,
                methods: vec![],
            }
        } else {
            maintained
                .and_then(|definition| match &definition.value {
                    source::DefinitionValue::Function {
                        declaration: declaration @ RDeclaration::Constructor { .. },
                        ..
                    } => Some(declaration.clone()),
                    _ => None,
                })
                .unwrap_or(RDeclaration::Function)
        };
        let identity = match &declaration {
            RDeclaration::Constant => SemanticIdentity::r_constant(name),
            RDeclaration::S3Generic { .. } => SemanticIdentity::r_s3_generic(name),
            _ => SemanticIdentity::r_function(name),
        };
        match identity.and_then(|identity| registry.register(&identity)) {
            Ok(reference) => {
                identities.insert(name, reference);
                declarations.insert(name, declaration);
            }
            Err(_) => failure(result, format!("Invalid native R identity for `{name}`.")),
        }
    }
    let mut dispatches = BTreeMap::new();
    let mut conflicted = BTreeSet::new();
    for method in &observation.methods {
        let Some(name) = canonical.get(method.binding.as_str()).copied() else {
            continue;
        };
        if bindings[name].kind != "function" {
            continue;
        }
        let generic = if method
            .package
            .as_ref()
            .is_none_or(|package| package == &observation.package_name)
            && let Some(local_name) = canonical.get(method.generic.as_str())
            && let Some(reference) = identities.get(local_name)
            && matches!(
                declarations.get(local_name),
                Some(RDeclaration::S3Generic { .. })
            ) {
            Some(RGenericReference::Workspace {
                item: reference.clone(),
            })
        } else if let Some(package) = method
            .package
            .as_ref()
            .filter(|package| *package != &observation.package_name)
        {
            Some(RGenericReference::External {
                package: package.clone(),
                name: method.generic.clone(),
            })
        } else {
            namespace
                .imports
                .get(&method.generic)
                .filter(|packages| packages.len() == 1)
                .map(|packages| RGenericReference::External {
                    package: packages.first().unwrap().clone(),
                    name: method.generic.clone(),
                })
                .or_else(|| libraries.default_generic(&method.generic))
        };
        let Some(generic) = generic else {
            failure(
                result,
                format!(
                    "Cannot resolve the native R S3 generic `{}`.",
                    method.generic
                ),
            );
            continue;
        };
        let key =
            serde_json::to_string(&(&generic, &method.class)).expect("serializable S3 coordinate");
        if let Some(previous) = dispatches.insert(key, name)
            && previous != name
        {
            failure(
                result,
                format!(
                    "Multiple native bindings register S3 method `{}.{}`.",
                    method.generic, method.class
                ),
            );
            conflicted.insert(previous);
            conflicted.insert(name);
            continue;
        }
        let registration: Vec<_> = namespace
            .methods
            .iter()
            .filter(|registration| {
                registration.class == method.class
                    && registration.binding == method.binding
                    && registration
                        .generic
                        .split_once("::")
                        .map_or(registration.generic.as_str(), |(_, name)| name)
                        == method.generic
            })
            .map(|registration| evidence(&registration.source, SourceRole::Registration))
            .collect();
        let declaration = RDeclaration::S3Method {
            generic: generic.clone(),
            class: method.class.clone(),
            registration,
        };
        if let Some(previous) = declarations.get(name) {
            if !matches!(previous, RDeclaration::S3Method { generic: old_generic, class, .. } if old_generic == &generic && class == &method.class)
            {
                failure(
                    result,
                    format!("Conflicting native R S3 registrations for `{name}`."),
                );
                conflicted.insert(name);
            }
            continue;
        }
        match SemanticIdentity::r_s3_method(name, &generic, &method.class)
            .and_then(|identity| registry.register(&identity))
        {
            Ok(reference) => {
                identities.insert(name, reference);
                declarations.insert(name, declaration);
            }
            Err(_) => failure(
                result,
                format!("Invalid native R S3 identity for `{name}`."),
            ),
        }
    }
    for name in conflicted {
        identities.remove(name);
        declarations.remove(name);
    }
    let mut items = BTreeMap::new();
    for (name, aliases) in selected {
        let (Some(reference), Some(declaration)) =
            (identities.get(name), declarations.remove(name))
        else {
            continue;
        };
        let binding = bindings[name];
        let maintained = definition(name, &source_bindings).filter(|definition| matches!(&definition.value, source::DefinitionValue::Function { .. } if binding.kind == "function") || matches!(&definition.value, source::DefinitionValue::Constant(_) if binding.kind == "value"));
        let mut native_signature = signature(binding);
        let mut sources = vec![];
        if let Some(definition) = maintained {
            match &definition.value {
                source::DefinitionValue::Function { signature, .. }
                    if rd::same_signature(&signature.signature, &native_signature) =>
                {
                    native_signature = signature.signature.clone();
                    sources = signature.sources.clone();
                }
                source::DefinitionValue::Constant(value) => {
                    native_signature = Signature::Value {
                        annotation: None,
                        value: Some(value.clone()),
                    };
                    sources.push(evidence(&definition.source, SourceRole::Signature));
                }
                _ => {}
            }
        }
        let mut item = Item {
            kind: match declaration {
                RDeclaration::Constant => ItemKind::Constant,
                RDeclaration::S3Method { .. } => ItemKind::Method,
                _ => ItemKind::Function,
            },
            name: name.into(),
            qualified_name: name.into(),
            language_data: Some(ItemLanguageData::R(RItemData {
                exported: aliases
                    .iter()
                    .any(|alias| namespace.exports.contains_key(*alias)),
                internal: false,
                help_topic: None,
                declaration,
            })),
            aliases: vec![],
            signatures: vec![SourcedSignature {
                signature: native_signature,
                sources,
            }],
            documentation: None,
            source_location: maintained.map(|definition| definition.source.clone()),
            children: vec![],
            provenance: maintained
                .map(|definition| provenance(&definition.source, false))
                .into_iter()
                .collect(),
        };
        for alias in aliases {
            if alias != name {
                item.aliases.push(ItemAlias {
                    qualified_name: alias.into(),
                    kind: ItemAliasKind::RAssignment,
                    sources: alias_sources(alias, name, &source_bindings),
                });
            }
            if let Some(sources) = namespace.exports.get(alias) {
                item.provenance
                    .extend(sources.iter().map(|source| provenance(source, false)));
            }
            if registry.bind_alias(alias, reference).is_err() {
                failure(result, format!("Conflicting native R alias `{alias}`."));
            }
        }
        items.insert(reference.item.clone(), item);
    }
    let method_links: Vec<_> = items
        .iter()
        .filter_map(|(id, item)| match &item.language_data {
            Some(ItemLanguageData::R(RItemData {
                declaration:
                    RDeclaration::S3Method {
                        generic: RGenericReference::Workspace { item: generic },
                        ..
                    },
                ..
            })) => Some((
                generic.item.clone(),
                crate::ir::ItemReference {
                    package: result.package.clone(),
                    item: id.clone(),
                },
            )),
            _ => None,
        })
        .collect();
    for (generic, method) in method_links {
        if let Some(Item {
            language_data:
                Some(ItemLanguageData::R(RItemData {
                    declaration: RDeclaration::S3Generic { methods, .. },
                    ..
                })),
            ..
        }) = items.get_mut(&generic)
        {
            methods.push(method);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    type BindingSpec<'a> = (&'a str, &'a str, &'a [(&'a str, Option<&'a str>)]);

    fn observe(bindings: &[BindingSpec<'_>], exports: &[&str]) -> NativeObservation {
        NativeObservation {
            r_version: "4.6.1".into(),
            pkgload_version: "1.4.1".into(),
            package_name: "nativepkg".into(),
            package_version: "1.0.1".into(),
            dependencies: BTreeMap::from([("base".into(), "4.6.1".into())]),
            exports: exports.iter().map(|name| (*name).into()).collect(),
            bindings: bindings
                .iter()
                .map(|(name, kind, formals)| NativeBinding {
                    name: (*name).into(),
                    kind: (*kind).into(),
                    generic: None,
                    same_as: vec![],
                    error: None,
                    formals: formals
                        .iter()
                        .map(|(name, default)| NativeFormal {
                            name: (*name).into(),
                            required: default.is_none(),
                            default: default.map(str::to_owned),
                        })
                        .collect(),
                })
                .collect(),
            methods: vec![],
        }
    }

    fn extract(
        source: &str,
        namespace: &str,
        topics: &[(&str, &str)],
        observation: &NativeObservation,
    ) -> RExtraction {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("R")).unwrap();
        fs::create_dir(root.path().join("man")).unwrap();
        fs::write(root.path().join("DESCRIPTION"), "Package: nativepkg\nVersion: 1.0-1\nTitle: Native package\nDescription: Native extraction.\n").unwrap();
        fs::write(root.path().join("NAMESPACE"), namespace).unwrap();
        fs::write(root.path().join("R/api.R"), source).unwrap();
        for (name, topic) in topics {
            fs::write(root.path().join("man").join(format!("{name}.Rd")), topic).unwrap();
        }
        let path = fs::canonicalize(root.path()).unwrap();
        let repository = ResolvedRepositoryPaths {
            id: "repo".into(),
            path: path.clone(),
        };
        let target = ResolvedTargetPath {
            id: "api".into(),
            path: path.clone(),
            julia_extensions: vec![],
        };
        let package = ResolvedPackagePaths {
            id: "native".into(),
            repository_index: 0,
            path: path.clone(),
            metadata_path: path.join("DESCRIPTION"),
            overview: None,
            targets: vec![target.clone()],
        };
        extract_native_target(&repository, &package, &target, observation)
    }

    fn no_errors(result: &RExtraction) {
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error),
            "{:?}",
            result.diagnostics
        );
    }

    fn item<'a>(result: &'a RExtraction, name: &str) -> &'a Item {
        result
            .items
            .values()
            .find(|item| item.name == name)
            .unwrap()
    }

    #[test]
    fn generated_functions_use_native_formals_and_roxygen_source_evidence() {
        let observation = observe(
            &[(
                "generated",
                "function",
                &[("x", None), ("...", None), ("option", Some("NULL"))],
            )],
            &["generated"],
        );
        let result = extract(
            "for (name in 'generated') assign(name, function(x, ..., option = NULL) x)",
            "exportPattern('generated')",
            &[(
                "generated",
                "% Generated by roxygen2: do not edit by hand\n% Please edit documentation in R/api.R\n\\name{generated}\\alias{generated}\\title{Generated function}\\usage{generated(x, ..., option = NULL)}\\description{Generated documentation.}",
            )],
            &observation,
        );
        no_errors(&result);
        let generated = item(&result, "generated");
        assert!(generated.source_location.is_none());
        assert!(generated.signatures[0].sources.is_empty());
        let Signature::Callable { parameters, .. } = &generated.signatures[0].signature else {
            panic!()
        };
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["x", "...", "option"]
        );
        assert_eq!(parameters[2].kind, ParameterKind::KeywordOnly);
        assert_eq!(
            parameters[2].default,
            Some(SignatureExpression::Literal {
                text: "NULL".into()
            })
        );
        let doc = generated.documentation.as_ref().unwrap();
        assert!(doc.provenance.iter().any(|record| matches!(&record.source, Some(DiagnosticSource::Repository { path, .. }) if path.as_str() == "R/api.R") && record.span.is_none()));
        assert!(matches!(
            result.provenance.activity,
            ProvenanceActivity::Extraction {
                mode: ExtractionMode::Native,
                ..
            }
        ));
    }

    #[test]
    fn changed_native_formals_and_exports_override_static_declarations() {
        let observation = observe(
            &[
                ("f", "function", &[("x", Some("2"))]),
                ("removed", "function", &[]),
            ],
            &["f"],
        );
        let result = extract(
            "f <- function(x = 1) x\nremoved <- function() NULL",
            "export(removed)",
            &[(
                "f",
                "\\name{f}\\alias{f}\\title{Function}\\usage{f(x = 2)}\\description{Native formals.}",
            )],
            &observation,
        );
        no_errors(&result);
        assert_eq!(result.items.len(), 1);
        let f = item(&result, "f");
        assert!(f.signatures[0].sources.is_empty());
        assert!(f.source_location.is_some());
        assert!(matches!(&f.language_data, Some(ItemLanguageData::R(data)) if data.exported));
    }

    #[test]
    fn native_numeric_defaults_preserve_equivalent_source_signatures() {
        let observation = observe(
            &[(
                "f",
                "function",
                &[
                    ("tolerance", Some("1e-08")),
                    ("real", Some("1")),
                    ("integer", Some("1L")),
                    ("empty", Some("NULL")),
                    ("mask", Some("255")),
                ],
            )],
            &["f"],
        );
        let result = extract(
            "f <- function(tolerance = 1e-8, real = 1.0, integer = 1L, empty = NULL, mask = 0xff) tolerance",
            "export(f)",
            &[(
                "f",
                "\\name{f}\\alias{f}\\title{Numeric defaults}\\usage{f(tolerance = 1e-8, real = 1.0, integer = 1L, empty = NULL, mask = 0xff)}\\description{Equivalent native defaults.}",
            )],
            &observation,
        );
        no_errors(&result);
        let signature = &item(&result, "f").signatures[0];
        assert!(!signature.sources.is_empty());
        let Signature::Callable { parameters, .. } = &signature.signature else {
            panic!()
        };
        for (parameter, expected) in parameters.iter().zip(["1e-8", "1.0", "1L", "NULL", "0xff"]) {
            assert_eq!(
                parameter.default,
                Some(SignatureExpression::Literal {
                    text: expected.into()
                })
            );
        }
    }

    #[test]
    fn native_integer_defaults_remain_distinct_from_doubles() {
        let observation = observe(&[("f", "function", &[("x", Some("1"))])], &["f"]);
        let result = extract(
            "f <- function(x = 1L) x",
            "export(f)",
            &[(
                "f",
                "\\name{f}\\alias{f}\\title{Function}\\usage{f(x = 1L)}\\description{Integer default.}",
            )],
            &observation,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error
                    && diagnostic.message.contains("formals"))
        );
        assert!(item(&result, "f").signatures[0].sources.is_empty());
    }

    #[test]
    fn unavailable_bindings_fail_only_when_selected_by_exports_topics_or_methods() {
        let mut observation = observe(
            &[
                ("f", "function", &[("x", None)]),
                ("unused", "unavailable", &[]),
            ],
            &["f"],
        );
        observation.bindings[1].error = Some("not initialized".into());
        let source = "f <- function(x) x";
        let result = extract(source, "export(f)", &[], &observation);
        no_errors(&result);
        assert_eq!(result.items.len(), 1);

        let topics = [(
            "f",
            "\\name{f}\\alias{f}\\alias{unused}\\title{Function}\\usage{f(x)}\\description{Shared help.}",
        )];
        let documented = extract(source, "export(f)", &topics, &observation);
        observation.exports.push("unused".into());
        let exported = extract(source, "export(f, unused)", &[], &observation);
        observation.exports.pop();
        observation.methods.push(NativeMethod {
            generic: "print".into(),
            class: "model".into(),
            binding: "unused".into(),
            package: Some("base".into()),
        });
        let registered = extract(source, "export(f)", &[], &observation);
        for result in [documented, exported, registered] {
            assert!(
                result.diagnostics.iter().any(|diagnostic| {
                    diagnostic.severity == Severity::Error
                        && diagnostic.message.contains("unused")
                        && diagnostic.message.contains("not initialized")
                }),
                "{:?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn proven_assignment_aliases_preserve_identity_without_merging_equal_functions() {
        let mut observation = observe(
            &[
                ("f", "function", &[("x", None)]),
                ("alias", "function", &[("x", None)]),
                ("g", "function", &[("x", None)]),
            ],
            &["alias", "g"],
        );
        observation.bindings[0].same_as.push("alias".into());
        observation.bindings[1].same_as.push("f".into());
        let result = extract(
            "f <- function(x) x\nalias <- f\ng <- function(x) x",
            "export(alias, g)",
            &[],
            &observation,
        );
        no_errors(&result);
        assert_eq!(result.items.len(), 2);
        let f = item(&result, "f");
        assert!(f.aliases.iter().any(|alias| alias.qualified_name == "alias"
            && alias.kind == ItemAliasKind::RAssignment
            && !alias.sources.is_empty()));
        assert!(!f.signatures[0].sources.is_empty());
        assert!(
            result
                .items
                .contains_key(SemanticIdentity::r_function("f").unwrap().item_id())
        );
        assert!(
            result
                .items
                .contains_key(SemanticIdentity::r_function("g").unwrap().item_id())
        );
    }

    #[test]
    fn runtime_rebindings_do_not_collapse_source_assignment_aliases() {
        let observation = observe(
            &[
                ("f", "function", &[("x", None)]),
                ("alias", "function", &[("x", None)]),
            ],
            &["f", "alias"],
        );
        let result = extract(
            "f <- function(x) x\nalias <- f",
            "export(f, alias)",
            &[],
            &observation,
        );
        no_errors(&result);
        assert_eq!(result.items.len(), 2);
        assert!(
            result
                .items
                .contains_key(SemanticIdentity::r_function("alias").unwrap().item_id())
        );
        assert!(item(&result, "alias").source_location.is_none());
    }

    #[test]
    fn generated_s3_methods_retain_native_dispatch_and_shared_topics() {
        let mut observation = observe(
            &[
                ("f", "function", &[("x", None), ("...", None)]),
                ("implementation", "function", &[("x", None), ("...", None)]),
            ],
            &["f"],
        );
        observation.bindings[0].generic = Some("f".into());
        observation.methods.push(NativeMethod {
            generic: "f".into(),
            class: "model".into(),
            binding: "implementation".into(),
            package: None,
        });
        let result = extract(
            "f <- make_generic()\nimplementation <- make_method()",
            "export(f)\nS3method(f, model, implementation)",
            &[(
                "f",
                "\\name{f}\\alias{f}\\alias{implementation}\\title{Model methods}\\usage{f(x, ...)\n\\method{f}{model}(x, ...)}\\description{Shared native documentation.}",
            )],
            &observation,
        );
        no_errors(&result);
        let generic = item(&result, "f");
        let method = item(&result, "implementation");
        assert_eq!(method.kind, ItemKind::Method);
        assert!(method.source_location.is_none());
        assert!(
            matches!(&method.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Method { generic: RGenericReference::Workspace { .. }, registration, .. } if !registration.is_empty()))
        );
        assert!(
            matches!(&generic.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Generic { methods, .. } if methods.len() == 1))
        );
        assert_eq!(
            generic.documentation.as_ref().unwrap().raw_source,
            method.documentation.as_ref().unwrap().raw_source
        );
    }

    #[test]
    fn native_external_methods_keep_coordinates_and_static_constructors() {
        let mut observation = observe(
            &[
                ("model", "function", &[("x", None)]),
                ("print.model", "function", &[("x", None), ("...", None)]),
                ("makeContent.model", "function", &[("x", None)]),
            ],
            &["model"],
        );
        observation.methods = vec![
            NativeMethod {
                generic: "print".into(),
                class: "model".into(),
                binding: "print.model".into(),
                package: None,
            },
            NativeMethod {
                generic: "makeContent".into(),
                class: "model".into(),
                binding: "makeContent.model".into(),
                package: Some("grid".into()),
            },
        ];
        let result = extract(
            "model <- function(x) structure(x, class = 'model')\nprint.model <- function(x, ...) x\nmakeContent.model <- function(x) x",
            "export(model)\nimport(grid)\nS3method(print, model)\nS3method(makeContent, model)",
            &[],
            &observation,
        );
        no_errors(&result);
        assert!(
            matches!(&item(&result, "model").language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::Constructor { classes } if classes == &["model"]))
        );
        for (name, expected_package, expected_generic) in [
            ("print.model", "base", "print"),
            ("makeContent.model", "grid", "makeContent"),
        ] {
            assert!(
                matches!(&item(&result, name).language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration, RDeclaration::S3Method { generic: RGenericReference::External { package, name }, .. } if package == expected_package && name == expected_generic))
            );
        }
    }

    #[test]
    fn constants_keep_supported_values_and_datasets_need_no_native_binding() {
        let mut observation = observe(
            &[
                ("PAD", "value", &[]),
                ("alias", "value", &[]),
                ("generated", "value", &[]),
            ],
            &["alias", "generated", "dataset"],
        );
        observation.bindings[0].same_as.push("alias".into());
        observation.bindings[1].same_as.push("PAD".into());
        let result = extract(
            "PAD <- 3\nalias <- PAD\ngenerated <- local(42)",
            "export(alias, generated, dataset)",
            &[
                (
                    "dataset",
                    "\\docType{data}\\name{dataset}\\alias{dataset}\\title{Data}\\usage{dataset}\\description{Dataset documentation.}",
                ),
                (
                    "generated",
                    "\\name{generated}\\alias{generated}\\title{Generated object}\\usage{generated}\\description{Native object.}",
                ),
            ],
            &observation,
        );
        no_errors(&result);
        assert!(
            matches!(&item(&result, "PAD").signatures[0].signature, Signature::Value { value: Some(SignatureExpression::Literal { text }), .. } if text == "3")
        );
        assert!(matches!(
            &item(&result, "generated").signatures[0].signature,
            Signature::Value { value: None, .. }
        ));
        assert!(
            matches!(&item(&result, "dataset").language_data, Some(ItemLanguageData::R(data)) if data.exported && matches!(data.declaration, RDeclaration::Dataset))
        );
    }

    #[test]
    fn malformed_native_formals_and_missing_exports_are_errors() {
        let mut observation = observe(&[("f", "function", &[("x", None)])], &["f"]);
        let duplicate = observation.bindings[0].formals[0].clone();
        observation.bindings[0].formals.push(duplicate);
        let result = extract("", "", &[], &observation);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
        );
        let observation = observe(&[], &["missing"]);
        let result = extract("", "", &[], &observation);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("was not observed"))
        );
    }
}
