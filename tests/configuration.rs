use std::error::Error;
use std::path::PathBuf;

use diplodocus::configuration::{
    ConceptKind, ConfigurationError, ExecutionEngine, ExecutionMode, PackageKind,
    PackageVisibility, RelationshipKind, load_configuration, parse_configuration,
};
use diplodocus::documents::AuthoredFormat;

mod support;

const COMPLETE: &str = include_str!("fixtures/configuration/complete.toml");
const ACCEPTANCE: &str = include_str!("fixtures/acceptance/workspace/diplodocus.toml");

#[test]
fn parses_every_configuration_section_and_retains_declared_values() {
    let config = parse_configuration(COMPLETE).unwrap();
    assert_eq!(config.project.name, "Foo");
    assert_eq!(config.repositories.len(), 1);
    let repository = &config.repositories[0];
    assert_eq!(repository.id, "python");
    assert_eq!(repository.path, PathBuf::from("../foo-python"));
    assert_eq!(
        repository.url.as_deref(),
        Some("https://forge.example/foo-python")
    );
    assert_eq!(repository.revision.as_deref(), Some("release-1.9"));
    assert_eq!(
        repository.source_link_template.as_deref(),
        Some("https://forge.example/foo-python/blob/{revision}/{path}#L{line}")
    );

    let package = &config.packages[0];
    assert_eq!(package.id, "pyfoo");
    assert_eq!(package.name, "Foo for Python");
    assert_eq!(package.slug, "python");
    assert_eq!(package.ecosystem, "python");
    assert_eq!(package.repository, "python");
    assert_eq!(package.path, PathBuf::from("."));
    assert_eq!(package.metadata_path, PathBuf::from("pyproject.toml"));
    assert_eq!(package.kind, PackageKind::Component);
    assert_eq!(package.visibility, PackageVisibility::Internal);
    assert_eq!(package.targets.len(), 2);
    assert_eq!(package.targets[0].id, "api");
    assert_eq!(package.targets[0].extractor, "python");
    assert_eq!(package.targets[0].path, PathBuf::from("python/foo"));
    assert_eq!(package.targets[0].role, "public-api");
    assert_eq!(package.targets[1].role, "internal-api");

    let content = &config.content[0];
    assert_eq!(content.id, "tutorials");
    assert_eq!(content.owner, "pyfoo");
    assert_eq!(content.repository, "python");
    assert_eq!(content.path, PathBuf::from("docs/tutorials"));
    assert_eq!(content.mount, "tutorials");
    assert_eq!(content.format, AuthoredFormat::Qmd);
    assert_eq!(content.execution.mode, ExecutionMode::Execute);
    assert_eq!(content.execution.engine, Some(ExecutionEngine::Jupyter));
    assert_eq!(content.execution.kernel.as_deref(), Some("python3"));
    assert_eq!(
        content.execution.declared_environment_inputs,
        [
            PathBuf::from("uv.lock"),
            PathBuf::from("environments/docs.toml")
        ]
    );

    let concept = &config.concepts[0];
    assert_eq!(concept.id, "foo-model.fit");
    assert_eq!(concept.kind, ConceptKind::Analogous);
    assert_eq!(concept.members.len(), 2);
    assert_eq!(concept.members[0].package, "pyfoo");
    assert_eq!(concept.members[0].item, "foo.FooModel.fit");
    assert_eq!(concept.members[1].item, "pyfoo::foo.fit");

    let relationship = &config.relationships[0];
    assert_eq!(relationship.from, "pyfoo");
    assert_eq!(relationship.to, "cargo:foo-core");
    assert_eq!(relationship.kind, RelationshipKind::Binds);
    assert_eq!(relationship.version_constraint.as_deref(), Some("^1.9"));
    assert_eq!(relationship.provenance.as_deref(), Some("explicit"));
}

#[test]
fn serialization_preserves_the_configuration_shape() {
    let config = parse_configuration(COMPLETE).unwrap();
    let serialized = toml::to_string(&config).unwrap();
    assert_eq!(
        toml::from_str::<toml::Value>(&serialized).unwrap(),
        toml::from_str::<toml::Value>(COMPLETE).unwrap()
    );
    assert_eq!(parse_configuration(&serialized).unwrap(), config);
}

#[test]
fn parses_design_examples() {
    let design = include_str!("../DESIGN.md");
    let mut examples = 0;
    for block in design.split("```toml\n").skip(1) {
        let source = block.split_once("```").unwrap().0;
        let source = if source.starts_with("[[concept]]") {
            format!("[project]\nname = 'Concept example'\n{source}")
        } else {
            source.to_owned()
        };
        parse_configuration(&source).unwrap();
        examples += 1;
    }
    assert_eq!(examples, 2);
}

#[test]
fn parses_acceptance_workspace_variants_and_own_documentation() {
    let config = parse_configuration(ACCEPTANCE).unwrap();
    assert_eq!(
        config
            .repositories
            .iter()
            .map(|repo| repo.id.as_str())
            .collect::<Vec<_>>(),
        ["core", "python", "r"]
    );
    assert_eq!(config.packages.len(), 2);
    assert_eq!(config.packages[1].ecosystem, "r");
    assert_eq!(config.packages[1].targets[0].extractor, "r");
    assert_eq!(config.content[0].owner, "project");
    assert_eq!(config.content[0].format, AuthoredFormat::Gfm);
    assert_eq!(config.content[4].execution.kernel.as_deref(), Some("ir"));
    for path in support::fixture_files("acceptance/workspace/variants") {
        load_configuration(support::fixture_path("acceptance/workspace/variants").join(path))
            .unwrap();
    }
    let own = load_configuration(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("diplodocus.toml"))
        .unwrap();
    assert!(own.packages.is_empty());
    assert_eq!(own.content.len(), 2);
}

#[test]
fn applies_only_documented_defaults_without_discovering_inputs() {
    let config = parse_configuration("[project]\nname = 'Empty workspace'\n").unwrap();
    assert!(config.repositories.is_empty());
    assert!(config.packages.is_empty());
    assert!(config.content.is_empty());
    assert!(config.concepts.is_empty());
    assert!(config.relationships.is_empty());

    let config = parse_configuration(ACCEPTANCE).unwrap();
    assert_eq!(config.packages[0].kind, PackageKind::Package);
    assert_eq!(config.packages[0].visibility, PackageVisibility::Public);
    assert_eq!(config.repositories[0].revision, None);
    assert_eq!(config.repositories[0].source_link_template, None);
    for content in &config.content[..3] {
        assert_eq!(content.execution.mode, ExecutionMode::Never);
        assert_eq!(content.execution.engine, None);
        assert_eq!(content.execution.kernel, None);
        assert!(content.execution.declared_environment_inputs.is_empty());
    }

    let config = parse_configuration(&COMPLETE.replace("mode = \"execute\"\n", "")).unwrap();
    assert_eq!(config.content[0].execution.mode, ExecutionMode::Never);
    let config = parse_configuration(
        &COMPLETE
            .replace("version_constraint = \"^1.9\"\n", "")
            .replace("provenance = \"explicit\"\n", "")
            .replace("url = \"https://forge.example/foo-python\"\n", ""),
    )
    .unwrap();
    assert_eq!(config.repositories[0].url, None);
    assert_eq!(config.relationships[0].version_constraint, None);
    assert_eq!(config.relationships[0].provenance, None);
}

// Paths describe positions in the input schema, independent of the Rust model.
fn table_mut<'a>(
    value: &'a mut toml::Value,
    path: &[&str],
) -> &'a mut toml::map::Map<String, toml::Value> {
    let mut current = value;
    for key in path {
        current = current.get_mut(*key).unwrap();
        if current.is_array() {
            current = &mut current.as_array_mut().unwrap()[0];
        }
    }
    current.as_table_mut().unwrap()
}

const TABLES: &[(&[&str], &[&str])] = &[
    (&[], &["project"]),
    (&["project"], &["name"]),
    (&["repository"], &["id", "path"]),
    (
        &["package"],
        &[
            "id",
            "name",
            "slug",
            "ecosystem",
            "repository",
            "path",
            "metadata_path",
            "targets",
        ],
    ),
    (
        &["package", "targets"],
        &["id", "extractor", "path", "role"],
    ),
    (
        &["content"],
        &["id", "owner", "repository", "path", "mount", "format"],
    ),
    (&["content", "execution"], &[]),
    (&["concept"], &["id", "kind", "members"]),
    (&["concept", "members"], &["package", "item"]),
    (&["relationship"], &["from", "to", "kind"]),
];

#[test]
fn rejects_missing_required_fields_in_each_section() {
    for (path, fields) in TABLES {
        for field in *fields {
            let mut value: toml::Value = toml::from_str(COMPLETE).unwrap();
            table_mut(&mut value, path).remove(*field).unwrap();
            let error = parse_configuration(&toml::to_string(&value).unwrap()).unwrap_err();
            assert!(
                error
                    .message()
                    .contains(&format!("missing field `{field}`")),
                "{path:?}.{field}: {error}"
            );
        }
    }
}

#[test]
fn rejects_unknown_fields_in_each_section() {
    for (path, _) in TABLES {
        let mut value: toml::Value = toml::from_str(COMPLETE).unwrap();
        table_mut(&mut value, path).insert("typo".into(), "ignored?".into());
        let error = parse_configuration(&toml::to_string(&value).unwrap()).unwrap_err();
        assert!(
            error.message().contains("unknown field `typo`"),
            "{path:?}: {error}"
        );
        assert!(error.span().is_some());
    }
}

#[test]
fn rejects_wrong_field_types_in_each_section() {
    for (path, _) in TABLES {
        let value: toml::Value = toml::from_str(COMPLETE).unwrap();
        let fields = table_mut(&mut value.clone(), path)
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for field in fields {
            let mut value = value.clone();
            table_mut(&mut value, path).insert(field.clone(), 42.into());
            assert!(
                parse_configuration(&toml::to_string(&value).unwrap()).is_err(),
                "{path:?}.{field}"
            );
        }
    }
}

#[test]
fn accepts_documented_enum_spellings_and_rejects_other_values() {
    let cases: &[(&[&str], &str, &[&str])] = &[
        (&["package"], "kind", &["package", "component"]),
        (
            &["package"],
            "visibility",
            &["public", "internal", "hidden"],
        ),
        (&["content"], "format", &["gfm", "qmd"]),
        (&["content", "execution"], "mode", &["never", "execute"]),
        (&["content", "execution"], "engine", &["jupyter"]),
        (
            &["concept"],
            "kind",
            &["equivalent", "analogous", "related"],
        ),
        (
            &["relationship"],
            "kind",
            &["depends-on", "binds", "wraps", "generated-from"],
        ),
    ];
    for (path, field, spellings) in cases {
        for spelling in *spellings {
            let mut value: toml::Value = toml::from_str(COMPLETE).unwrap();
            table_mut(&mut value, path).insert((*field).into(), (*spelling).into());
            let config = parse_configuration(&toml::to_string(&value).unwrap()).unwrap();
            let mut serialized: toml::Value =
                toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
            assert_eq!(
                table_mut(&mut serialized, path)[*field].as_str(),
                Some(*spelling)
            );
        }
        for spelling in ["unsupported".to_owned(), spellings[0].to_uppercase()] {
            let mut value: toml::Value = toml::from_str(COMPLETE).unwrap();
            table_mut(&mut value, path).insert((*field).into(), spelling.into());
            let error = parse_configuration(&toml::to_string(&value).unwrap()).unwrap_err();
            assert!(
                error.message().contains("unknown variant"),
                "{path:?}.{field}: {error}"
            );
        }
    }
}

#[test]
fn reports_malformed_toml_with_source_ranges() {
    for source in ["[project", "[project]\nname = 'Foo'\nname = 'Bar'\n"] {
        let error = parse_configuration(source).unwrap_err();
        let span = error.span().unwrap();
        assert!(span.start <= span.end && span.end <= source.len());
        assert!(!error.message().is_empty());
    }
}

#[test]
fn loader_reads_only_the_config_file_and_preserves_error_causes() {
    let workspace = support::TestWorkspace::new();
    workspace.write("nested/workspace.toml", COMPLETE);
    let path = workspace.path().join("nested/workspace.toml");
    assert_eq!(
        load_configuration(&path).unwrap(),
        parse_configuration(COMPLETE).unwrap()
    );

    workspace.write("nested/workspace.toml", "[project]\nname = 42\n");
    let error = load_configuration(&path).unwrap_err();
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.source().is_some());
    match error {
        ConfigurationError::Parse {
            path: source_path,
            source,
        } => {
            assert_eq!(source_path, path);
            assert!(source.span().is_some());
        }
        error => panic!("expected a parse error, got {error}"),
    }

    workspace.remove("nested/workspace.toml");
    let error = load_configuration(&path).unwrap_err();
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.source().is_some());
    match error {
        ConfigurationError::Read {
            path: source_path,
            source,
        } => {
            assert_eq!(source_path, path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        error => panic!("expected a read error, got {error}"),
    }
}
