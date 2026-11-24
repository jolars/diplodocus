//! Normalize the checked-out Julia runtime's API and match authored evidence.

use crate::assembly::AssemblyError;

use super::signatures::{ParsedSignature, parse_header};
use super::source::{Declarations, Draft, DraftDeclaration, Export, Import, qualify};
use super::*;

const POLICY: &str = "julia-native-v1";
const CAPABILITIES: &[&str] = &[
    "diagnostics.unsupported-visible",
    "julia.declarations",
    "julia.docs.markdown",
    "julia.docs.native",
    "julia.docs.references",
    "julia.exports.native",
    "julia.extensions.static",
    "julia.includes.native",
    "julia.metadata.project",
    "julia.methods.native",
    "julia.reexports",
    "provenance.source",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    policy: String,
    julia_version: String,
    package_name: String,
    package_uuid: String,
    package_version: String,
    project_digest: String,
    dependencies: BTreeMap<String, String>,
    inputs: Vec<Input>,
    pub(crate) complete: bool,
    items: Vec<Binding>,
    exports: Vec<VisibleBinding>,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    path: DiagnosticPath,
    canonical: DiagnosticPath,
    module: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    name: String,
    qualified_name: String,
    module: String,
    kind: BindingKind,
    aliases: Vec<Alias>,
    docs: Vec<Doc>,
    methods: Vec<Method>,
    #[serde(rename = "type")]
    ty: Option<Type>,
    annotation: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BindingKind {
    Module,
    Type,
    Function,
    Macro,
    Constant,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    name: String,
    kind: AliasKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AliasKind {
    Assignment,
    Import,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibleBinding {
    module: String,
    name: String,
    visibility: JuliaVisibility,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Method {
    module: String,
    selector: String,
    display: String,
    types: Vec<String>,
    source_offsets: Vec<usize>,
    arguments: Vec<String>,
    keywords: Vec<String>,
    path: Option<DiagnosticPath>,
    line: Option<usize>,
    docs: Vec<Doc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    module: String,
    path: Option<DiagnosticPath>,
    line: Option<usize>,
    text: Option<String>,
    unsupported: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Type {
    flavor: String,
    mutable: bool,
    bits: usize,
    supertype: String,
    parameters: Vec<String>,
    fields: Vec<Field>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    name: String,
    #[serde(rename = "type")]
    annotation: String,
    #[serde(rename = "const")]
    is_const: bool,
    docs: Vec<Doc>,
}

impl Observation {
    /// Include both the spellings Julia used and their checked-out referents.
    pub(crate) fn input_paths(&self, package: &ResolvedPackagePaths) -> Vec<PathBuf> {
        std::iter::once(package.path.join("Project.toml"))
            .chain(
                self.inputs
                    .iter()
                    .map(|input| package.path.join(input.path.as_str())),
            )
            .collect()
    }

    pub(crate) fn verify(&self, package: &ResolvedPackagePaths) -> Result<(), AssemblyError> {
        let verify =
            |path: &Path, expected_digest: &str, matches_target: &dyn Fn(&Path) -> bool| {
                const CHECK: &str = "Julia worker input verification";
                let expected = Fingerprint {
                    algorithm: "sha256".into(),
                    value: expected_digest.into(),
                };
                let target = fs::canonicalize(path).map_err(|error| {
                    AssemblyError::inputs_changed(
                        CHECK,
                        format!(
                            "{}: {error}",
                            crate::assembly::fingerprint_difference(
                                path.display(),
                                Some(&expected),
                                None
                            )
                        ),
                    )
                })?;
                if !matches_target(&target) {
                    return Err(AssemblyError::fingerprint_changed(
                        CHECK,
                        format_args!("{} resolved to {}", path.display(), target.display()),
                        Some(&expected),
                        None,
                    ));
                }
                let observed = fs::read(path).ok().map(|bytes| fingerprint_bytes(&bytes));
                if observed.as_ref() != Some(&expected) {
                    return Err(AssemblyError::fingerprint_changed(
                        CHECK,
                        path.display(),
                        Some(&expected),
                        observed.as_ref(),
                    ));
                }
                Ok(())
            };
        verify(
            &package.path.join("Project.toml"),
            &self.project_digest,
            &|path| path.starts_with(&package.path),
        )?;
        for input in &self.inputs {
            let canonical = package.path.join(input.canonical.as_str());
            verify(
                &package.path.join(input.path.as_str()),
                &input.digest,
                &|path| path == canonical,
            )?;
        }
        Ok(())
    }
}

struct Sources<'a> {
    declarations: BTreeMap<String, Vec<Draft>>,
    files: BTreeMap<&'a DiagnosticPath, (SourceLocation, Vec<usize>)>,
    diagnostics: Vec<Diagnostic>,
    repository_path: &'a Path,
}

impl Sources<'_> {
    fn location(&self, path: Option<&DiagnosticPath>) -> Option<SourceLocation> {
        self.files.get(path?).map(|(location, _)| location.clone())
    }

    fn at_line(
        &self,
        source: &SourceLocation,
        path: Option<&DiagnosticPath>,
        line: Option<usize>,
    ) -> bool {
        let Some((location, offsets)) = path.and_then(|path| self.files.get(path)) else {
            return false;
        };
        let (Some(span), Some(index)) = (source.span, line.and_then(|line| line.checked_sub(1)))
        else {
            return false;
        };
        source.path == location.path
            && offsets.get(index).is_some_and(|start| {
                span.end > *start && offsets.get(index + 1).is_none_or(|end| span.start < *end)
            })
    }

    fn for_binding(&self, name: &str) -> impl Iterator<Item = &Draft> + Clone {
        self.declarations.get(name).into_iter().flatten()
    }

    fn documentation(
        &self,
        name: &str,
        docs: &[Doc],
        field: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<(Option<SourcedDocument>, Option<String>), String> {
        let mut docs = docs.iter();
        let Some(doc) = docs.next() else {
            return Ok((None, None));
        };
        if docs.any(|other| other.text != doc.text || other.unsupported != doc.unsupported) {
            return Err(format!(
                "Multiple Julia documentation attachments claim `{name}`."
            ));
        }
        let source = self.location(doc.path.as_ref());
        let Some(raw) = &doc.text else {
            let message = format!(
                "Julia documentation for `{name}`: {}",
                doc.unsupported
                    .as_deref()
                    .unwrap_or("Missing registered documentation text."),
            );
            diagnostics.push(match source.as_ref() {
                Some(source) => diagnostic(
                    DiagnosticCode::JuliaUnsupportedDocstring,
                    Severity::Warning,
                    message,
                    source,
                ),
                None => Diagnostic::new(
                    DiagnosticCode::JuliaUnsupportedDocstring,
                    Severity::Warning,
                    message,
                ),
            });
            return Ok((None, None));
        };
        let portable = raw.replace(
            self.repository_path.to_string_lossy().as_ref(),
            "<repository>",
        );
        let mapped = unique(self.for_binding(name).filter_map(|draft| {
            let parsed = draft.documentation.as_ref()?;
            let location = parsed.source_location.as_ref()?;
            let exact = parsed.raw_source.as_deref().is_some_and(|text| {
                text == portable || (text.contains('\r') && text.replace("\r\n", "\n") == portable)
            });
            (exact
                && (self.at_line(location, doc.path.as_ref(), doc.line)
                    || (field
                        && source
                            .as_ref()
                            .is_some_and(|source| source.path == location.path))))
            .then_some(parsed)
        }));
        let result = if let Some(document) = mapped {
            let location = document.source_location.as_ref().unwrap();
            diagnostics.extend(
                self.diagnostics
                    .iter()
                    .filter(|diag| {
                        diag.source.as_ref().is_some_and(|source| {
                            matches!(source,
                    DiagnosticSource::Repository { repository, path }
                    if repository == &location.repository && path == &location.path)
                        }) && diag.span.zip(location.span).is_some_and(|(part, whole)| {
                            part.start >= whole.start && part.end <= whole.end
                        })
                    })
                    .cloned(),
            );
            document.clone()
        } else {
            let parsed = docstrings::parse_runtime(&portable, source);
            diagnostics.extend(parsed.diagnostics);
            parsed.document
        };
        Ok((Some(result), Some(doc.module.clone())))
    }
}

fn unique<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn named(
    module: &str,
    name: &str,
    canonical: &str,
    declaration: DraftDeclaration,
    source: Option<SourceLocation>,
) -> Draft {
    Draft {
        module: module.into(),
        name: name.into(),
        canonical_name: Some(canonical.into()),
        declaration,
        documentation: None,
        documentation_module: None,
        documented: false,
        source,
        wrappers: Vec::new(),
    }
}

fn expression(name: &str) -> SignatureExpression {
    SignatureExpression::Name {
        name: name.into(),
        target: None,
    }
}

fn source_kind(draft: &Draft, kind: JuliaIdentityKind) -> bool {
    matches!(draft.declaration, DraftDeclaration::Named { kind: value, .. } if value == kind)
}

fn source_value(draft: &Draft) -> Option<Signature> {
    match &draft.declaration {
        DraftDeclaration::Named { signature, .. } => signature.clone(),
        _ => None,
    }
}

fn method_candidates<'a>(
    sources: &'a Sources<'_>,
    name: &'a str,
    method: &'a Method,
) -> impl Iterator<Item = &'a Draft> + Clone {
    sources.for_binding(name).filter(|draft| {
        matches!(draft.declaration, DraftDeclaration::Callable { .. })
            && draft.module == method.module
            && draft
                .source
                .as_ref()
                .is_some_and(|source| sources.at_line(source, method.path.as_ref(), method.line))
    })
}

fn positional(signature: &ParsedSignature) -> impl Iterator<Item = &Parameter> {
    signature.parameters.iter().filter(|parameter| {
        !matches!(
            parameter.kind,
            ParameterKind::KeywordOnly | ParameterKind::VariadicKeyword
        )
    })
}

fn method_header(draft: &Draft) -> &ParsedSignature {
    let DraftDeclaration::Callable { signature, .. } = &draft.declaration else {
        unreachable!("method attribution has a callable header");
    };
    signature
}

fn runtime_method(
    method: &Method,
    selector: Option<ParsedSignature>,
) -> Result<ParsedSignature, String> {
    let mut selector = selector
        .ok_or_else(|| format!("Unsupported Julia method selector `{}`.", method.selector))?;
    if method.arguments.len() != method.types.len() {
        return Err(format!(
            "Julia method arguments disagree with dispatch for `{}`.",
            method.selector
        ));
    }
    // Preserve reflected keyword names without asserting which ones have defaults.
    selector.signature = if method.keywords.is_empty() {
        parse_header(&method.display)
            .map(|header| header.signature)
            .unwrap_or_else(|| reflected_signature(&method.display))
    } else {
        reflected_signature(&method.display)
    };
    SemanticIdentity::julia_method(&selector.name, &selector.dispatch)
        .map_err(|e| e.to_string())?;
    Ok(selector)
}

fn reflected_signature(display: &str) -> Signature {
    Signature::LanguageSpecific {
        language: "julia".into(),
        syntax: SignatureExpression::LanguageSpecific {
            language: "julia".into(),
            name: "native-signature".into(),
            children: vec![],
            source: Some(display.into()),
        },
    }
}

fn with_doc(
    draft: &mut Draft,
    sources: &Sources<'_>,
    name: &str,
    docs: &[Doc],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), String> {
    let (doc, module) = sources.documentation(name, docs, false, diagnostics)?;
    draft.documented = !docs.is_empty();
    draft.documentation = doc;
    draft.documentation_module = module;
    Ok(())
}

fn binding_drafts(
    sources: &Sources<'_>,
    binding: &Binding,
    output: &mut Declarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), String> {
    let name = &binding.qualified_name;
    let mut methods = Vec::new();
    let constructor = matches!(binding.kind, BindingKind::Type);
    let macro_binding = matches!(binding.kind, BindingKind::Macro);
    let signatures: Vec<_> = binding
        .methods
        .iter()
        .map(|method| {
            let parsed = parse_header(&method.selector);
            let source = unique(method_candidates(sources, name, method).filter(|draft| {
                draft
                    .source
                    .as_ref()
                    .and_then(|source| source.span)
                    .is_some_and(|span| {
                        method
                            .source_offsets
                            .iter()
                            .any(|&offset| (span.start..span.end).contains(&offset))
                    })
            }));
            (method, source, parsed)
        })
        .collect();
    for (method, source, parsed) in &signatures {
        // Julia inserts positional-default forwarding methods on the same line.
        // A verified source header determines when they belong to one API method.
        if source.is_none()
            && method.docs.is_empty()
            && signatures.iter().any(|(full, source, _)| {
                source.is_some_and(|source| {
                    let parameters = positional(method_header(source));
                    let required = parameters
                        .filter(|parameter| parameter.default.is_none())
                        .count();
                    method.types.len() >= required
                        && full.types.len() > method.types.len()
                        && full.types.starts_with(&method.types)
                        && method.path.is_some()
                        && full.path == method.path
                        && full.line == method.line
                        && full.module == method.module
                })
            })
        {
            continue;
        }
        if constructor
            && source.is_none()
            && method.docs.is_empty()
            && !method_candidates(sources, name, method)
                .any(|draft| positional(method_header(draft)).count() == method.types.len())
        {
            continue;
        }
        let signature = match source {
            Some(source) => method_header(source).clone(),
            None => runtime_method(method, parsed.clone())?,
        };
        let mut draft = named(
            &method.module,
            &binding.name,
            name,
            DraftDeclaration::Callable {
                signature,
                macro_definition: macro_binding,
            },
            source
                .and_then(|draft| draft.source.clone())
                .or_else(|| sources.location(method.path.as_ref())),
        );
        if let Some(source) = source {
            draft.wrappers.clone_from(&source.wrappers);
        }
        let docs = if macro_binding {
            &binding.docs
        } else {
            &method.docs
        };
        with_doc(&mut draft, sources, name, docs, diagnostics)?;
        methods.push(draft);
    }
    if macro_binding {
        if methods.is_empty() {
            return Err(format!("Julia macro `{name}` has no retained method."));
        }
        output.drafts.extend(methods);
        return Ok(());
    }
    let mut fields = Vec::new();
    let (declaration, source) = match binding.kind {
        BindingKind::Module => {
            let source = unique(
                sources
                    .for_binding(name)
                    .filter(|draft| source_kind(draft, JuliaIdentityKind::Module)),
            );
            let bare = source.is_some_and(|draft| {
                matches!(
                    draft.declaration,
                    DraftDeclaration::Named {
                        declaration: JuliaDeclaration::Module { bare: true },
                        ..
                    }
                )
            });
            (
                DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Module,
                    declaration: JuliaDeclaration::Module { bare },
                    signature: None,
                },
                source,
            )
        }
        BindingKind::Constant => {
            let source = unique(
                sources
                    .for_binding(name)
                    .filter(|draft| source_kind(draft, JuliaIdentityKind::Constant)),
            );
            let signature = source.and_then(source_value).or_else(|| {
                Some(Signature::Value {
                    annotation: binding.annotation.as_deref().map(expression),
                    value: None,
                })
            });
            (
                DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Constant,
                    declaration: JuliaDeclaration::Constant,
                    signature,
                },
                source,
            )
        }
        BindingKind::Type => {
            let ty = binding
                .ty
                .as_ref()
                .ok_or_else(|| format!("Julia type `{name}` has no reflected type data."))?;
            let flavor = match ty.flavor.as_str() {
                "abstract" => JuliaTypeKind::Abstract,
                "primitive" => JuliaTypeKind::Primitive {
                    bits: Some(ty.bits.to_string()),
                },
                "struct" => JuliaTypeKind::Struct {
                    mutable: ty.mutable,
                },
                _ => return Err(format!("Invalid reflected type flavor for `{name}`.")),
            };
            let source = unique(sources.for_binding(name).filter(|draft| matches!(
                &draft.declaration,
                DraftDeclaration::Named { declaration: JuliaDeclaration::Type { flavor: authored, .. }, .. }
                if std::mem::discriminant(authored) == std::mem::discriminant(&flavor)
            )));
            let (parameters, supertype) = match source.map(|source| &source.declaration) {
                Some(DraftDeclaration::Named {
                    declaration:
                        JuliaDeclaration::Type {
                            parameters,
                            supertype,
                            ..
                        },
                    ..
                }) => (parameters.clone(), supertype.clone()),
                _ => (
                    ty.parameters.iter().map(|p| expression(p)).collect(),
                    Some(expression(&ty.supertype)),
                ),
            };
            let signature = source.and_then(source_value).or_else(|| {
                let parameters = if ty.parameters.is_empty() {
                    String::new()
                } else {
                    format!("{{{}}}", ty.parameters.join(", "))
                };
                Some(reflected_signature(&format!(
                    "{name}{parameters} <: {}",
                    ty.supertype
                )))
            });
            for field in &ty.fields {
                let canonical = qualify(name, &field.name);
                let source = unique(
                    sources
                        .for_binding(&canonical)
                        .filter(|draft| source_kind(draft, JuliaIdentityKind::Field)),
                );
                let signature = source.and_then(source_value).or_else(|| {
                    Some(Signature::Value {
                        annotation: Some(expression(&field.annotation)),
                        value: None,
                    })
                });
                let field_name = qualify(&binding.name, &field.name);
                let mut draft = named(
                    &binding.module,
                    &field_name,
                    &canonical,
                    DraftDeclaration::Named {
                        kind: JuliaIdentityKind::Field,
                        declaration: JuliaDeclaration::Field {
                            is_const: field.is_const,
                        },
                        signature,
                    },
                    source.and_then(|draft| draft.source.clone()),
                );
                (draft.documentation, draft.documentation_module) =
                    sources.documentation(&canonical, &field.docs, true, diagnostics)?;
                // Julia registers field docs with the type's docstring. An
                // independently authored field can still have verified prose.
                if field.docs.is_empty() {
                    draft.documentation = source.and_then(|draft| draft.documentation.clone());
                }
                fields.push(draft);
            }
            (
                DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Type,
                    declaration: JuliaDeclaration::Type {
                        flavor,
                        parameters,
                        supertype,
                        constructors: vec![],
                    },
                    signature,
                },
                source,
            )
        }
        BindingKind::Function => {
            let source = unique(
                sources
                    .for_binding(name)
                    .filter(|draft| matches!(draft.declaration, DraftDeclaration::Generic)),
            );
            (DraftDeclaration::Generic, source)
        }
        BindingKind::Macro => unreachable!("macro handled with its method"),
    };
    let mut source_location = source.and_then(|draft| draft.source.clone());
    if matches!(binding.kind, BindingKind::Function) && source_location.is_none() {
        source_location = methods.iter().find_map(|method| method.source.clone());
    }
    let mut draft = named(
        &binding.module,
        &binding.name,
        name,
        declaration,
        source_location,
    );
    with_doc(&mut draft, sources, name, &binding.docs, diagnostics)?;
    output.drafts.push(draft);
    output.drafts.extend(fields);
    output.drafts.extend(methods);
    Ok(())
}

pub(crate) fn reconcile(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    observed: Observation,
) -> Result<(JuliaExtraction, Provenance), String> {
    if !valid_target(repository, package, target) {
        return Err(
            "Julia targets must be explicit .jl entry files inside the configured package.".into(),
        );
    }
    let (mut result, mut extraction) =
        prepare_extraction(repository, package, target, CAPABILITIES);
    let metadata = result
        .metadata
        .as_ref()
        .ok_or("Julia metadata is unavailable.")?;
    if observed.policy != POLICY
        || observed.package_name != metadata.name
        || !observed.package_uuid.eq_ignore_ascii_case(&metadata.uuid)
        || metadata
            .version
            .as_ref()
            .is_some_and(|version| version != &observed.package_version)
        || observed.julia_version.is_empty()
        || observed.dependencies.is_empty()
    {
        return Err("Julia runtime identity conflicts with Project.toml.".into());
    }

    let project = package.path.join("Project.toml");
    if project != package.metadata_path {
        read_input(
            repository,
            package,
            &project,
            "package-metadata",
            &mut result,
            &mut extraction,
        );
    }
    let mut files: Vec<_> = observed
        .inputs
        .iter()
        .map(|input| {
            (
                package.path.join(input.canonical.as_str()),
                input.module.clone(),
            )
        })
        .collect();
    files.sort();
    files.dedup();
    let authored = source::extract(
        repository,
        package,
        source::Inputs::Observed(&files),
        &mut result,
        &mut extraction,
    );
    let source_diagnostics = std::mem::take(&mut result.diagnostics);
    result.diagnostics.extend(
        source_diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.severity == Severity::Error
                    && diagnostic.code != DiagnosticCode::JuliaSyntax
            })
            .cloned(),
    );
    let modules = observed
        .items
        .iter()
        .filter(|i| matches!(i.kind, BindingKind::Module))
        .map(|i| i.qualified_name.clone())
        .collect();
    let aliases: BTreeMap<_, _> = observed
        .items
        .iter()
        .flat_map(|binding| {
            binding
                .aliases
                .iter()
                .map(|alias| (alias.name.clone(), binding.qualified_name.clone()))
        })
        .collect();
    let mut drafts = BTreeMap::<_, Vec<_>>::new();
    for draft in authored.drafts {
        let name = surface::canonical(&draft, &aliases, &modules);
        drafts.entry(name).or_default().push(draft);
    }
    let mut locations = BTreeMap::new();
    for input in &observed.inputs {
        let path = repository
            .source_location(package.path.join(input.canonical.as_str()), None)
            .map_err(|_| "Julia input is outside the repository.")?
            .path;
        let source = result
            .inputs
            .get(&path)
            .ok_or("Missing observed Julia source input.")?;
        let text = source
            .text
            .as_ref()
            .ok_or("Julia input is not valid UTF-8.")?;
        let lines: Vec<_> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .collect();
        locations.insert(&input.path, (source.source.clone(), lines.clone()));
        locations.insert(&input.canonical, (source.source.clone(), lines));
    }
    let sources = Sources {
        declarations: drafts,
        files: locations,
        diagnostics: source_diagnostics,
        repository_path: &repository.path,
    };
    let mut native = Declarations::default();
    for export in observed.exports {
        let source = unique(authored.exports.iter().filter(|entry| {
            entry.module == export.module
                && entry.name == export.name
                && entry.visibility == export.visibility
        }))
        .and_then(|entry| entry.source.clone());
        native.exports.push(Export {
            module: export.module,
            name: export.name,
            visibility: export.visibility,
            source,
        });
    }
    for binding in &observed.items {
        binding_drafts(&sources, binding, &mut native, &mut result.diagnostics)?;
        for alias in &binding.aliases {
            let (module, name) = alias
                .name
                .rsplit_once('.')
                .ok_or("Julia alias is missing a defining module.")?;
            match alias.kind {
                AliasKind::Import => {
                    let source = unique(
                        authored
                            .imports
                            .iter()
                            .filter(|entry| entry.module == module && entry.name == name),
                    )
                    .and_then(|entry| entry.source.clone());
                    native.imports.push(Import {
                        module: module.into(),
                        name: name.into(),
                        target: binding.qualified_name.clone(),
                        source,
                        whole_using: false,
                    });
                }
                AliasKind::Assignment => {
                    let source = unique(
                        sources
                            .for_binding(&binding.qualified_name)
                            .chain(sources.for_binding(&alias.name))
                            .filter(|draft| draft.module == module && draft.name == name),
                    )
                    .and_then(|draft| draft.source.clone());
                    native.drafts.push(named(
                        module,
                        name,
                        &alias.name,
                        DraftDeclaration::Alias {
                            target: binding.qualified_name.clone(),
                        },
                        source,
                    ));
                }
            }
        }
    }
    let extensions = source::extract(
        repository,
        package,
        source::Inputs::Extensions(target),
        &mut result,
        &mut extraction,
    );
    native.imports.extend(extensions.imports);
    native.exports.extend(extensions.exports);
    native.drafts.extend(extensions.drafts);
    surface::assemble(native, &mut result);
    let evidence = Provenance {
        activity: ProvenanceActivity::JuliaRuntime {
            target: result.target.clone(),
            policy: observed.policy,
            julia_version: observed.julia_version.clone(),
            package_name: observed.package_name,
            package_uuid: observed.package_uuid,
            package_version: observed.package_version,
            dependencies: observed.dependencies,
        },
        source: None,
        span: None,
        tools: BTreeMap::from([("julia".into(), observed.julia_version.clone())]),
    };
    for item in result.items.values_mut() {
        if item
            .source_location
            .as_ref()
            .is_none_or(|source| source.span.is_none())
        {
            item.provenance.push(evidence.clone());
        }
    }
    result.provenance = extraction.into_provenance(None, None);
    result
        .provenance
        .tools
        .insert("julia-runtime".into(), observed.julia_version);
    if let ProvenanceActivity::Extraction { mode, .. } = &mut result.provenance.activity {
        *mode = ExtractionMode::Native;
    }
    result
        .diagnostics
        .extend(observed.errors.into_iter().map(|message| {
            let code =
                if message.contains("undefined") || message.contains("external-only reexports") {
                    DiagnosticCode::JuliaUnresolvedDefinition
                } else {
                    DiagnosticCode::JuliaUnsupportedSurface
                };
            Diagnostic::new(code, Severity::Error, message)
        }));
    result.diagnostics.sort();
    result.diagnostics.dedup();
    Ok((result, evidence))
}
