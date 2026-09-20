use diplodocus::configuration::{
    ConfigurationError, ExecutionConfigurationError, load_configuration, parse_configuration,
};
use diplodocus::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};
use diplodocus::documents::{AuthoredFormat, parse_authored_document, parse_collection_document};
use diplodocus::ir::SourceSpan;
use diplodocus::paths::{
    PathResolutionError, PathResolutionErrorKind, PathType, resolve_workspace_paths,
};
use serde_json::json;

mod support;

fn path(value: &str) -> DiagnosticPath {
    DiagnosticPath::try_from(value).unwrap()
}

fn repository_source(repository: &str, value: &str) -> DiagnosticSource {
    DiagnosticSource::Repository {
        repository: repository.to_owned(),
        path: path(value),
    }
}

#[test]
fn context_serialization_preserves_existing_spans() {
    let source = "---\nexecute: true\njupyter: python3\n---\n";
    let collection = toml::from_str(
        "id = 'guide'\nowner = 'project'\nrepository = 'docs'\npath = 'guide'\nmount = 'guide'\nformat = 'qmd'\n",
    )
    .unwrap();
    let parsed = parse_collection_document(source, &collection).unwrap();
    let original = parsed.diagnostics[0].clone();
    let parsed = parsed.with_context(
        repository_source("docs", "guide/index.qmd"),
        DiagnosticEntity::Document {
            collection: "guide".to_owned(),
            path: path("index.qmd"),
        },
    );
    let diagnostic = &parsed.diagnostics[0];
    assert_eq!(diagnostic.span, original.span);
    assert_eq!(diagnostic.related_spans, original.related_spans);
    assert_eq!(diagnostic.related_spans.len(), 1);
    let value = serde_json::to_value(diagnostic).unwrap();
    assert_eq!(
        value,
        json!({
            "code": "document-execution-not-authorized",
            "severity": "error",
            "message": original.message,
            "span": original.span,
            "related_spans": original.related_spans,
            "source": {"kind": "repository", "repository": "docs", "path": "guide/index.qmd"},
            "related_entity": {"kind": "document", "collection": "guide", "path": "index.qmd"},
        })
    );
    assert_eq!(
        serde_json::from_value::<Diagnostic>(value).unwrap(),
        *diagnostic
    );
}

#[test]
fn context_is_optional_for_in_memory_and_legacy_diagnostics() {
    let parsed = parse_authored_document("<div>unsupported</div>\n", AuthoredFormat::Gfm);
    let diagnostic = &parsed.diagnostics[0];
    assert!(diagnostic.source.is_none());
    assert!(diagnostic.related_entity.is_none());
    let value = serde_json::to_value(diagnostic).unwrap();
    assert!(value.get("source").is_none());
    assert!(value.get("related_entity").is_none());
    assert_eq!(
        serde_json::from_value::<Diagnostic>(value).unwrap(),
        *diagnostic
    );
}

#[test]
fn portable_paths_reject_machine_paths_and_noncanonical_spellings() {
    for value in [
        "",
        "/tmp/docs.qmd",
        "C:/docs.qmd",
        "C:docs.qmd",
        "\\\\host\\docs",
        "docs\\index.qmd",
        "../docs.qmd",
        "docs/../index.qmd",
        "./docs.qmd",
        "docs//index.qmd",
        "docs/",
        "docs/./index.qmd",
        "docs\0.qmd",
    ] {
        assert!(DiagnosticPath::try_from(value).is_err(), "{value:?}");
        assert!(
            serde_json::from_value::<DiagnosticPath>(json!(value)).is_err(),
            "{value:?}"
        );
    }
    let portable = path("docs/Résumé 🦕.qmd");
    assert_eq!(portable.as_str(), "docs/Résumé 🦕.qmd");
    assert_eq!(
        serde_json::to_value(&portable).unwrap(),
        json!(portable.as_str())
    );
    assert_eq!(
        serde_json::from_value::<DiagnosticPath>(json!(portable.as_str())).unwrap(),
        portable
    );
}

fn diagnostic() -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::InvalidQmdMetadata,
        Severity::Error,
        "invalid metadata",
    )
    .with_source(repository_source("docs", "a.qmd"))
}

#[test]
fn ordering_has_explicit_tiebreakers_and_places_missing_locations_last() {
    let base = diagnostic();
    let mut positioned = base.clone();
    positioned.span = Some(SourceSpan { start: 2, end: 5 });
    assert!(positioned < base);
    let mut maximum_span = base.clone();
    maximum_span.span = Some(SourceSpan {
        start: usize::MAX,
        end: usize::MAX,
    });
    assert!(maximum_span < base);
    let mut later_end = positioned.clone();
    later_end.span.as_mut().unwrap().end = 6;
    assert!(positioned < later_end);
    let mut later_start = positioned.clone();
    later_start.span = Some(SourceSpan { start: 3, end: 4 });
    assert!(later_end < later_start);
    let mut later_code = positioned.clone();
    later_code.code = DiagnosticCode::UnsupportedQmdMetadata;
    assert!(positioned < later_code);
    let mut warning = positioned.clone();
    warning.severity = Severity::Warning;
    assert!(positioned < warning);
    let mut entity = positioned.clone();
    entity.related_entity = Some(DiagnosticEntity::Content {
        id: "guide".to_owned(),
    });
    assert!(entity < positioned);
    let mut message = positioned.clone();
    message.message = "other metadata".to_owned();
    assert!(positioned < message);
    let mut related = positioned.clone();
    related.related_spans.push(SourceSpan { start: 8, end: 9 });
    assert!(positioned < related);
    let mut later_related = related.clone();
    later_related.related_spans[0].end = 10;
    assert!(related < later_related);
    let mut later_source = positioned.clone();
    later_source.source = Some(repository_source("docs", "b.qmd"));
    assert!(base < later_source);
    let mut later_repository = positioned.clone();
    later_repository.source = Some(repository_source("other", "a.qmd"));
    assert!(later_source < later_repository);
    let mut unknown_source = positioned.clone();
    unknown_source.source = None;
    assert!(later_repository < unknown_source);
    let mut configuration = positioned.clone();
    configuration.source = Some(DiagnosticSource::Configuration {
        path: path("diplodocus.toml"),
    });
    assert!(configuration < positioned);

    let mut expected = vec![
        base,
        positioned,
        later_end,
        later_start,
        later_code,
        warning,
        entity,
        message,
        related,
        later_source,
        later_repository,
        unknown_source,
        configuration,
        maximum_span,
        later_related,
    ];
    expected.sort();
    for offset in 0..expected.len() {
        let mut shuffled = expected.clone();
        shuffled.rotate_left(offset);
        shuffled.reverse();
        shuffled.sort();
        assert_eq!(shuffled, expected);
        assert_eq!(
            serde_json::to_vec(&shuffled).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
    }
}

#[test]
fn entity_serialization_and_order_preserve_identity_scopes() {
    let entities = [
        json!({"kind": "concept", "id": "fit"}),
        json!({"kind": "configuration"}),
        json!({"kind": "configuration-field", "path": "package[0].slug"}),
        json!({"kind": "content", "id": "guide"}),
        json!({"kind": "document", "collection": "guide", "path": "index.qmd"}),
        json!({"kind": "item", "package": "pyfoo", "id": "fit"}),
        json!({"kind": "item", "package": "rfoo", "id": "fit"}),
        json!({"kind": "package", "id": "pyfoo"}),
        json!({"kind": "project"}),
        json!({"kind": "relationship", "index": 2}),
        json!({"kind": "relationship", "index": 10}),
        json!({"kind": "repository", "id": "docs"}),
        json!({"kind": "target", "package": "pyfoo", "id": "api"}),
        json!({"kind": "target", "package": "rfoo", "id": "api"}),
    ]
    .map(|value| {
        let entity = serde_json::from_value::<DiagnosticEntity>(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&entity).unwrap(), value);
        entity
    });
    assert!(entities.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn configuration_errors_keep_portable_context_and_available_spans() {
    let first = support::TestWorkspace::new();
    let second = support::TestWorkspace::new();
    for workspace in [&first, &second] {
        workspace.write("diplodocus.toml", "[project]\nname = 42\n");
    }
    let diagnostics = [&first, &second].map(|workspace| {
        let error = load_configuration(workspace.path().join("diplodocus.toml")).unwrap_err();
        let ConfigurationError::Parse { source, .. } = &error else {
            panic!("parse error");
        };
        let diagnostic = error.to_diagnostic(path("diplodocus.toml"));
        let span = source.span().unwrap();
        assert_eq!(
            diagnostic.span,
            Some(SourceSpan {
                start: span.start,
                end: span.end
            })
        );
        assert_eq!(diagnostic.code, DiagnosticCode::InvalidConfiguration);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::Configuration)
        );
        assert_eq!(
            diagnostic.source,
            Some(DiagnosticSource::Configuration {
                path: path("diplodocus.toml")
            })
        );
        assert!(
            !diagnostic
                .message
                .contains(workspace.path().to_str().unwrap())
        );
        diagnostic
    });
    assert_eq!(
        serde_json::to_vec(&diagnostics[0]).unwrap(),
        serde_json::to_vec(&diagnostics[1]).unwrap()
    );
    let error = load_configuration(first.path().join("missing.toml")).unwrap_err();
    let diagnostic = error.to_diagnostic(path("missing.toml"));
    assert_eq!(diagnostic.code, DiagnosticCode::ConfigurationReadFailed);
    assert!(diagnostic.span.is_none());
    assert!(
        !serde_json::to_string(&diagnostic)
            .unwrap()
            .contains(first.path().to_str().unwrap())
    );
}

#[test]
fn path_resolution_diagnostics_do_not_serialize_runtime_paths() {
    let mut diagnostics = Vec::new();
    for _ in 0..2 {
        let workspace = support::TestWorkspace::new();
        let configuration = parse_configuration(
            "[project]\nname = 'Docs'\n[[repository]]\nid = 'docs'\npath = 'missing'\n",
        )
        .unwrap();
        let error =
            resolve_workspace_paths(workspace.path().join("diplodocus.toml"), &configuration)
                .unwrap_err();
        let diagnostic = error.to_diagnostic(path("diplodocus.toml"));
        assert_eq!(diagnostic.code, DiagnosticCode::SourcePathIo);
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::ConfigurationField {
                path: "repository[0] (`docs`).path".to_owned()
            })
        );
        assert!(diagnostic.span.is_none());
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains(workspace.path().to_str().unwrap())
        );
        diagnostics.push(diagnostic);
    }
    assert_eq!(diagnostics[0], diagnostics[1]);
}

#[test]
fn execution_configuration_errors_can_identify_the_owning_collection() {
    let error = ExecutionConfigurationError::MissingKernel;
    let diagnostic = error.to_diagnostic("examples");
    assert_eq!(
        diagnostic.code,
        DiagnosticCode::InvalidExecutionConfiguration
    );
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.related_entity,
        Some(DiagnosticEntity::Content {
            id: "examples".to_owned()
        })
    );
    assert_eq!(diagnostic.message, error.to_string());
    assert!(diagnostic.span.is_none());
    assert!(diagnostic.source.is_none());
}

#[test]
fn stable_codes_match_their_serialized_identifiers() {
    use DiagnosticCode::*;
    for (code, spelling) in [
        (UnsupportedKernelMessage, "unsupported-kernel-message"),
        (UnsupportedAuthoredSyntax, "unsupported-authored-syntax"),
        (InvalidEmbeddedYaml, "invalid-embedded-yaml"),
        (AmbiguousCellOption, "ambiguous-cell-option"),
        (
            DocumentExecutionNotAuthorized,
            "document-execution-not-authorized",
        ),
        (UnsupportedQmdMetadata, "unsupported-qmd-metadata"),
        (InvalidQmdMetadata, "invalid-qmd-metadata"),
        (ConfigurationReadFailed, "configuration-read-failed"),
        (InvalidConfiguration, "invalid-configuration"),
        (
            InvalidExecutionConfiguration,
            "invalid-execution-configuration",
        ),
        (InvalidSourcePath, "invalid-source-path"),
        (SourcePathIo, "source-path-io"),
        (SourcePathWrongType, "source-path-wrong-type"),
        (SourcePathOutsideBoundary, "source-path-outside-boundary"),
        (InvalidRepositoryReference, "invalid-repository-reference"),
        (PythonMetadata, "python-metadata"),
        (PythonDynamicMetadata, "python-dynamic-metadata"),
        (PythonSyntax, "python-syntax"),
        (PythonUnsupportedVersion, "python-unsupported-version"),
        (PythonUnsupportedSyntax, "python-unsupported-syntax"),
        (PythonSourceRead, "python-source-read"),
        (PythonModuleCollision, "python-module-collision"),
        (PythonDynamicExport, "python-dynamic-export"),
        (PythonUnresolvedReexport, "python-unresolved-reexport"),
        (PythonConflictingStub, "python-conflicting-stub"),
        (PythonUnsupportedSurface, "python-unsupported-surface"),
        (PythonDuplicateIdentity, "python-duplicate-identity"),
        (PythonConflictingAlias, "python-conflicting-alias"),
        (PythonInvalidIdentity, "python-invalid-identity"),
        (PythonIncompleteDocstring, "python-incomplete-docstring"),
        (PythonUnsupportedDocstring, "python-unsupported-docstring"),
        (
            PythonDocstringSourceAttribution,
            "python-docstring-source-attribution",
        ),
    ] {
        assert_eq!(code.as_str(), spelling);
        assert_eq!(serde_json::to_value(code).unwrap(), json!(spelling));
        assert_eq!(
            serde_json::from_value::<DiagnosticCode>(json!(spelling)).unwrap(),
            code
        );
    }
    assert_eq!(
        serde_json::to_value(Severity::Warning).unwrap(),
        json!("warning")
    );
    assert_eq!(
        serde_json::to_value(Severity::Error).unwrap(),
        json!("error")
    );
}

#[test]
fn configuration_error_conversion_omits_even_custom_io_paths() {
    let error = ConfigurationError::Read {
        path: "/private/checkout/config.toml".into(),
        source: std::io::Error::other("cannot read /private/checkout/config.toml"),
    };
    let diagnostic = error.to_diagnostic(path("config.toml"));
    assert!(
        !serde_json::to_string(&diagnostic)
            .unwrap()
            .contains("/private")
    );
    let invalid: toml::de::Error = serde::de::Error::custom("invalid configuration");
    let diagnostic = ConfigurationError::Parse {
        path: "/private/checkout/config.toml".into(),
        source: invalid,
    }
    .to_diagnostic(path("config.toml"));
    assert!(diagnostic.span.is_none());
}

#[test]
fn path_failures_have_distinct_codes_and_portable_messages() {
    for (kind, code, message) in [
        (
            PathResolutionErrorKind::InvalidPath {
                path: "/private/checkout/input".into(),
                reason: "path must be relative",
            },
            DiagnosticCode::InvalidSourcePath,
            "invalid source path: path must be relative",
        ),
        (
            PathResolutionErrorKind::FileSystem {
                path: "/private/checkout/input".into(),
                source: std::io::Error::other("cannot inspect /private/checkout/input"),
            },
            DiagnosticCode::SourcePathIo,
            "could not inspect source path: other error",
        ),
        (
            PathResolutionErrorKind::WrongType {
                path: "/private/checkout/input".into(),
                expected: PathType::File,
            },
            DiagnosticCode::SourcePathWrongType,
            "source path must identify a regular file",
        ),
        (
            PathResolutionErrorKind::OutsideBoundary {
                path: "/private/checkout/input".into(),
                boundary: "/private/checkout/repo".into(),
            },
            DiagnosticCode::SourcePathOutsideBoundary,
            "source path escapes its declared boundary",
        ),
        (
            PathResolutionErrorKind::RepositoryReference {
                repository: "missing".to_owned(),
                matches: 0,
            },
            DiagnosticCode::InvalidRepositoryReference,
            "repository `missing` matches 0 declarations; expected exactly one",
        ),
    ] {
        let diagnostic = PathResolutionError {
            configuration_path: "/private/checkout/config.toml".into(),
            field: "package[0].path".to_owned(),
            kind,
        }
        .to_diagnostic(path("config.toml"));
        assert_eq!(diagnostic.code, code);
        assert_eq!(diagnostic.message, message);
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains("/private")
        );
    }
}
