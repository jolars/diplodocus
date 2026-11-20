//! Validate workspace identities and references without accessing source files.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::configuration::{ExecutionMode, IncrementalConfiguration, WorkspaceConfiguration};
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};
use crate::documents::AuthoredFormat;

/// Collect identity and reference errors in the shared [`Diagnostic`] order.
///
/// Repository, package, content, and concept IDs each have a distinct workspace
/// namespace. Package slugs are unique across the entire site, including hidden
/// packages. Target IDs are unique only within their declaring package. Names
/// are compared exactly as declared, without normalization or inference. Each
/// later duplicate names its field and the first declaration's field.
///
/// Package and content repository references must name declared repositories.
/// Content owners must be the reserved `project` owner or a declared package ID.
/// Executable Documenter collections with a package owner require a local checkout.
/// Concept members must name declared packages; resolving their items requires
/// extracted IR and remains a later step. Duplicate declarations are diagnosed
/// without selecting a winner or cascading errors onto references to their IDs.
///
/// Relationship endpoints first match exact workspace package IDs. Otherwise,
/// external endpoints require `ecosystem:package` syntax: an ASCII ecosystem
/// identifier starting with a letter and continuing with letters, digits, `.`,
/// `_`, or `-`, followed by a nonempty package coordinate. All colon-separated
/// components must be nonempty, and whitespace and control characters are
/// rejected. Package components otherwise remain opaque, allowing scoped or
/// multipart coordinates without an ecosystem allowlist or dependency resolution.
/// Unqualified unknown names are errors, never inferred external dependencies.
///
/// This performs no I/O, changes no declarations, and supplies no source path or
/// spans. Use [`validate_configuration_with_source`] when a portable configuration
/// path is available. Parsing still checks collection execution settings, and
/// [`crate::paths::resolve_workspace_paths`] separately checks filesystem inputs.
pub fn validate_configuration(configuration: &WorkspaceConfiguration) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut repositories = Names::new(DiagnosticCode::DuplicateRepositoryId);
    let mut packages = Names::new(DiagnosticCode::DuplicatePackageId);
    let mut slugs = Names::new(DiagnosticCode::DuplicatePackageSlug);
    let mut content = Names::new(DiagnosticCode::DuplicateContentId);
    let mut concepts = Names::new(DiagnosticCode::DuplicateConceptId);

    for (index, repository) in configuration.repositories.iter().enumerate() {
        repositories.declare(
            &repository.id,
            format!("repositories[{index}].id"),
            DiagnosticEntity::Repository {
                id: repository.id.clone(),
            },
            &mut diagnostics,
        );
    }
    if let Some(selected) = &configuration.presentation.header.repositories
        && configuration.package_bundles.is_empty()
    {
        for (index, repository) in selected.iter().enumerate() {
            let field = format!("presentation.header.repositories[{index}]");
            check_repository(
                &repositories,
                repository,
                &field,
                DiagnosticEntity::ConfigurationField {
                    path: field.clone(),
                },
                &mut diagnostics,
            );
        }
    }
    for (field, repository) in [
        (
            "presentation.logo.repository",
            configuration
                .presentation
                .logo
                .as_ref()
                .map(|image| &image.repository),
        ),
        (
            "presentation.open-graph.repository",
            configuration
                .presentation
                .open_graph
                .as_ref()
                .map(|image| &image.repository),
        ),
    ] {
        let Some(repository) = repository else {
            continue;
        };
        check_repository(
            &repositories,
            repository,
            field,
            DiagnosticEntity::ConfigurationField { path: field.into() },
            &mut diagnostics,
        );
    }
    for (index, asset) in configuration.presentation.assets.iter().enumerate() {
        let field = format!("presentation.assets[{index}].repository");
        check_repository(
            &repositories,
            &asset.repository,
            &field,
            DiagnosticEntity::ConfigurationField {
                path: field.clone(),
            },
            &mut diagnostics,
        );
    }
    if let Err(message) = crate::site_assets::validate_declarations(&configuration.presentation) {
        diagnostics.push(error(
            DiagnosticCode::InvalidConfiguration,
            message,
            DiagnosticEntity::ConfigurationField {
                path: "presentation.assets".into(),
            },
        ));
    }
    if let Some(image) = &configuration.presentation.open_graph {
        for (invalid, field, message) in [
            (
                configuration.presentation.site_url.is_none(),
                "presentation.site-url",
                "presentation.site-url is required when presentation.open-graph is configured",
            ),
            (
                image.alt.trim().is_empty(),
                "presentation.open-graph.alt",
                "presentation.open-graph.alt must describe the preview image",
            ),
        ] {
            if invalid {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    message.into(),
                    DiagnosticEntity::ConfigurationField { path: field.into() },
                ));
            }
        }
    }
    for (index, package) in configuration.packages.iter().enumerate() {
        if let Some(settings) = &package.r {
            let mut invalid = package.ecosystem != "r";
            for path in settings
                .extra_inputs
                .iter()
                .chain(&settings.generated_outputs)
            {
                invalid |= path.as_os_str().is_empty()
                    || path
                        .components()
                        .any(|component| !matches!(component, std::path::Component::Normal(_)));
            }
            if invalid {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "R package settings require ecosystem `r` and nonempty package-relative input and output paths without `.` or `..` components.".into(),
                    DiagnosticEntity::ConfigurationField { path: format!("packages[{index}].r") },
                ));
            }
        }
        let entity = DiagnosticEntity::Package {
            id: package.id.clone(),
        };
        packages.declare(
            &package.id,
            format!("packages[{index}].id"),
            entity.clone(),
            &mut diagnostics,
        );
        slugs.declare(
            &package.slug,
            format!("packages[{index}].slug"),
            entity.clone(),
            &mut diagnostics,
        );
        check_repository(
            &repositories,
            &package.repository,
            &format!("packages[{index}].repository"),
            entity,
            &mut diagnostics,
        );

        let mut targets = Names::new(DiagnosticCode::DuplicateTargetId);
        for (target_index, target) in package.targets.iter().enumerate() {
            if target.extractor == "r" && package.ecosystem != "r" {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "R targets require ecosystem `r`.".into(),
                    DiagnosticEntity::Target {
                        package: package.id.clone(),
                        id: target.id.clone(),
                    },
                ));
            }
            if target.extractor == "doxygen" && package.ecosystem != "cpp" {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "Doxygen targets require ecosystem `cpp`.".into(),
                    DiagnosticEntity::Target {
                        package: package.id.clone(),
                        id: target.id.clone(),
                    },
                ));
            }
            targets.declare(
                &target.id,
                format!("packages[{index}].targets[{target_index}].id"),
                DiagnosticEntity::Target {
                    package: package.id.clone(),
                    id: target.id.clone(),
                },
                &mut diagnostics,
            );
            if (target.rust.is_some() && target.extractor != "rust")
                || (target.extractor == "rust" && package.ecosystem != "rust")
            {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "Rust target settings require extractor `rust` and ecosystem `rust`.".into(),
                    DiagnosticEntity::Target {
                        package: package.id.clone(),
                        id: target.id.clone(),
                    },
                ));
            }
            if let Some(settings) = &target.rust
                && settings.features.iter().any(|feature| {
                    feature.is_empty()
                        || feature.starts_with('-')
                        || feature.contains(',')
                        || feature.chars().any(|c| c.is_whitespace() || c.is_control())
                })
            {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "Rust features must be nonempty individual Cargo feature names.".into(),
                    DiagnosticEntity::Target {
                        package: package.id.clone(),
                        id: target.id.clone(),
                    },
                ));
            }
            if target.julia.is_some()
                && (target.extractor != "julia" || package.ecosystem != "julia")
            {
                diagnostics.push(error(
                    DiagnosticCode::InvalidConfiguration,
                    "Julia target settings require extractor `julia` and ecosystem `julia`.".into(),
                    DiagnosticEntity::Target {
                        package: package.id.clone(),
                        id: target.id.clone(),
                    },
                ));
            }
        }
    }
    let local_packages = packages.clone();
    for (index, bundle) in configuration.package_bundles.iter().enumerate() {
        let invalid = bundle.path.is_some() == bundle.github_release.is_some()
            || bundle
                .mount
                .as_deref()
                .is_some_and(|mount| !crate::configuration::valid_package_mount(mount))
            || (bundle.local_override
                && (bundle.github_release.is_none() || !local_packages.contains(&bundle.package)))
            || bundle
                .github_release
                .as_ref()
                .is_some_and(|release| !release.valid());
        if invalid {
            let path = format!("package-bundles[{index}]");
            diagnostics.push(error(DiagnosticCode::InvalidConfiguration,
                format!("{path}: expected one valid bundle source, valid mount and release pins, and a matching local package for local-override"),
                DiagnosticEntity::ConfigurationField { path }));
        }
        if bundle.local_override && local_packages.contains(&bundle.package) {
            if configuration.package_bundles[..index]
                .iter()
                .any(|other| other.package == bundle.package)
            {
                diagnostics.push(error(
                    DiagnosticCode::DuplicatePackageId,
                    format!("duplicate bundle for package `{}`", bundle.package),
                    DiagnosticEntity::Package {
                        id: bundle.package.clone(),
                    },
                ));
            }
            continue;
        }
        packages.declare(
            &bundle.package,
            format!("package-bundles[{index}].package"),
            DiagnosticEntity::Package {
                id: bundle.package.clone(),
            },
            &mut diagnostics,
        );
    }
    for (index, collection) in configuration.content.iter().enumerate() {
        let entity = DiagnosticEntity::Content {
            id: collection.id.clone(),
        };
        content.declare(
            &collection.id,
            format!("content[{index}].id"),
            entity.clone(),
            &mut diagnostics,
        );
        check_repository(
            &repositories,
            &collection.repository,
            &format!("content[{index}].repository"),
            entity.clone(),
            &mut diagnostics,
        );
        if collection.owner != "project" && !packages.contains(&collection.owner) {
            diagnostics.push(error(
                DiagnosticCode::UnknownContentOwner,
                format!("content[{index}].owner: unknown owner `{}`; expected `project` or a workspace package ID", collection.owner),
                entity,
            ));
        } else if collection.owner != "project"
            && collection.format == AuthoredFormat::Documenter
            && collection.execution.mode == ExecutionMode::Execute
            && !local_packages.contains(&collection.owner)
        {
            diagnostics.push(error(
                DiagnosticCode::InvalidConfiguration,
                format!(
                    "content[{index}].owner: executable Documenter collections require a local checkout for package owner `{}`",
                    collection.owner
                ),
                entity,
            ));
        }
    }
    for (index, package) in configuration.packages.iter().enumerate() {
        validate_incremental(
            &package.incremental,
            &format!("packages[{index}].incremental"),
            &local_packages,
            &mut diagnostics,
        );
    }
    for (index, collection) in configuration.content.iter().enumerate() {
        validate_incremental(
            &collection.incremental,
            &format!("content[{index}].incremental"),
            &local_packages,
            &mut diagnostics,
        );
    }
    for (index, concept) in configuration.concepts.iter().enumerate() {
        let entity = DiagnosticEntity::Concept {
            id: concept.id.clone(),
        };
        concepts.declare(
            &concept.id,
            format!("concepts[{index}].id"),
            entity.clone(),
            &mut diagnostics,
        );
        for (member_index, member) in concept.members.iter().enumerate() {
            if !packages.contains(&member.package) {
                diagnostics.push(error(
                    DiagnosticCode::UnknownConceptPackage,
                    format!("concepts[{index}].members[{member_index}].package: unknown workspace package `{}`", member.package),
                    entity.clone(),
                ));
            }
        }
    }
    for (index, relationship) in configuration.relationships.iter().enumerate() {
        for (field, endpoint) in [("from", &relationship.from), ("to", &relationship.to)] {
            if packages.contains(endpoint) {
                continue;
            }
            let (code, message) = if endpoint.contains(':') {
                if is_external_coordinate(endpoint) {
                    continue;
                }
                (
                    DiagnosticCode::InvalidExternalPackageCoordinate,
                    format!(
                        "relationships[{index}].{field}: invalid external package coordinate `{endpoint}`; expected nonempty `ecosystem:package` components without whitespace or control characters"
                    ),
                )
            } else {
                (
                    DiagnosticCode::UnknownRelationshipEndpoint,
                    format!(
                        "relationships[{index}].{field}: unknown workspace package `{endpoint}`; external packages require an explicit `ecosystem:package` coordinate"
                    ),
                )
            };
            diagnostics.push(error(
                code,
                message,
                DiagnosticEntity::Relationship { index },
            ));
        }
    }

    diagnostics.sort();
    diagnostics
}

fn validate_incremental(
    settings: &IncrementalConfiguration,
    field: &str,
    packages: &Names<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for dependency in &settings.depends_on {
        if !packages.contains(dependency) {
            diagnostics.push(error(
                DiagnosticCode::InvalidConfiguration,
                format!("{field}.depends-on: unknown workspace package `{dependency}`"),
                DiagnosticEntity::ConfigurationField {
                    path: format!("{field}.depends-on"),
                },
            ));
        }
    }
    if settings.extra_inputs.iter().any(|path| {
        path.as_os_str().is_empty()
            || path.components().any(|component| {
                !matches!(
                    component,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
    }) {
        diagnostics.push(error(
            DiagnosticCode::InvalidConfiguration,
            format!("{field}.extra-inputs must contain nonempty repository-relative paths without `..` components"),
            DiagnosticEntity::ConfigurationField {
                path: format!("{field}.extra-inputs"),
            },
        ));
    }
    if settings
        .environment_variables
        .iter()
        .any(|name| name.is_empty() || name.contains(['=', '\0']))
    {
        diagnostics.push(error(
            DiagnosticCode::InvalidConfiguration,
            format!("{field}.environment-variables must contain nonempty variable names without `=` or null bytes"),
            DiagnosticEntity::ConfigurationField {
                path: format!("{field}.environment-variables"),
            },
        ));
    }
}

/// Validate identities and references with a caller-supplied configuration path.
///
/// The path is relative to the workspace configuration directory. Declarations
/// do not retain source ranges, so primary and related spans remain absent.
/// This attaches the same source to every diagnostic from [`validate_configuration`]
/// and preserves its deterministic order without reading the configuration file.
pub fn validate_configuration_with_source(
    configuration: &WorkspaceConfiguration,
    path: DiagnosticPath,
) -> Vec<Diagnostic> {
    let source = DiagnosticSource::Configuration { path };
    validate_configuration(configuration)
        .into_iter()
        .map(|diagnostic| diagnostic.with_source(source.clone()))
        .collect()
}

#[derive(Clone)]
struct Names<'a> {
    first_fields: BTreeMap<&'a str, String>,
    duplicate_code: DiagnosticCode,
}

impl<'a> Names<'a> {
    fn new(duplicate_code: DiagnosticCode) -> Self {
        Self {
            first_fields: BTreeMap::new(),
            duplicate_code,
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.first_fields.contains_key(name)
    }

    fn declare(
        &mut self,
        name: &'a str,
        field: String,
        entity: DiagnosticEntity,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        match self.first_fields.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(field);
            }
            Entry::Occupied(entry) => diagnostics.push(error(
                self.duplicate_code,
                format!(
                    "{field}: duplicate `{name}`; first declared at {}",
                    entry.get()
                ),
                entity,
            )),
        }
    }
}

fn check_repository(
    repositories: &Names<'_>,
    repository: &str,
    field: &str,
    entity: DiagnosticEntity,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !repositories.contains(repository) {
        diagnostics.push(error(
            DiagnosticCode::InvalidRepositoryReference,
            format!(
                "{field}: unknown repository `{repository}`; expected a workspace repository ID"
            ),
            entity,
        ));
    }
}

fn is_external_coordinate(endpoint: &str) -> bool {
    let Some((ecosystem, _)) = endpoint.split_once(':') else {
        return false;
    };
    ecosystem.starts_with(|character: char| character.is_ascii_alphabetic())
        && ecosystem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && endpoint.split(':').all(|component| !component.is_empty())
        && !endpoint
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn error(code: DiagnosticCode, message: String, entity: DiagnosticEntity) -> Diagnostic {
    Diagnostic::new(code, Severity::Error, message).with_entity(entity)
}
