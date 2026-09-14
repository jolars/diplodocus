//! Validate workspace identities and references without accessing source files.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::configuration::WorkspaceConfiguration;
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};

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
            format!("repository[{index}].id"),
            DiagnosticEntity::Repository {
                id: repository.id.clone(),
            },
            &mut diagnostics,
        );
    }
    for (index, package) in configuration.packages.iter().enumerate() {
        let entity = DiagnosticEntity::Package {
            id: package.id.clone(),
        };
        packages.declare(
            &package.id,
            format!("package[{index}].id"),
            entity.clone(),
            &mut diagnostics,
        );
        slugs.declare(
            &package.slug,
            format!("package[{index}].slug"),
            entity.clone(),
            &mut diagnostics,
        );
        check_repository(
            &repositories,
            &package.repository,
            &format!("package[{index}].repository"),
            entity,
            &mut diagnostics,
        );

        let mut targets = Names::new(DiagnosticCode::DuplicateTargetId);
        for (target_index, target) in package.targets.iter().enumerate() {
            targets.declare(
                &target.id,
                format!("package[{index}].targets[{target_index}].id"),
                DiagnosticEntity::Target {
                    package: package.id.clone(),
                    id: target.id.clone(),
                },
                &mut diagnostics,
            );
        }
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
        }
    }
    for (index, concept) in configuration.concepts.iter().enumerate() {
        let entity = DiagnosticEntity::Concept {
            id: concept.id.clone(),
        };
        concepts.declare(
            &concept.id,
            format!("concept[{index}].id"),
            entity.clone(),
            &mut diagnostics,
        );
        for (member_index, member) in concept.members.iter().enumerate() {
            if !packages.contains(&member.package) {
                diagnostics.push(error(
                    DiagnosticCode::UnknownConceptPackage,
                    format!("concept[{index}].members[{member_index}].package: unknown workspace package `{}`", member.package),
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
                        "relationship[{index}].{field}: invalid external package coordinate `{endpoint}`; expected nonempty `ecosystem:package` components without whitespace or control characters"
                    ),
                )
            } else {
                (
                    DiagnosticCode::UnknownRelationshipEndpoint,
                    format!(
                        "relationship[{index}].{field}: unknown workspace package `{endpoint}`; external packages require an explicit `ecosystem:package` coordinate"
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
