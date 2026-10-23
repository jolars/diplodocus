use std::error::Error;
use std::path::PathBuf;

use diplodocus::configuration::{
    ConceptKind, ConfigurationError, ExecutionEngine, ExecutionMode, HomeActionIcon, PackageKind,
    PackageVisibility, RelationshipKind, load_configuration, parse_configuration,
};
use diplodocus::documents::AuthoredFormat;

mod support;

const COMPLETE: &str = include_str!("fixtures/configuration/complete.yaml");
const ACCEPTANCE: &str = include_str!("fixtures/acceptance/workspace/diplodocus.yaml");

const HOMEPAGE: &str = r#"project:
  name: Docs
presentation:
  home:
    tagline: Start here
    description: A guide
    guide: guide/index.html
    actions:
      - label: Read the guide
        path: guide/index.html
        icon: book-open
"#;

#[test]
fn homepage_action_icons_parse_and_serialize_as_named_icons() {
    for (name, icon) in [
        ("arrow-right", HomeActionIcon::ArrowRight),
        ("book-open", HomeActionIcon::BookOpen),
        ("code", HomeActionIcon::Code),
        ("download", HomeActionIcon::Download),
        ("terminal", HomeActionIcon::Terminal),
    ] {
        let config =
            parse_configuration(&HOMEPAGE.replace("icon: book-open", &format!("icon: {name}")))
                .unwrap();
        assert_eq!(
            config.presentation.home.as_ref().unwrap().actions[0].icon,
            Some(icon)
        );
        let serialized = serde_saphyr::to_string(&config).unwrap();
        assert_eq!(parse_configuration(&serialized).unwrap(), config);
        assert_eq!(
            serde_json::to_value(&config).unwrap()["presentation"]["home"]["actions"][0]["icon"],
            name
        );
    }
}

#[test]
fn homepage_actions_omit_unconfigured_icons() {
    for source in [
        HOMEPAGE.replace("        icon: book-open\n", ""),
        HOMEPAGE.replace("icon: book-open", "icon: null"),
    ] {
        let config = parse_configuration(&source).unwrap();
        assert_eq!(
            config.presentation.home.as_ref().unwrap().actions[0].icon,
            None
        );
        assert!(
            serde_json::to_value(&config).unwrap()["presentation"]["home"]["actions"][0]
                .get("icon")
                .is_none()
        );
        assert_eq!(
            parse_configuration(&serde_saphyr::to_string(&config).unwrap()).unwrap(),
            config
        );
    }
}

#[test]
fn homepage_action_icons_reject_unknown_names_and_non_string_values() {
    for value in [
        "unknown",
        "book_open",
        "BookOpen",
        "assets/book.svg",
        "'<svg></svg>'",
        "''",
        "42",
        "false",
        "[]",
        "{}",
    ] {
        let source = HOMEPAGE.replace("icon: book-open", &format!("icon: {value}"));
        assert!(parse_configuration(&source).is_err(), "{value}");
    }
    let error = parse_configuration(&HOMEPAGE.replace("icon: book-open", "icon: unknown"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown"), "{error}");
    assert!(error.contains("book-open"), "{error}");
}

fn kebab_case_configuration() -> String {
    let mut source = COMPLETE.to_owned();
    source.push_str(
        "presentation:\n  title: null\n  description: null\n  site-url: \"https://example.test/docs/\"\n  canonical-url-style: \"clean\"\n",
    );
    source.push_str(
        "  footer:\n    docs-version: \"dev\"\n    copyright: \"© 2026 Contributors\"\n    licenses:\n      - label: \"MIT\"\n        url: \"https://example.test/LICENSE\"\n    links:\n      - label: \"GitHub\"\n        url: \"https://example.test/repo\"\n    built-with: true\n",
    );
    source
}

#[test]
fn configuration_keys_parse_and_serialize_in_kebab_case() {
    let source = kebab_case_configuration();
    let config = parse_configuration(&source).unwrap();
    let serialized = serde_saphyr::to_string(&config).unwrap();
    assert_eq!(
        serde_saphyr::from_str::<serde_json::Value>(&serialized).unwrap(),
        serde_saphyr::from_str::<serde_json::Value>(&source).unwrap()
    );
    assert_eq!(parse_configuration(&serialized).unwrap(), config);
}

#[test]
fn configuration_rejects_snake_case_keys() {
    let source = kebab_case_configuration();
    for field in [
        "source-link-template",
        "metadata-path",
        "declared-environment-inputs",
        "version-constraint",
        "site-url",
        "canonical-url-style",
        "docs-version",
        "built-with",
    ] {
        let old_field = field.replace('-', "_");
        let error = parse_configuration(&source.replace(field, &old_field)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("unknown field `{old_field}`")),
            "{error}"
        );
    }
}

#[test]
fn presentation_site_urls_require_absolute_http_urls_without_request_metadata() {
    for url in [
        "",
        "/docs",
        "example.test",
        "ftp://example.test/",
        "https:///",
        "https://user:pass@example.test/",
        "https://example.test/?q=1",
        "https://example.test/#top",
        " https://example.test/",
        "https://example.test/a b",
        "https://example.test/\n",
    ] {
        let source = format!(
            "project:\n  name: \"Docs\"\npresentation:\n  site-url: {}\n",
            serde_json::Value::String(url.into())
        );
        assert!(parse_configuration(&source).is_err(), "{url:?}");
    }
    for style in ["file", "clean"] {
        let source = format!(
            "project:\n  name: \"Docs\"\npresentation:\n  site-url: \"https://example.test/docs\"\n  canonical-url-style: \"{style}\"\n"
        );
        let configuration = parse_configuration(&source).unwrap();
        assert_eq!(
            parse_configuration(&serde_saphyr::to_string(&configuration).unwrap()).unwrap(),
            configuration
        );
    }
    assert!(
        parse_configuration(
            "project:\n  name: \"Docs\"\npresentation:\n  canonical-url-style: \"unknown\"\n"
        )
        .is_err()
    );
}

#[test]
fn presentation_settings_accept_only_portable_display_metadata() {
    for settings in [
        "title: 42",
        "description: false",
        "theme_path: \"/tmp/theme\"",
        "execute: true",
    ] {
        assert!(
            parse_configuration(&format!(
                "project:\n  name: \"Docs\"\npresentation:\n  {settings}\n\n"
            ))
            .is_err()
        );
    }
    let config = parse_configuration(
        "project:\n  name: \"Docs\"\npresentation:\n  title: \"Documentation\"\n  description: \"A guide\"\n",
    )
    .unwrap();
    assert_eq!(
        parse_configuration(&serde_saphyr::to_string(&config).unwrap()).unwrap(),
        config
    );
}

#[test]
fn logo_declarations_require_a_repository_and_portable_image_path() {
    let source = "project:\n  name: \"Docs\"\npresentation:\n  logo:\n    repository: \"docs\"\n    path: \"assets/logo.svg\"\n";
    let config = parse_configuration(source).unwrap();
    assert_eq!(
        parse_configuration(&serde_saphyr::to_string(&config).unwrap()).unwrap(),
        config
    );
    for settings in [
        "repository: \"docs\"",
        "path: \"logo.svg\"",
        "repository: \"docs\"\npath: 42",
        "repository: \"docs\"\npath: \"/logo.svg\"",
        "repository: \"docs\"\npath: \"../logo.svg\"",
        "repository: \"docs\"\npath: \"./logo.svg\"",
        "repository: \"docs\"\npath: \"https://example.test/logo.svg\"",
        "repository: \"docs\"\npath: \"logo.svg\"\nunknown: true",
    ] {
        assert!(
            parse_configuration(&format!(
                "project:\n  name: \"Docs\"\npresentation:\n  logo:\n    {}\n",
                settings.replace('\n', "\n    ")
            ))
            .is_err(),
            "{settings}"
        );
    }
}

#[test]
fn footer_links_reject_unsafe_or_ambiguous_urls() {
    for field in ["licenses", "links"] {
        for url in [
            "javascript:alert(1)",
            "data:text/html,hello",
            "file:///tmp/license",
            "//example.test/license",
            "license.html",
            "https:example.test",
            "https:///example.test",
            "https://user:pass@example.test/",
            "https://@example.test/",
            " https://example.test/",
            "https://example.test/a b",
            "https://example.test/\n",
        ] {
            let source = format!(
                "project:\n  name: Docs\npresentation:\n  footer:\n    {field}: [{{label: License, url: {}}}]\n",
                serde_json::Value::String(url.into())
            );
            assert!(parse_configuration(&source).is_err(), "{field}: {url:?}");
        }
    }
    for settings in [
        "docs-version: 1",
        "copyright: false",
        "built-with: \"yes\"",
        "links: [{ url: \"https://example.test/\" }]",
        "licenses: [{ label: \"MIT\" }]",
        "unknown: true",
    ] {
        assert!(
            parse_configuration(&format!(
                "project:\n  name: \"Docs\"\npresentation:\n  footer:\n    {}\n",
                settings.replace('\n', "\n    ")
            ))
            .is_err(),
            "{settings}"
        );
    }
}

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
    let serialized = serde_saphyr::to_string(&config).unwrap();
    assert_eq!(
        serde_saphyr::from_str::<serde_json::Value>(&serialized).unwrap(),
        serde_saphyr::from_str::<serde_json::Value>(COMPLETE).unwrap()
    );
    assert_eq!(parse_configuration(&serialized).unwrap(), config);
}

#[test]
fn parses_design_examples() {
    let design = include_str!("../DESIGN.md");
    let mut examples = 0;
    for block in design.split("```yaml\n").skip(1) {
        let source = block.split_once("```").unwrap().0;
        let source = if source.starts_with("concepts:") {
            format!("project:\n  name: \"Concept example\"\n{source}")
        } else {
            source.to_owned()
        };
        parse_configuration(&source).unwrap();
        examples += 1;
    }
    assert!(
        examples > 0,
        "DESIGN.md should contain a YAML configuration example"
    );
}

#[test]
fn parses_acceptance_workspace_variants() {
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
}

#[test]
fn applies_only_documented_defaults_without_discovering_inputs() {
    let config = parse_configuration("project:\n  name: \"Empty workspace\"\n").unwrap();
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

    assert!(parse_configuration(&COMPLETE.replace("      mode: \"execute\"\n", "")).is_err());
    let config = parse_configuration(
        &COMPLETE
            .replace("    version-constraint: \"^1.9\"\n", "")
            .replace("    provenance: \"explicit\"\n", "")
            .replace("    url: \"https://forge.example/foo-python\"\n", ""),
    )
    .unwrap();
    assert_eq!(config.repositories[0].url, None);
    assert_eq!(config.relationships[0].version_constraint, None);
    assert_eq!(config.relationships[0].provenance, None);
}

#[test]
fn package_defaults_are_independent_of_explicit_kind_and_visibility() {
    let kinds = [
        (None, PackageKind::Package),
        (Some("package"), PackageKind::Package),
        (Some("component"), PackageKind::Component),
    ];
    let visibilities = [
        (None, PackageVisibility::Public),
        (Some("public"), PackageVisibility::Public),
        (Some("internal"), PackageVisibility::Internal),
        (Some("hidden"), PackageVisibility::Hidden),
    ];
    for (kind, expected_kind) in kinds {
        for (visibility, expected_visibility) in visibilities {
            let mut value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
            let package = table_mut(&mut value, &["packages"]);
            package.remove("kind");
            package.remove("visibility");
            if let Some(kind) = kind {
                package.insert("kind".into(), kind.into());
            }
            if let Some(visibility) = visibility {
                package.insert("visibility".into(), visibility.into());
            }

            let config = parse_configuration(&serde_saphyr::to_string(&value).unwrap()).unwrap();
            assert_eq!(config.packages[0].kind, expected_kind);
            assert_eq!(config.packages[0].visibility, expected_visibility);
            let serialized = serde_saphyr::to_string(&config).unwrap();
            assert_eq!(parse_configuration(&serialized).unwrap(), config);
        }
    }
}

#[test]
fn omitted_and_explicitly_empty_workspace_inputs_are_equivalent() {
    let minimal = "project:\n  name: \"Empty workspace\"\n";
    let empty = format!("repositories: []\npackages: []\n{minimal}");
    let expected = parse_configuration(minimal).unwrap();
    let config = parse_configuration(&empty).unwrap();
    assert_eq!(config, expected);
    assert!(config.repositories.is_empty());
    assert!(config.packages.is_empty());
    assert_eq!(
        parse_configuration(&serde_saphyr::to_string(&config).unwrap()).unwrap(),
        config
    );
}

#[test]
fn loading_populated_sources_never_discovers_workspace_inputs() {
    let workspace = support::acceptance_workspace();
    let path = workspace.path().join("workspace/diplodocus.yaml");

    workspace.write(
        "workspace/diplodocus.yaml",
        "project:\n  name: \"Minimal\"\n",
    );
    let minimal = load_configuration(&path).unwrap();
    assert!(minimal.repositories.is_empty());
    assert!(minimal.packages.is_empty());

    let mut value: serde_json::Value = serde_saphyr::from_str(ACCEPTANCE).unwrap();
    let mut packages = value.as_object_mut().unwrap().remove("packages").unwrap();
    value.as_object_mut().unwrap().remove("concepts");
    value.as_object_mut().unwrap().remove("relationships");
    value["content"]
        .as_array_mut()
        .unwrap()
        .retain(|collection| collection["owner"].as_str() == Some("project"));
    workspace.write(
        "workspace/diplodocus.yaml",
        serde_saphyr::to_string(&value).unwrap(),
    );
    let docs_only = load_configuration(&path).unwrap();
    assert_eq!(docs_only.repositories.len(), 3);
    assert!(docs_only.packages.is_empty());
    assert_eq!(docs_only.content.len(), 1);

    for package in packages.as_array_mut().unwrap() {
        package["targets"] = serde_json::Value::Array(Vec::new());
    }
    value
        .as_object_mut()
        .unwrap()
        .insert("packages".into(), packages);
    workspace.write(
        "workspace/diplodocus.yaml",
        serde_saphyr::to_string(&value).unwrap(),
    );
    let config = load_configuration(&path).unwrap();
    assert_eq!(config.repositories.len(), 3);
    assert_eq!(config.packages.len(), 2);
    for package in &config.packages {
        assert!(package.targets.is_empty());
    }
    let serialized = serde_saphyr::to_string(&config).unwrap();
    let serialized_value: serde_json::Value = serde_saphyr::from_str(&serialized).unwrap();
    for package in serialized_value["packages"].as_array().unwrap() {
        assert!(package["targets"].as_array().unwrap().is_empty());
    }
    assert_eq!(parse_configuration(&serialized).unwrap(), config);
}

#[test]
fn loading_requires_explicit_targets_even_when_package_sources_are_present() {
    let workspace = support::acceptance_workspace();
    let path = workspace.path().join("workspace/diplodocus.yaml");
    let mut value: serde_json::Value = serde_saphyr::from_str(ACCEPTANCE).unwrap();
    table_mut(&mut value, &["packages"]).remove("targets");
    workspace.write(
        "workspace/diplodocus.yaml",
        serde_saphyr::to_string(&value).unwrap(),
    );

    let ConfigurationError::Parse {
        path: source_path,
        source,
    } = load_configuration(&path).unwrap_err()
    else {
        panic!("expected a parse error");
    };
    assert_eq!(source_path, path);
    assert!(source.to_string().contains("missing field `targets`"));
    assert!(source.location().is_some());
}

// Paths describe positions in the input schema, independent of the Rust model.
fn table_mut<'a>(
    value: &'a mut serde_json::Value,
    path: &[&str],
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    let mut current = value;
    for key in path {
        current = current.get_mut(*key).unwrap();
        if current.is_array() {
            current = &mut current.as_array_mut().unwrap()[0];
        }
    }
    current.as_object_mut().unwrap()
}

const TABLES: &[(&[&str], &[&str])] = &[
    (&[], &["project"]),
    (&["project"], &["name"]),
    (&["repositories"], &["id", "path"]),
    (
        &["packages"],
        &[
            "id",
            "name",
            "slug",
            "ecosystem",
            "repository",
            "path",
            "metadata-path",
            "targets",
        ],
    ),
    (
        &["packages", "targets"],
        &["id", "extractor", "path", "role"],
    ),
    (
        &["content"],
        &["id", "owner", "repository", "path", "mount", "format"],
    ),
    (&["content", "execution"], &[]),
    (&["concepts"], &["id", "kind", "members"]),
    (&["concepts", "members"], &["package", "item"]),
    (&["relationships"], &["from", "to", "kind"]),
];

#[test]
fn rejects_missing_required_fields_in_each_section() {
    for (path, fields) in TABLES {
        for field in *fields {
            let mut value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
            table_mut(&mut value, path).remove(*field).unwrap();
            let error = parse_configuration(&serde_saphyr::to_string(&value).unwrap()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{field}`")),
                "{path:?}.{field}: {error}"
            );
        }
    }
}

#[test]
fn rejects_unknown_fields_in_each_section() {
    for (path, _) in TABLES {
        let mut value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
        table_mut(&mut value, path).insert("typo".into(), "ignored?".into());
        let error = parse_configuration(&serde_saphyr::to_string(&value).unwrap()).unwrap_err();
        assert!(
            error.to_string().contains("unknown field `typo`"),
            "{path:?}: {error}"
        );
        assert!(error.location().is_some());
    }
}

#[test]
fn rejects_wrong_field_types_in_each_section() {
    for (path, _) in TABLES {
        let value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
        let fields = table_mut(&mut value.clone(), path)
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for field in fields {
            let mut value = value.clone();
            table_mut(&mut value, path).insert(field.clone(), 42.into());
            assert!(
                parse_configuration(&serde_saphyr::to_string(&value).unwrap()).is_err(),
                "{path:?}.{field}"
            );
        }
    }
}

#[test]
fn accepts_documented_enum_spellings_and_rejects_other_values() {
    let cases: &[(&[&str], &str, &[&str])] = &[
        (&["packages"], "kind", &["package", "component"]),
        (
            &["packages"],
            "visibility",
            &["public", "internal", "hidden"],
        ),
        (&["content"], "format", &["gfm", "qmd"]),
        (&["content", "execution"], "mode", &["never", "execute"]),
        (&["content", "execution"], "engine", &["jupyter"]),
        (
            &["concepts"],
            "kind",
            &["equivalent", "analogous", "related"],
        ),
        (
            &["relationships"],
            "kind",
            &["depends-on", "binds", "wraps", "generated-from"],
        ),
    ];
    for (path, field, spellings) in cases {
        for spelling in *spellings {
            let mut value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
            if (*field == "format" && *spelling == "gfm")
                || (*field == "mode" && *spelling == "never")
            {
                table_mut(&mut value, &["content", "execution"]).clear();
            }
            table_mut(&mut value, path).insert((*field).into(), (*spelling).into());
            let config = parse_configuration(&serde_saphyr::to_string(&value).unwrap()).unwrap();
            let mut serialized: serde_json::Value =
                serde_saphyr::from_str(&serde_saphyr::to_string(&config).unwrap()).unwrap();
            assert_eq!(
                table_mut(&mut serialized, path)[*field].as_str(),
                Some(*spelling)
            );
        }
        for spelling in ["unsupported".to_owned(), spellings[0].to_uppercase()] {
            let mut value: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
            table_mut(&mut value, path).insert((*field).into(), spelling.into());
            let error = parse_configuration(&serde_saphyr::to_string(&value).unwrap()).unwrap_err();
            assert!(
                error.to_string().contains("unknown variant"),
                "{path:?}.{field}: {error}"
            );
        }
    }
}

#[test]
fn reports_malformed_yaml_with_source_ranges() {
    for source in ["project: [", "project:\n  name: Foo\n  name: Bar\n"] {
        let error = parse_configuration(source).unwrap_err();
        let span = support::configuration_error_span(&error).unwrap();
        assert!(span.start <= span.end && span.end <= source.len());
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn loader_reads_only_the_config_file_and_preserves_error_causes() {
    let workspace = support::TestWorkspace::new();
    workspace.write("nested/workspace.yaml", COMPLETE);
    let path = workspace.path().join("nested/workspace.yaml");
    assert_eq!(
        load_configuration(&path).unwrap(),
        parse_configuration(COMPLETE).unwrap()
    );

    workspace.write("nested/workspace.yaml", "project:\n  name: 42\n");
    let error = load_configuration(&path).unwrap_err();
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.source().is_some());
    match error {
        ConfigurationError::Parse {
            path: source_path,
            source,
        } => {
            assert_eq!(source_path, path);
            assert!(source.location().is_some());
        }
        error => panic!("expected a parse error, got {error}"),
    }

    workspace.remove("nested/workspace.yaml");
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

#[test]
fn yaml_rejects_legacy_collection_keys_and_toml_syntax() {
    for key in ["repository", "package", "concept", "relationship"] {
        let error =
            parse_configuration(&format!("project:\n  name: Docs\n{key}: []\n")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("unknown field `{key}`")),
            "{error}"
        );
    }
    assert!(parse_configuration("[project]\nname = 'Docs'\n").is_err());
}

#[test]
fn yaml_rejects_duplicate_keys_and_multiple_documents() {
    for source in [
        "project:\n  name: First\n  name: Second\n",
        "project: {name: First}\nproject: {name: Second}\n",
        "project: {name: First}\n---\nproject: {name: Second}\n",
        "project: {name: First}\n---\n",
    ] {
        assert!(parse_configuration(source).is_err(), "{source}");
    }
}

#[test]
fn yaml_preserves_multiline_text_and_declared_list_order() {
    let config = parse_configuration(
        "project:\n  name: Docs\npresentation:\n  description: |-\n    First line.\n    Second line.\nrepositories:\n  - {id: second, path: ../second}\n  - {id: first, path: ../first}\n",
    ).unwrap();
    assert_eq!(
        config.presentation.description.as_deref(),
        Some("First line.\nSecond line.")
    );
    assert_eq!(
        config
            .repositories
            .iter()
            .map(|repository| repository.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
}

#[test]
fn loading_yaml_uses_the_explicit_filename_without_extension_inference() {
    let workspace = support::TestWorkspace::new();
    for name in ["diplodocus.yaml", "custom.yml", "workspace.config"] {
        workspace.write(name, "project:\n  name: Docs\n");
        assert_eq!(
            load_configuration(workspace.path().join(name))
                .unwrap()
                .project
                .name,
            "Docs"
        );
    }
}

#[test]
fn yaml_boolean_fields_preserve_scalar_types() {
    for value in [
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!("true"),
        serde_json::json!("false"),
    ] {
        let mut configuration: serde_json::Value = serde_saphyr::from_str(COMPLETE).unwrap();
        configuration["presentation"] = serde_json::json!({"footer": {"built-with": value}});
        let source = serde_saphyr::to_string(&configuration).unwrap();
        assert_eq!(
            parse_configuration(&source).is_ok(),
            value.is_boolean(),
            "{source}"
        );
        configuration
            .as_object_mut()
            .unwrap()
            .remove("presentation");
        configuration["packages"][0]["targets"][0]["rust"] =
            serde_json::json!({"default-features": value});
        let source = serde_saphyr::to_string(&configuration).unwrap();
        assert_eq!(
            parse_configuration(&source).is_ok(),
            value.is_boolean(),
            "{source}"
        );
    }
}
