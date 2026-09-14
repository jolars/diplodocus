use diplodocus::configuration::{WorkspaceConfiguration, parse_configuration};
use diplodocus::configuration_validation::{
    validate_configuration, validate_configuration_with_source,
};
use diplodocus::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};

const COMPLETE: &str = include_str!("fixtures/configuration/complete.toml");
const ACCEPTANCE: &str = include_str!("fixtures/acceptance/workspace/diplodocus.toml");

fn configuration() -> WorkspaceConfiguration {
    parse_configuration(COMPLETE).unwrap()
}

fn only_diagnostic(configuration: &WorkspaceConfiguration, code: DiagnosticCode) -> Diagnostic {
    let diagnostics = validate_configuration(configuration);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let diagnostic = diagnostics.into_iter().next().unwrap();
    assert_eq!(diagnostic.code, code);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert!(diagnostic.source.is_none());
    assert!(diagnostic.span.is_none());
    assert!(diagnostic.related_spans.is_empty());
    diagnostic
}

#[test]
fn duplicate_repository_ids_have_repository_context() {
    let mut config = configuration();
    config.repositories.push(config.repositories[0].clone());
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicateRepositoryId);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Repository {
            id: "python".into()
        })
    );
    assert!(diagnostic.message.contains("repository[1].id"));
    assert!(diagnostic.message.contains("repository[0].id"));
}

#[test]
fn duplicate_package_ids_are_independent_of_slugs() {
    let mut config = configuration();
    let mut duplicate = config.packages[0].clone();
    duplicate.slug = "another-slug".into();
    config.packages.push(duplicate);
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicatePackageId);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Package { id: "pyfoo".into() })
    );
}

#[test]
fn package_slugs_are_unique_across_ecosystems_and_visibility() {
    let mut config = parse_configuration(ACCEPTANCE).unwrap();
    config.packages[1].slug = config.packages[0].slug.clone();
    config.packages[1].visibility = diplodocus::configuration::PackageVisibility::Hidden;
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicatePackageSlug);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Package { id: "rfoo".into() })
    );
    assert!(diagnostic.message.contains("package[0].slug"));
    assert!(diagnostic.message.contains("package[1].slug"));
}

#[test]
fn duplicate_target_ids_have_package_scoped_context() {
    let mut config = configuration();
    config.packages[0].targets[1].id = "api".into();
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicateTargetId);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Target {
            package: "pyfoo".into(),
            id: "api".into()
        })
    );
    assert!(diagnostic.message.contains("package[0].targets[0].id"));
    assert!(diagnostic.message.contains("package[0].targets[1].id"));
}

#[test]
fn duplicate_content_ids_are_workspace_scoped() {
    let mut config = configuration();
    let mut duplicate = config.content[0].clone();
    duplicate.owner = "project".into();
    config.content.push(duplicate);
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicateContentId);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Content {
            id: "tutorials".into()
        })
    );
}

#[test]
fn duplicate_concept_ids_have_concept_context() {
    let mut config = configuration();
    config.concepts.push(config.concepts[0].clone());
    let diagnostic = only_diagnostic(&config, DiagnosticCode::DuplicateConceptId);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Concept {
            id: "foo-model.fit".into()
        })
    );
}

#[test]
fn every_later_duplicate_refers_to_the_first_declaration() {
    let mut config = configuration();
    for _ in 0..2 {
        config.concepts.push(config.concepts[0].clone());
    }
    let diagnostics = validate_configuration(&config);
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("concept[0].id"))
    );
    assert!(diagnostics[0].message.contains("concept[1].id"));
    assert!(diagnostics[1].message.contains("concept[2].id"));
}

#[test]
fn owners_must_be_project_or_an_exact_workspace_package_id() {
    for owner in [
        "typo",
        "python",
        "Foo for Python",
        "cargo:foo-core",
        "Project",
        "",
    ] {
        let mut config = configuration();
        config.content[0].owner = owner.into();
        let diagnostic = only_diagnostic(&config, DiagnosticCode::UnknownContentOwner);
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::Content {
                id: "tutorials".into()
            })
        );
        assert!(diagnostic.message.contains("content[0].owner"));
        assert!(diagnostic.message.contains(&format!("`{owner}`")));
    }
    for owner in ["project", "pyfoo"] {
        let mut config = configuration();
        config.content[0].owner = owner.into();
        assert!(validate_configuration(&config).is_empty());
    }
}

#[test]
fn both_unknown_relationship_endpoints_are_reported_without_external_inference() {
    let mut config = configuration();
    config.relationships[0].from = "python".into();
    config.relationships[0].to = "foo-core".into();
    let diagnostics = validate_configuration(&config);
    assert_eq!(diagnostics.len(), 2);
    for (diagnostic, field, endpoint) in [
        (&diagnostics[0], "from", "python"),
        (&diagnostics[1], "to", "foo-core"),
    ] {
        assert_eq!(diagnostic.code, DiagnosticCode::UnknownRelationshipEndpoint);
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::Relationship { index: 0 })
        );
        assert!(
            diagnostic
                .message
                .contains(&format!("relationship[0].{field}"))
        );
        assert!(diagnostic.message.contains(&format!("`{endpoint}`")));
    }
}

#[test]
fn explicit_external_coordinates_need_no_local_packages_or_repositories() {
    for endpoint in [
        "cargo:foo-core",
        "python:foo",
        "r:foo",
        "npm:@scope/pkg",
        "maven:org.example:artifact",
        "future-ecosystem:release/name@v1",
    ] {
        let mut config = configuration();
        config.repositories.clear();
        config.packages.clear();
        config.content.clear();
        config.concepts.clear();
        config.relationships[0].from = endpoint.into();
        config.relationships[0].to = endpoint.into();
        let before = config.clone();
        assert!(validate_configuration(&config).is_empty(), "{endpoint}");
        assert_eq!(config, before);
    }
}

#[test]
fn malformed_external_coordinates_do_not_hide_unknown_endpoints() {
    for endpoint in [
        "cargo:",
        ":foo",
        "cargo::foo",
        "car go:foo",
        "cargo: foo",
        "cargo:foo\nbar",
    ] {
        let mut config = configuration();
        config.relationships[0].to = endpoint.into();
        let diagnostic = only_diagnostic(&config, DiagnosticCode::InvalidExternalPackageCoordinate);
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::Relationship { index: 0 })
        );
        assert!(diagnostic.message.contains("relationship[0].to"));
    }
}

#[test]
fn declared_package_ids_take_precedence_over_external_coordinate_syntax() {
    let mut config = configuration();
    config.packages[0].id = "cargo:local".into();
    config.content[0].owner = "cargo:local".into();
    for member in &mut config.concepts[0].members {
        member.package = "cargo:local".into();
    }
    config.relationships[0].from = "cargo:local".into();
    assert!(validate_configuration(&config).is_empty());
}

#[test]
fn unknown_package_and_content_repositories_are_reported_without_filesystem_access() {
    let mut config = configuration();
    config.repositories.clear();
    let diagnostics = validate_configuration(&config);
    assert_eq!(diagnostics.len(), 2);
    for diagnostic in &diagnostics {
        assert_eq!(diagnostic.code, DiagnosticCode::InvalidRepositoryReference);
        assert!(diagnostic.message.contains("`python`"));
    }
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.related_entity
                == Some(DiagnosticEntity::Package { id: "pyfoo".into() }))
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.related_entity
                == Some(DiagnosticEntity::Content {
                    id: "tutorials".into()
                }))
    );
}

#[test]
fn concept_packages_must_exist_but_item_resolution_waits_for_extraction() {
    let mut config = configuration();
    config.concepts[0].members[0].package = "cargo:external".into();
    config.concepts[0].members[1].item = "not-extracted-yet".into();
    let diagnostic = only_diagnostic(&config, DiagnosticCode::UnknownConceptPackage);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Concept {
            id: "foo-model.fit".into()
        })
    );
    assert!(diagnostic.message.contains("concept[0].members[0].package"));
}

#[test]
fn distinct_namespaces_and_target_ids_reused_across_packages_are_valid() {
    let mut config = parse_configuration(ACCEPTANCE).unwrap();
    assert_eq!(
        config.packages[0].targets[0].id,
        config.packages[1].targets[0].id
    );
    config.content[0].id = "pyfoo".into();
    config.concepts[0].id = "pyfoo".into();
    assert!(validate_configuration(&config).is_empty());
}

#[test]
fn valid_fixtures_and_empty_workspaces_have_no_identity_diagnostics() {
    for source in [
        "[project]\nname = 'Empty'\n",
        COMPLETE,
        ACCEPTANCE,
        include_str!("fixtures/acceptance/workspace/variants/relationship-compatible.toml"),
        include_str!("fixtures/acceptance/workspace/variants/relationship-incompatible.toml"),
        include_str!("fixtures/acceptance/workspace/variants/relationship-external.toml"),
        include_str!("fixtures/acceptance/workspace/variants/visibility.toml"),
        include_str!("../diplodocus.toml"),
    ] {
        let config = parse_configuration(source).unwrap();
        assert!(validate_configuration(&config).is_empty());
    }
}

#[test]
fn diagnostics_are_sorted_and_can_carry_a_portable_configuration_source() {
    let mut config = configuration();
    config.content[0].owner = "unknown".into();
    config.concepts.push(config.concepts[0].clone());
    config.repositories.push(config.repositories[0].clone());
    let diagnostics = validate_configuration(&config);
    let mut sorted = diagnostics.clone();
    sorted.sort();
    assert_eq!(diagnostics, sorted);
    assert_eq!(validate_configuration(&config), diagnostics);
    let path = "config/diplodocus.toml".try_into().unwrap();
    let sourced = validate_configuration_with_source(&config, path);
    let expected = diagnostics
        .into_iter()
        .map(|diagnostic| {
            diagnostic.with_source(DiagnosticSource::Configuration {
                path: "config/diplodocus.toml".try_into().unwrap(),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(sourced, expected);
    assert!(sourced.iter().all(|diagnostic| diagnostic.span.is_none()));
    let encoded = serde_json::to_string(&sourced).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Diagnostic>>(&encoded).unwrap(),
        sourced
    );
}

#[test]
fn semantic_diagnostic_codes_have_stable_serialized_spellings() {
    for (code, spelling) in [
        (
            DiagnosticCode::DuplicateRepositoryId,
            "duplicate-repository-id",
        ),
        (DiagnosticCode::DuplicatePackageId, "duplicate-package-id"),
        (DiagnosticCode::DuplicateTargetId, "duplicate-target-id"),
        (DiagnosticCode::DuplicateContentId, "duplicate-content-id"),
        (DiagnosticCode::DuplicateConceptId, "duplicate-concept-id"),
        (
            DiagnosticCode::DuplicatePackageSlug,
            "duplicate-package-slug",
        ),
        (DiagnosticCode::UnknownContentOwner, "unknown-content-owner"),
        (
            DiagnosticCode::UnknownConceptPackage,
            "unknown-concept-package",
        ),
        (
            DiagnosticCode::UnknownRelationshipEndpoint,
            "unknown-relationship-endpoint",
        ),
        (
            DiagnosticCode::InvalidExternalPackageCoordinate,
            "invalid-external-package-coordinate",
        ),
    ] {
        assert_eq!(code.as_str(), spelling);
        assert_eq!(serde_json::to_value(code).unwrap(), spelling);
        assert_eq!(
            serde_json::from_value::<DiagnosticCode>(spelling.into()).unwrap(),
            code
        );
    }
}
