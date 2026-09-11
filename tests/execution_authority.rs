use diplodocus::configuration::{
    ContentConfiguration, ExecutionConfigurationError, ExecutionMode, parse_configuration,
};
use diplodocus::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use diplodocus::documents::{AuthoredFormat, parse_authored_document, parse_collection_document};
use diplodocus::ir::{Block, MetadataValue, SourceSpan};
use diplodocus::validation::validate_document_execution;

mod support;

fn collection(execute: bool) -> ContentConfiguration {
    let execution = if execute {
        "[execution]\nmode = 'execute'\nengine = 'jupyter'\nkernel = 'uninstalled-kernel'\ndeclared_environment_inputs = ['missing.lock']\n"
    } else {
        ""
    };
    toml::from_str(&format!(
        "id = 'examples'\nowner = 'project'\nrepository = 'docs'\npath = 'missing'\nmount = 'examples'\nformat = 'qmd'\n{execution}"
    ))
    .unwrap()
}

fn page(metadata: &str) -> String {
    format!(
        "---\ntitle: Résumé 🦕\n{metadata}---\n\n```{{python}}\n#| eval: true\nraise RuntimeError('must not run')\n```\n"
    )
}

fn declaration_text(source: &str, span: SourceSpan) -> &str {
    source[span.start..span.end].trim()
}

fn assert_error(diagnostic: &Diagnostic, code: DiagnosticCode) {
    assert_eq!(diagnostic.code, code);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert!(diagnostic.span.is_some());
}

#[test]
fn acceptance_metadata_cannot_authorize_execution() {
    let config = parse_configuration(&support::load_fixture(
        "acceptance/workspace/diplodocus.toml",
    ))
    .unwrap();
    let collection = config
        .content
        .iter()
        .find(|collection| collection.path == std::path::Path::new("safety"))
        .unwrap();
    assert_eq!(collection.execution.mode, ExecutionMode::Never);
    let source = support::load_fixture(
        "acceptance-cases/document-execution-not-authorized/python/safety/metadata-cannot-authorize.qmd",
    );
    let parsed = parse_collection_document(&source, collection).unwrap();
    assert_eq!(parsed.diagnostics.len(), 1);
    let diagnostic = &parsed.diagnostics[0];
    assert_error(diagnostic, DiagnosticCode::DocumentExecutionNotAuthorized);
    assert_eq!(
        declaration_text(&source, diagnostic.span.unwrap()),
        "execute: true"
    );
    assert_eq!(
        diagnostic
            .related_spans
            .iter()
            .map(|span| declaration_text(&source, *span))
            .collect::<Vec<_>>(),
        ["jupyter: python3"]
    );
    assert_eq!(
        parsed.document,
        parse_authored_document(&source, AuthoredFormat::Qmd).document
    );
    let json = serde_json::to_string(&diagnostic).unwrap();
    assert!(json.contains("document-execution-not-authorized"));
    assert_eq!(
        serde_json::from_str::<Diagnostic>(&json).unwrap(),
        *diagnostic
    );
    assert_eq!(
        parsed,
        parse_collection_document(&source, collection).unwrap()
    );
    support::assert_json_golden(&parsed.diagnostics, "execution/authority.json");
}

#[test]
fn page_veto_and_cell_defaults_do_not_grant_collection_authority() {
    for execute in [false, true] {
        let collection = collection(execute);
        let before = collection.clone();
        for metadata in [
            "",
            "execute: false\n",
            "execute: {}\n",
            "execute:\n  eval: false\n  echo: false\n  output: asis\n  include: false\n  error: true\n",
            "execute: {eval: true, echo: true, output: true, include: true, error: false}\n",
        ] {
            let source = page(metadata);
            let parsed = parse_collection_document(&source, &collection).unwrap();
            assert!(
                parsed.diagnostics.is_empty(),
                "{metadata}: {:?}",
                parsed.diagnostics
            );
            assert_eq!(
                parsed.document,
                parse_authored_document(&source, AuthoredFormat::Qmd).document
            );
            assert_eq!(collection, before);
        }
    }
}

#[test]
fn explicit_execute_true_requires_an_authorized_collection() {
    let source = page("execute: true\n");
    assert!(
        parse_collection_document(&source, &collection(true))
            .unwrap()
            .diagnostics
            .is_empty()
    );
    let parsed = parse_collection_document(&source, &collection(false)).unwrap();
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_error(
        &parsed.diagnostics[0],
        DiagnosticCode::DocumentExecutionNotAuthorized,
    );
}

#[test]
fn document_selectors_are_rejected_even_when_matching_the_collection() {
    for declaration in [
        "jupyter: uninstalled-kernel\n",
        "jupyter: {kernelspec: {name: uninstalled-kernel}}\n",
        "engine: jupyter\n",
        "kernel: uninstalled-kernel\n",
        "execution: {mode: execute, engine: jupyter, kernel: uninstalled-kernel}\n",
        "execution: {mode: never}\n",
        "execute:\n  mode: execute\n",
        "execute:\n  mode: never\n",
        "execute:\n  engine: jupyter\n",
        "execute:\n  kernel: uninstalled-kernel\n",
        "execute: {kernel: uninstalled-kernel}\n",
        "'jupyter': uninstalled-kernel\n",
        "execute: {'engine': jupyter}\n",
        "jupyter: [invalid, selector]\n",
        "kernel:\n",
    ] {
        for execute in [false, true] {
            let parsed =
                parse_collection_document(&page(declaration), &collection(execute)).unwrap();
            assert_eq!(
                parsed.diagnostics.len(),
                1,
                "{declaration}: {:?}",
                parsed.diagnostics
            );
            assert_error(
                &parsed.diagnostics[0],
                if execute {
                    DiagnosticCode::UnsupportedQmdMetadata
                } else {
                    DiagnosticCode::DocumentExecutionNotAuthorized
                },
            );
        }
    }
}

#[test]
fn every_authority_declaration_is_retained_in_one_source_ordered_error() {
    let source = page(
        "kernel: python3\njupyter: ir\nexecute:\n  eval: false\n  kernel: ir\n  engine: jupyter\n",
    );
    let parsed = parse_collection_document(&source, &collection(false)).unwrap();
    assert_eq!(parsed.diagnostics.len(), 1);
    let diagnostic = &parsed.diagnostics[0];
    assert_error(diagnostic, DiagnosticCode::DocumentExecutionNotAuthorized);
    let declarations = std::iter::once(diagnostic.span.unwrap())
        .chain(diagnostic.related_spans.iter().copied())
        .map(|span| declaration_text(&source, span))
        .collect::<Vec<_>>();
    assert_eq!(
        declarations,
        [
            "kernel: python3",
            "jupyter: ir",
            "kernel: ir",
            "engine: jupyter"
        ]
    );
    let authorized = parse_collection_document(&source, &collection(true)).unwrap();
    assert_eq!(authorized.diagnostics.len(), 4);
    assert_eq!(
        authorized
            .diagnostics
            .iter()
            .map(|diagnostic| {
                assert_error(diagnostic, DiagnosticCode::UnsupportedQmdMetadata);
                declaration_text(&source, diagnostic.span.unwrap())
            })
            .collect::<Vec<_>>(),
        declarations
    );
}

#[test]
fn duplicate_metadata_remains_a_parser_error() {
    for metadata in [
        "execute: true\nexecute: false\n",
        "execute: false\nexecute: true\n",
        "execute: {kernel: python3, kernel: ir}\n",
        "jupyter: python3\njupyter: ir\n",
    ] {
        for execute in [false, true] {
            let source = page(metadata);
            let original = parse_authored_document(&source, AuthoredFormat::Qmd);
            let parsed = parse_collection_document(&source, &collection(execute)).unwrap();
            assert_eq!(parsed.diagnostics, original.diagnostics);
            assert_error(&parsed.diagnostics[0], DiagnosticCode::InvalidEmbeddedYaml);
        }
    }
}

#[test]
fn retained_duplicate_declarations_cannot_hide_an_authority_request() {
    let source = page("execute: true\n");
    let mut document = parse_authored_document(&source, AuthoredFormat::Qmd).document;
    let MetadataValue::Mapping { entries, .. } = document.frontmatter.as_mut().unwrap() else {
        panic!("metadata mapping");
    };
    let mut restriction = entries.last().unwrap().clone();
    let MetadataValue::Scalar { raw, value, .. } = &mut restriction.value else {
        panic!("execute scalar");
    };
    *raw = "false".to_owned();
    *value = "false".to_owned();
    entries.push(restriction);
    let diagnostics = validate_document_execution(&document, &collection(false)).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_error(
        &diagnostics[0],
        DiagnosticCode::DocumentExecutionNotAuthorized,
    );
}

#[test]
fn page_veto_does_not_hide_forbidden_selectors() {
    for execute in [false, true] {
        let source = page("execute: false\njupyter: python3\n");
        let parsed = parse_collection_document(&source, &collection(execute)).unwrap();
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            declaration_text(&source, parsed.diagnostics[0].span.unwrap()),
            "jupyter: python3"
        );
    }
}

#[test]
fn execute_requires_a_literal_boolean_or_mapping() {
    for value in [
        "'true'",
        "\"false\"",
        "yes",
        "FALSE",
        "1",
        "null",
        "",
        "[true]",
        "|\n  true",
    ] {
        for execute in [false, true] {
            let source = page(&format!("execute: {value}\n"));
            let parsed = parse_collection_document(&source, &collection(execute)).unwrap();
            assert_eq!(
                parsed.diagnostics.len(),
                1,
                "{value}: {:?}",
                parsed.diagnostics
            );
            assert_error(&parsed.diagnostics[0], DiagnosticCode::InvalidQmdMetadata);
        }
    }
}

#[test]
fn ordinary_text_and_cell_defaults_are_not_authority_declarations() {
    let source = "---\ntitle: 'execute: true'\naudience: [jupyter, engine, kernel]\nexecute: {eval: true}\n---\n\nexecute: true\n\n```{python}\n#| eval: true\njupyter = 'python3'\n```\n";
    assert!(
        parse_collection_document(source, &collection(false))
            .unwrap()
            .diagnostics
            .is_empty()
    );
    assert!(
        parse_collection_document("# No metadata\n", &collection(false))
            .unwrap()
            .diagnostics
            .is_empty()
    );
}

#[test]
fn parser_diagnostics_are_preserved_alongside_authority_errors() {
    let source = "---\nexecute: true\njupyter: python3\n---\n\n```{python, echo=true, echo=false}\npass\n```\n";
    let original = parse_authored_document(source, AuthoredFormat::Qmd);
    let parsed = parse_collection_document(source, &collection(false)).unwrap();
    assert_eq!(parsed.diagnostics.len(), original.diagnostics.len() + 1);
    assert_error(
        &parsed.diagnostics[0],
        DiagnosticCode::DocumentExecutionNotAuthorized,
    );
    assert_eq!(&parsed.diagnostics[1..], &original.diagnostics);
    assert_eq!(
        validate_document_execution(&original.document, &collection(false)).unwrap(),
        parsed.diagnostics[..1]
    );
}

#[test]
fn gfm_fences_remain_display_only_and_cannot_select_execution() {
    let mut collection = collection(false);
    collection.format = AuthoredFormat::Gfm;
    let source = "```{python}\nraise RuntimeError('must not run')\n```\n";
    let parsed = parse_collection_document(source, &collection).unwrap();
    assert!(parsed.diagnostics.is_empty());
    assert!(matches!(parsed.document.blocks[0], Block::CodeBlock { .. }));

    collection.execution = self::collection(true).execution;
    assert_eq!(
        parse_collection_document(source, &collection),
        Err(ExecutionConfigurationError::GfmExecution)
    );
    assert_eq!(
        validate_document_execution(&parsed.document, &collection),
        Err(ExecutionConfigurationError::GfmExecution)
    );
}

#[test]
fn modified_collection_configuration_is_revalidated() {
    let mut collection = collection(true);
    collection.execution.kernel = None;
    assert_eq!(
        parse_collection_document(&page("execute: false\n"), &collection),
        Err(ExecutionConfigurationError::MissingKernel)
    );
}
