use diplodocus::configuration::{ContentConfiguration, ExecutionConfigurationError};
use diplodocus::diagnostics::{DiagnosticCode, Severity};
use diplodocus::documents::{
    AuthoredFormat, PreparedDocument, parse_authored_document, parse_collection_document,
    prepare_collection_document,
};
use diplodocus::execution::{OptionOrigin, OutputVisibility};
use diplodocus::ir::SourceSpan;
use serde_json::json;

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

fn prepare(source: &str, execute: bool) -> PreparedDocument {
    let collection = collection(execute);
    let result = prepare_collection_document(source, &collection).unwrap();
    assert_eq!(
        result.parsed,
        parse_collection_document(source, &collection).unwrap()
    );
    assert_eq!(
        result.parsed.document,
        parse_authored_document(source, AuthoredFormat::Qmd).document
    );
    result
}

fn text(source: &str, span: SourceSpan) -> &str {
    source[span.start..span.end].trim()
}

#[test]
fn defaults_and_all_precedence_tiers_retain_their_origins() {
    let source = "---\ntitle: Résumé 🦕\naudience: [Python, R]\nexecute: {eval: false, echo: false, output: false}\n---\n\n```{python #setup, echo=true, output=asis}\n#| echo: false\n#| eval: true\n#| fig-alt: 'A **literal** figure'\n#| fig-cap: Caption\n#| fig-subcap: [Second, First]\nprint('unchanged')\n```\n";
    let result = prepare(source, true);
    assert!(
        result.parsed.diagnostics.is_empty(),
        "{:?}",
        result.parsed.diagnostics
    );
    let prepared = result.preparation.unwrap();
    assert!(prepared.execution_eligible);
    assert!(!prepared.page_veto);
    assert!(!prepared.defaults.eval.value);
    let OptionOrigin::Document { span } = prepared.defaults.eval.origin else {
        panic!("document origin")
    };
    assert_eq!(text(source, span), "eval: false");
    let cell = &prepared.cells[0];
    assert_eq!(cell.ordinal, 0);
    assert_eq!(cell.cell.source, "print('unchanged')\n");
    assert!(cell.cell.outputs.is_empty());
    let options = &cell.options;
    assert!(options.execution.eval.value);
    assert!(!options.execution.echo.value);
    assert_eq!(options.execution.output.value, OutputVisibility::AsIs);
    assert!(options.execution.include.value);
    assert!(!options.execution.error.value);
    assert_eq!(options.execution.error.origin, OptionOrigin::Default);
    let OptionOrigin::Hashpipe { span } = options.execution.echo.origin else {
        panic!("hashpipe origin")
    };
    assert_eq!(
        span,
        cell.cell
            .options
            .iter()
            .find(|option| {
                option.canonical_key.as_deref() == Some("echo")
                    && option.source == diplodocus::ir::CellOptionSource::HashpipeYaml
            })
            .unwrap()
            .span
    );
    assert!(text(source, span).starts_with("echo: false"));
    let OptionOrigin::Inline { span } = options.execution.output.origin else {
        panic!("inline origin")
    };
    assert_eq!(text(source, span), "output=asis");
    assert_eq!(options.label.value.as_deref(), Some("setup"));
    assert!(matches!(
        options.label.origin,
        OptionOrigin::FenceIdentifier { .. }
    ));
    assert_eq!(
        options.fig_alt.value.as_deref(),
        Some("A **literal** figure")
    );
    assert_eq!(options.fig_cap.value.as_deref(), Some("Caption"));
    assert_eq!(options.fig_subcap.value, ["Second", "First"]);
}

#[test]
fn invalid_values_are_errors_even_when_overridden_or_disabled() {
    for value in [
        "'true'",
        "\"false\"",
        "TRUE",
        "FALSE",
        "yes",
        "no",
        "1",
        "null",
        "",
        "[true]",
        "{a: true}",
    ] {
        for execute in [false, true] {
            for source in [
                format!("```{{python, echo={value}}}\n#| eval: false\n#| echo: true\npass\n```\n"),
                format!("---\nexecute: false\n---\n```{{python}}\n#| echo: {value}\npass\n```\n"),
            ] {
                let result = prepare(&source, execute);
                assert!(result.preparation.is_none(), "{source}");
                assert!(
                    result
                        .parsed
                        .diagnostics
                        .iter()
                        .any(|d| d.severity == Severity::Error),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn unsupported_options_and_classes_keep_declaration_ranges() {
    for declaration in [
        "warning: false",
        "message: false",
        "results: asis",
        "cache: true",
        "timeout: 1",
        "fig-width: 4",
        "tags: [hide]",
        "classes: [hidden]",
    ] {
        let source = format!("```{{python}}\n#| eval: false\n#| {declaration}\npass\n```\n");
        let result = prepare(&source, false);
        assert!(result.preparation.is_none());
        assert_eq!(
            result.parsed.diagnostics.len(),
            1,
            "{source}: {:?}",
            result.parsed.diagnostics
        );
        let diagnostic = &result.parsed.diagnostics[0];
        assert_eq!(diagnostic.code, DiagnosticCode::UnsupportedCellOption);
        assert_eq!(text(&source, diagnostic.span.unwrap()), declaration);
    }
    let result = prepare("```{python .hidden}\npass\n```\n", true);
    assert!(result.preparation.is_none());
    assert_eq!(
        result.parsed.diagnostics[0].code,
        DiagnosticCode::UnsupportedCellOption
    );
}

#[test]
fn duplicate_options_are_errors_in_every_tier_without_duplicate_warnings() {
    for source in [
        "```{python, echo=true, echo=false}\npass\n```\n",
        "```{python, echo=true, echo=false}\n#| echo: true\npass\n```\n",
    ] {
        let result = prepare(source, false);
        assert!(result.preparation.is_none());
        assert_eq!(result.parsed.diagnostics.len(), 1);
        let diagnostic = &result.parsed.diagnostics[0];
        assert_eq!(diagnostic.code, DiagnosticCode::AmbiguousCellOption);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(text(source, diagnostic.span.unwrap()), "echo=true");
        assert_eq!(diagnostic.related_spans.len(), 1);
        assert_eq!(text(source, diagnostic.related_spans[0]), "echo=false");
    }
    for source in [
        "---\ntitle: First\ntitle: Second\n---\n",
        "---\nexecute: {echo: true, echo: false}\n---\n",
        "```{python}\n#| echo: true\n#| echo: false\npass\n```\n",
    ] {
        let result = prepare(source, true);
        assert!(result.preparation.is_none());
        assert!(
            result
                .parsed
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::InvalidEmbeddedYaml)
        );
    }
}

#[test]
fn metadata_policy_rejects_unknown_keys_and_bad_types() {
    for metadata in [
        "format: html",
        "params: {}",
        "Title: Example",
        "execute: {fig-alt: text}",
    ] {
        let result = prepare(&format!("---\n{metadata}\n---\n"), true);
        assert!(result.preparation.is_none());
        assert_eq!(
            result.parsed.diagnostics[0].code,
            DiagnosticCode::UnsupportedQmdMetadata
        );
    }
    for metadata in [
        "title: ''",
        "title: []",
        "title:",
        "audience: {}",
        "audience: [okay, []]",
        "execute: {eval: 'true'}",
        "execute: {output: other}",
    ] {
        let result = prepare(&format!("---\n{metadata}\n---\n"), true);
        assert!(result.preparation.is_none(), "{metadata}");
        assert_eq!(
            result.parsed.diagnostics[0].code,
            DiagnosticCode::InvalidQmdMetadata,
            "{metadata}"
        );
    }
    for metadata in [
        "title: Example",
        "audience: Python",
        "audience: []",
        "execute: true",
        "execute: {output: 'asis'}",
    ] {
        let result = prepare(&format!("---\n{metadata}\n---\n"), true);
        assert!(
            result.parsed.diagnostics.is_empty(),
            "{metadata}: {:?}",
            result.parsed.diagnostics
        );
        assert!(result.preparation.is_some());
    }
}

#[test]
fn yaml_tags_aliases_anchors_and_merges_are_not_interpreted() {
    for metadata in [
        "title: !custom text",
        "title: &title text",
        "title: *title",
        "audience: [!custom text]",
        "execute: !!map {eval: true}",
        "execute: {<<: {eval: true}}",
    ] {
        let result = prepare(&format!("---\n{metadata}\n---\n"), true);
        assert!(
            result.preparation.is_none(),
            "{metadata}: {:?}",
            result.parsed
        );
    }
    for option in [
        "fig-alt: !custom text",
        "fig-alt: &alt text",
        "fig-alt: *alt",
        "fig-subcap: [!custom text]",
        "<<: {echo: false}",
    ] {
        let result = prepare(&format!("```{{python}}\n#| {option}\npass\n```\n"), true);
        assert!(
            result.preparation.is_none(),
            "{option}: {:?}",
            result.parsed
        );
    }
}

#[test]
fn labels_validate_fallback_agreement_and_page_anchor_collisions() {
    let result = prepare("```{python #setup}\n#| label: setup\npass\n```\n", true);
    assert!(result.parsed.diagnostics.is_empty());
    assert!(matches!(
        result.preparation.unwrap().cells[0].options.label.origin,
        OptionOrigin::Hashpipe { .. }
    ));
    for source in [
        "```{python #setup}\n#| label: other\npass\n```\n",
        "```{python}\n#| label: '9invalid'\npass\n```\n",
        "```{python #café}\npass\n```\n",
        "```{python}\n#| label: ''\npass\n```\n",
        "```{python #same}\npass\n```\n\n```{r #same}\n#| eval: false\n1\n```\n",
        "# Heading {#same}\n\n```{python #same}\npass\n```\n",
        "```{python #same}\npass\n```\n\n::: {.callout-note #same}\nNote.\n:::\n",
        "[Link](https://example.com){#same}\n\n```{python #same}\npass\n```\n",
    ] {
        let result = prepare(source, true);
        assert!(result.preparation.is_none(), "{source}");
        assert!(
            result
                .parsed
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::InvalidCellOption),
            "{source}: {:?}",
            result.parsed.diagnostics
        );
    }
}

#[test]
fn figure_options_validate_types_without_counting_nonexistent_outputs() {
    for option in [
        "fig-alt: []",
        "fig-cap: {}",
        "fig-subcap: caption",
        "fig-subcap: [one, {}]",
    ] {
        let result = prepare(&format!("```{{python}}\n#| {option}\npass\n```\n"), true);
        assert!(result.preparation.is_none(), "{option}");
        assert_eq!(
            result.parsed.diagnostics[0].code,
            DiagnosticCode::InvalidCellOption
        );
    }
    let result = prepare(
        "```{python}\n#| fig-alt: ''\n#| fig-subcap: [First, Second]\npass\n```\n",
        true,
    );
    assert!(result.parsed.diagnostics.is_empty());
    assert_eq!(
        result.preparation.unwrap().cells[0]
            .options
            .fig_subcap
            .value
            .len(),
        2
    );
}

#[test]
fn nested_cells_keep_source_order_unicode_ranges_and_exact_bytes() {
    let source = "Résumé 🦕\r\n\r\n```{Python3}\r\nx = 1\r\n```\r\n\r\n> ```{r}\r\n> #| eval: false\r\n> x <- 'é'\r\n> ```\r\n\r\n- Item\r\n\r\n  ```{julia}\r\n  x = 3\r\n  ```\r\n\r\n::: {.callout-note}\r\n```{python}\r\nx += 1\r\n```\r\n:::\r\n";
    let result = prepare(source, true);
    assert!(
        result.parsed.diagnostics.is_empty(),
        "{:?}",
        result.parsed.diagnostics
    );
    let cells = result.preparation.unwrap().cells;
    assert_eq!(cells.len(), 4);
    for (ordinal, cell) in cells.iter().enumerate() {
        assert_eq!(cell.ordinal, ordinal);
        assert_eq!(
            cell.cell.source,
            cell.cell
                .source_segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
        for segment in &cell.cell.source_segments {
            assert_eq!(&source[segment.span.start..segment.span.end], segment.text);
        }
        if ordinal > 0 {
            assert!(cells[ordinal - 1].cell.span.end < cell.cell.span.start);
        }
    }
    assert!(!cells[1].options.execution.eval.value);
}

#[test]
fn eligibility_distinguishes_vetoes_defaults_and_collection_authority() {
    for (metadata, cell_option, expected) in [
        ("", "", true),
        ("execute: false", "#| eval: true\n", false),
        ("execute: {eval: false}", "", false),
        ("execute: {eval: false}", "#| eval: true\n", true),
        ("", "#| eval: false\n", false),
    ] {
        let source = format!(
            "---\n{metadata}\n---\n```{{python}}\n{cell_option}raise RuntimeError('must not run')\n```\n"
        );
        for authorized in [false, true] {
            let result = prepare(&source, authorized);
            assert!(
                result.parsed.diagnostics.is_empty(),
                "{source}: {:?}",
                result.parsed.diagnostics
            );
            let prepared = result.preparation.unwrap();
            assert_eq!(prepared.execution_eligible, authorized && expected);
            assert_eq!(prepared.page_veto, metadata == "execute: false");
            assert_eq!(prepared.cells.len(), 1);
        }
    }
    for source in [
        "# Empty\n",
        "```python\nraise RuntimeError('must not run')\n```\n",
    ] {
        let prepared = prepare(source, true).preparation.unwrap();
        assert!(!prepared.execution_eligible);
        assert!(prepared.cells.is_empty());
    }
    let result = prepare(
        "```{python}\n#| echo: false\n#| output: false\n#| include: false\n#| error: true\nraise RuntimeError('must not run')\n```\n",
        false,
    );
    let cell = &result.preparation.unwrap().cells[0];
    assert!(!cell.options.execution.include.value);
    assert!(cell.options.execution.error.value);
    assert!(cell.cell.source.contains("must not run"));
}

#[test]
fn gfm_and_invalid_collection_configuration_cannot_prepare_execution() {
    let mut config = collection(false);
    config.format = AuthoredFormat::Gfm;
    let result =
        prepare_collection_document("```{python}\n#| unsupported: true\npass\n```\n", &config)
            .unwrap();
    assert!(result.preparation.is_none());
    assert!(result.parsed.diagnostics.is_empty());
    config.execution = collection(true).execution;
    assert_eq!(
        prepare_collection_document("", &config),
        Err(ExecutionConfigurationError::GfmExecution)
    );
    let mut config = collection(true);
    config.execution.kernel = None;
    assert_eq!(
        prepare_collection_document("", &config),
        Err(ExecutionConfigurationError::MissingKernel)
    );
}

#[test]
fn acceptance_pages_have_reviewable_preparation_records() {
    for language in ["python", "r"] {
        let source = support::load_fixture(format!("acceptance/{language}/execution/stateful.qmd"));
        let result = prepare(&source, true);
        assert!(result.parsed.diagnostics.is_empty());
        let preparation = result.preparation.unwrap();
        assert!(preparation.execution_eligible);
        assert_eq!(preparation.cells.len(), 5);
        let cells = preparation
            .cells
            .iter()
            .map(|cell| {
                json!({
                    "ordinal": cell.ordinal,
                    "language": cell.cell.language,
                    "span": cell.cell.span,
                    "options": cell.options,
                })
            })
            .collect::<Vec<_>>();
        support::assert_json_golden(
            &json!({
                "defaults": preparation.defaults,
                "page_veto": preparation.page_veto,
                "execution_eligible": preparation.execution_eligible,
                "cells": cells,
            }),
            format!("execution/prepared-{language}.json"),
        );
    }
}

#[test]
fn strings_are_literals_and_boolean_strings_are_not_booleans() {
    for (inline, expected) in [
        ("fig.cap=\"A nice plot\"", "A nice plot"),
        ("fig-cap='true'", "true"),
        ("fig-cap=\"Line\\nbreak\"", "Line\nbreak"),
        ("fig-cap=\"It's literal\"", "It's literal"),
        ("fig-cap=''", ""),
        ("fig-cap=inf", "inf"),
    ] {
        let source = format!("```{{python, {inline}}}\npass\n```\n");
        let result = prepare(&source, true);
        assert!(
            result.parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            result.parsed.diagnostics
        );
        assert_eq!(
            result.preparation.unwrap().cells[0]
                .options
                .fig_cap
                .value
                .as_deref(),
            Some(expected)
        );
    }
    for value in ["true", "null", "12", "1.2", "0xFF", ".inf"] {
        let source = format!("```{{python}}\n#| fig-cap: {value}\npass\n```\n");
        assert!(prepare(&source, true).preparation.is_none(), "{value}");
    }
    for key in ["eval", "echo", "include", "error", "output"] {
        for value in ["'true'", "\"false\""] {
            let source = format!("```{{python, {key}={value}}}\npass\n```\n");
            let result = prepare(&source, true);
            assert!(result.preparation.is_none());
            assert_eq!(
                result.parsed.diagnostics[0].code,
                DiagnosticCode::InvalidCellOption
            );
        }
    }
}

#[test]
fn block_scalar_captions_are_cooked_without_hashpipe_or_container_prefixes() {
    for (header, body, expected) in [
        ("|", "#|   First\n#|   Second\n", "First\nSecond\n"),
        (">-", "#|   First\n#|   Second\n", "First Second"),
        ("|+", "#|   First\n#| \n", "First\n\n"),
        ("|2-", "#|     indented\n", "  indented"),
        (">", "#|   First\n#| \n#|   Second\n", "First\nSecond\n"),
    ] {
        let source =
            format!("```{{python}}\n#| fig-cap: {header}\n{body}#| echo: true\npass\n```\n");
        for source in [
            source.clone(),
            source.lines().map(|line| format!("> {line}\n")).collect(),
        ] {
            let result = prepare(&source, true);
            assert!(
                result.parsed.diagnostics.is_empty(),
                "{source}: {:?}",
                result.parsed.diagnostics
            );
            assert_eq!(
                result.preparation.unwrap().cells[0]
                    .options
                    .fig_cap
                    .value
                    .as_deref(),
                Some(expected),
                "{source}: {:?}",
                result.parsed.document
            );
        }
    }
}

#[test]
fn preparation_neither_reads_missing_inputs_nor_creates_execution_artifacts() {
    let root = tempfile::TempDir::new().unwrap();
    let marker = root.path().join("executed");
    let source = format!(
        "```{{python}}\nopen({:?}, 'w').write('executed')\n```\n",
        marker
    );
    let mut config = collection(true);
    config.path = root.path().join("missing-pages");
    config.execution.declared_environment_inputs = vec!["missing.lock".into()];
    let result = prepare_collection_document(&source, &config).unwrap();
    assert!(result.preparation.unwrap().execution_eligible);
    assert!(result.parsed.diagnostics.is_empty());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn authority_errors_suppress_redundant_selector_type_diagnostics() {
    let source = "---\nexecute: true\njupyter: !custom python3\n---\n";
    let result = prepare(source, false);
    assert!(result.preparation.is_none());
    assert_eq!(result.parsed.diagnostics.len(), 1);
    assert_eq!(
        result.parsed.diagnostics[0].code,
        DiagnosticCode::DocumentExecutionNotAuthorized
    );
    assert_eq!(result.parsed.diagnostics[0].related_spans.len(), 1);
}

#[test]
fn canonical_cell_keys_and_repeated_fence_identifiers_cannot_hide_declarations() {
    let result = prepare(
        "```{python, FIG.ALT='Literal', ECHO=false}\n#| echo: true\npass\n```\n",
        true,
    );
    assert!(result.parsed.diagnostics.is_empty());
    let cell = &result.preparation.unwrap().cells[0];
    assert_eq!(cell.options.fig_alt.value.as_deref(), Some("Literal"));
    assert!(cell.options.execution.echo.value);
    for source in [
        "```{python, ECHO=false, echo=true}\n#| echo: true\npass\n```\n",
        "```{python}\n#| ECHO: false\n#| echo: true\npass\n```\n",
        "```{python #first #second}\npass\n```\n",
        "```{python #same #same}\npass\n```\n",
        "```{r bare-label}\n1\n```\n",
    ] {
        let result = prepare(source, true);
        assert!(
            result.preparation.is_none(),
            "{source}: {:?}",
            result.parsed
        );
        assert!(
            result
                .parsed
                .diagnostics
                .iter()
                .all(|d| d.severity == Severity::Error)
        );
    }
}

#[test]
fn complex_metadata_keys_are_rejected_even_without_an_execution_request() {
    for source in [
        "---\n? [title, execute]\n: true\n---\n",
        "---\n{[title, execute]: true}\n---\n",
    ] {
        let result = prepare(source, false);
        assert!(
            result.preparation.is_none(),
            "{source}: {:?}",
            result.parsed
        );
        assert!(
            result
                .parsed
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
        );
    }
}
