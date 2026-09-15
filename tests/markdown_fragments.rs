mod support;

use diplodocus::diagnostics::{DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity};
use diplodocus::documents::{
    AuthoredFormat, MarkdownFragmentOrigin, parse_authored_document, parse_markdown_fragment,
};
use diplodocus::ir::{
    Block, CellOutput, CellOutputKind, CodeCell, OutputRepresentation, ProvenanceActivity,
    SourceLocation, SourceSpan, StreamName,
};
use serde_json::Value;

fn origin() -> MarkdownFragmentOrigin {
    MarkdownFragmentOrigin {
        collection: "python-execution".into(),
        cell: 2,
        output: 1,
        source: SourceLocation {
            repository: "python".into(),
            path: DiagnosticPath::try_from("execution/generated-markdown.qmd").unwrap(),
            span: Some(SourceSpan {
                start: 170,
                end: 362,
            }),
        },
    }
}

fn only_cell(source: &str) -> CodeCell {
    let parsed = parse_authored_document(source, AuthoredFormat::Qmd);
    let mut cells = parsed.document.blocks.into_iter().filter_map(|block| {
        if let Block::CodeCell(cell) = block {
            Some(cell)
        } else {
            None
        }
    });
    let cell = cells.next().expect("one authored cell");
    assert!(cells.next().is_none());
    cell
}

// These acceptance cells use adjacent JSON-compatible string literals, so the
// output can be recovered without starting Python or evaluating any code.
fn literal_output(cell: &CodeCell) -> String {
    cell.source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('"'))
        .map(|line| serde_json::from_str::<String>(line).expect("fixture string literal"))
        .collect()
}

fn assert_inert_and_spanned(value: &Value, source: &str) {
    match value {
        Value::Object(fields) => {
            assert_ne!(
                fields.get("type").and_then(Value::as_str),
                Some("code-cell")
            );
            assert_ne!(
                fields.get("kind").and_then(Value::as_str),
                Some("sanitized-html")
            );
            for (key, span) in fields {
                if key != "span" && !key.ends_with("_span") {
                    continue;
                }
                let span: SourceSpan = serde_json::from_value(span.clone()).unwrap();
                assert!(span.start <= span.end && span.end <= source.len());
                assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
            }
            if let Some(raw) = fields.get("raw").and_then(Value::as_str) {
                let span: SourceSpan = serde_json::from_value(fields["span"].clone()).unwrap();
                assert_eq!(raw, &source[span.start..span.end]);
            }
            if let Some(segments) = fields.get("source_segments").and_then(Value::as_array) {
                let mut reconstructed = String::new();
                for segment in segments {
                    let span: SourceSpan = serde_json::from_value(segment["span"].clone()).unwrap();
                    let text = segment["text"].as_str().unwrap();
                    assert_eq!(&source[span.start..span.end], text);
                    reconstructed.push_str(text);
                }
                assert_eq!(fields["source"], reconstructed);
            }
            if let Some(attributes) = fields.get("attributes") {
                assert!(attributes["identifier"].is_null());
                assert_eq!(attributes["classes"], serde_json::json!([]));
                assert_eq!(attributes["key_values"], serde_json::json!([]));
            }
            for child in fields.values() {
                assert_inert_and_spanned(child, source);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_inert_and_spanned(child, source);
            }
        }
        _ => {}
    }
}

#[test]
fn acceptance_markdown_output_remains_inert_and_retains_its_producer() {
    let workspace = support::acceptance_workspace();
    let authored = workspace.read("python/execution/generated-markdown.qmd");
    let mut cell = only_cell(&authored);
    let source = literal_output(&cell);
    let mut producing = origin();
    producing.cell = 0;
    producing.output = 0;
    producing.source.span = Some(cell.span);
    let parsed = parse_markdown_fragment(&source, producing.clone());
    assert!(parsed.diagnostics.is_empty());
    assert_inert_and_spanned(
        &serde_json::to_value(&parsed.representation).unwrap(),
        &source,
    );
    let OutputRepresentation::MarkdownBlocks { media_type, blocks } = &parsed.representation else {
        panic!("Markdown representation");
    };
    assert_eq!(media_type, "text/markdown");
    assert!(
        matches!(blocks.as_slice(), [Block::CodeBlock { source, span, .. }]
        if source == "raise RuntimeError('Generated Markdown must stay inert')\n" && span.start == 0)
    );
    assert_eq!(
        parsed.provenance.activity,
        ProvenanceActivity::GeneratedMarkdown {
            collection: producing.collection,
            cell: 0,
            output: 0,
        }
    );
    assert_eq!(
        parsed.provenance.source,
        Some(DiagnosticSource::Repository {
            repository: producing.source.repository,
            path: producing.source.path,
        })
    );
    assert_eq!(parsed.provenance.span, Some(cell.span));
    support::assert_json_golden(&parsed, "fragments/generated-markdown.json");
    let provenance = parsed.provenance.clone();
    let (output, diagnostics) = parsed.into_cell_output(CellOutputKind::Display);
    assert_eq!(output.provenance, vec![provenance]);
    assert!(diagnostics.is_empty());
    cell.outputs.push(output);
    assert_eq!(cell.source, only_cell(&authored).source);
    assert_eq!(cell.span, only_cell(&authored).span);
    assert_eq!(cell.outputs.len(), 1);
}

#[test]
fn nested_fences_and_containers_never_acquire_execution_authority() {
    let fragments = [
        "```{python #injected, eval=true}\n#| eval: true\n#| label: injected\nraise RuntimeError('inert')\n```\n",
        "> ```{r}\n> #| eval: true\n> stop('inert')\n> ```\n",
        "- nested:\n\n  ```{python}\n  #| eval: true\n  print('inert')\n  ```\n",
        "> [!WARNING]\n> - nested:\n>\n>   ```{python}\n>   #| label: injected\n>   print('inert')\n>   ```\n",
        "::: {.callout-note #injected}\n\n```{python}\n#| eval: true\nprint('inert')\n```\n\n:::\n",
    ];
    for source in fragments {
        let parsed = parse_markdown_fragment(source, origin());
        let value = serde_json::to_value(&parsed.representation).unwrap();
        assert_inert_and_spanned(&value, source);
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(serialized.contains("code-block"), "{source}");
        assert!(
            serialized.contains("#|"),
            "hashpipe source remains visible: {source}"
        );
        assert!(!serialized.contains("resolved_options"));
    }
}

fn collect_display_sources<'a>(value: &'a Value, sources: &mut Vec<&'a str>) {
    match value {
        Value::Object(fields) => {
            if fields.get("type").and_then(Value::as_str) == Some("code-block") {
                sources.push(fields["source"].as_str().unwrap());
            }
            for value in fields.values() {
                collect_display_sources(value, sources);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_display_sources(value, sources);
            }
        }
        _ => {}
    }
}

#[test]
fn display_source_retains_hashpipes_and_line_endings_without_container_prefixes() {
    let cases = [
        (
            "> ```{r}\n> #| eval: true\n> stop('inert')\n> ```",
            "#| eval: true\nstop('inert')\n",
        ),
        (
            "- nested:\r\n\r\n  ```{python}\r\n  #| label: caf\u{e9}\r\n  print('inert')\r\n  ```",
            "#| label: caf\u{e9}\r\nprint('inert')\r\n",
        ),
        (
            "> [!WARNING]\n> - nested:\n>\n>   ```{python}\n>   #| label: injected\n>   print('inert')\n>   ```",
            "#| label: injected\nprint('inert')\n",
        ),
        (
            "```{python}\n  #| echo: true\nprint('inert')\n```",
            "  #| echo: true\nprint('inert')\n",
        ),
        (
            "> ```{r}\n>   #| eval: true\n> stop('inert')\n> ```",
            "  #| eval: true\nstop('inert')\n",
        ),
        (
            "- nested:\n\n  ```{python}\n    #| eval: true\n  print('inert')\n  ```",
            "  #| eval: true\nprint('inert')\n",
        ),
        (
            "> ```{python}\n> #| label: [invalid\n> print('inert')\n> ```",
            "#| label: [invalid\nprint('inert')\n",
        ),
        ("```{python}\n#| label: inert\n```", "#| label: inert\n"),
        ("> ```{r}\n> #| eval: true", "#| eval: true"),
        (
            "> ```{r}\n>#| eval: true\n> stop('inert')\n> ```",
            "#| eval: true\nstop('inert')\n",
        ),
        ("  ```{python}\n  #| eval: true\n  ```", "  #| eval: true\n"),
        (
            "> ```{python #caf\u{e9}, echo=true}\n> #| label: caf\u{e9}\n> print('inert')\n> ```",
            "#| label: caf\u{e9}\nprint('inert')\n",
        ),
        (
            "~~~{sql}\r\n--| echo: true\r\nSELECT 1\r\n~~~",
            "--| echo: true\r\nSELECT 1\r\n",
        ),
    ];
    for (source, expected) in cases {
        let parsed = parse_markdown_fragment(source, origin());
        let value = serde_json::to_value(&parsed.representation).unwrap();
        assert_inert_and_spanned(&value, source);
        let mut sources = Vec::new();
        collect_display_sources(&value, &mut sources);
        assert_eq!(sources, [expected], "{source}");
    }
}

#[test]
fn multiple_fragments_keep_the_original_code_languages_and_ranges() {
    let source = "```{python #caf\u{e9}}\n#| eval: true\nprint('inert')\n```\n\n> ```{r}\n> #| echo: true\n> stop('inert')\n> ```\n\n```text\nplain\n```";
    let parsed = parse_markdown_fragment(source, origin());
    assert!(parsed.diagnostics.is_empty());
    let value = serde_json::to_value(&parsed.representation).unwrap();
    assert_inert_and_spanned(&value, source);
    let mut sources = Vec::new();
    collect_display_sources(&value, &mut sources);
    assert_eq!(
        sources,
        [
            "#| eval: true\nprint('inert')\n",
            "#| echo: true\nstop('inert')\n",
            "plain\n"
        ]
    );
    let OutputRepresentation::MarkdownBlocks { blocks, .. } = parsed.representation else {
        panic!()
    };
    assert!(
        matches!(&blocks[0], Block::CodeBlock { language, .. } if language.as_deref() == Some("{python"))
    );
    assert!(
        matches!(&blocks[2], Block::CodeBlock { language, .. } if language.as_deref() == Some("text"))
    );

    let indented =
        parse_authored_document("  ```text\n  #| eval: true\n  ```", AuthoredFormat::Gfm);
    assert!(
        matches!(&indented.document.blocks[0], Block::CodeBlock { source, .. } if source == "  #| eval: true\n")
    );
}

#[test]
fn parser_tool_identity_matches_the_exact_dependency_pin() {
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    let parsed = parse_markdown_fragment("", origin());
    let version = &parsed.provenance.tools["panache-parser"];
    assert_eq!(
        manifest["dependencies"]["panache-parser"].as_str(),
        Some(format!("={version}").as_str())
    );
    assert_eq!(
        parsed.provenance.tools["diplodocus"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        matches!(&parsed.representation, OutputRepresentation::MarkdownBlocks { blocks, .. } if blocks.is_empty())
    );
}

#[test]
fn metadata_and_unsupported_html_remain_visible_with_fragment_diagnostics() {
    let source = "---\nexecute: true\njupyter: python3\nfilters: [evil.lua]\n---\n\n# Generated {#injected}\n\n<script>alert('unsafe')</script>\n\nText <img src=x onerror=alert(1)> and $x$.\n";
    let parsed = parse_markdown_fragment(source, origin());
    let value = serde_json::to_value(&parsed.representation).unwrap();
    assert_inert_and_spanned(&value, source);
    let OutputRepresentation::MarkdownBlocks { blocks, .. } = &parsed.representation else {
        panic!()
    };
    assert!(matches!(&blocks[0], Block::Unsupported { raw, span, .. }
        if raw.starts_with("---\nexecute: true") && &source[span.start..span.end] == raw));
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::Unsupported { raw, .. }
        if raw.contains("<script>")))
    );
    assert!(parsed.diagnostics.len() >= 4);
    for diagnostic in &parsed.diagnostics {
        assert_eq!(diagnostic.code, DiagnosticCode::UnsupportedAuthoredSyntax);
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert!(
            diagnostic.source.is_none(),
            "fragment offsets are not page offsets"
        );
        assert!(diagnostic.span.is_some());
    }
    assert!(parsed.diagnostics.windows(2).all(|pair| pair[0] <= pair[1]));
    support::assert_json_golden(&parsed, "fragments/unsupported.json");
}

#[test]
fn supported_gfm_and_semantic_references_keep_fragment_ranges() {
    let source = "# Caf\u{e9}\n\nA **strong** [`pyfoo::foo.fit`] and [guide](../guide.qmd).\n\n| Name | Value |\n| --- | ---: |\n| x | 1 |\n\n[local][ref]\n\n[ref]: https://example.invalid\n";
    let parsed = parse_markdown_fragment(source, origin());
    assert!(parsed.diagnostics.is_empty());
    let value = serde_json::to_value(&parsed.representation).unwrap();
    assert_inert_and_spanned(&value, source);
    let serialized = serde_json::to_string(&value).unwrap();
    assert!(serialized.contains("semantic-reference"));
    assert!(serialized.contains("pyfoo::foo.fit"));
    assert!(serialized.contains("../guide.qmd"));
    assert!(serialized.contains("https://example.invalid"));
    assert!(serialized.contains("table"));
    support::assert_json_golden(&parsed, "fragments/gfm.json");
}

#[test]
fn malformed_metadata_has_deterministic_local_diagnostics() {
    let source = "---\nexecute: [true\n---\n\n```{python}\n#| eval: [true\nprint('inert')\n```\n";
    let parsed = parse_markdown_fragment(source, origin());
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidEmbeddedYaml)
    );
    assert_inert_and_spanned(
        &serde_json::to_value(&parsed.representation).unwrap(),
        source,
    );
    let (output, diagnostics) = parsed.clone().into_cell_output(CellOutputKind::Stream {
        stream: StreamName::Stdout,
    });
    assert_eq!(diagnostics, parsed.diagnostics);
    assert_eq!(output.provenance, vec![parsed.provenance]);
    assert_eq!(output.representations, vec![parsed.representation]);
}

#[test]
fn ordinary_stdout_with_markdown_syntax_stays_plain_text() {
    let workspace = support::acceptance_workspace();
    let authored = workspace.read("python/execution/markdown-looking-stdout.qmd");
    let text = literal_output(&only_cell(&authored));
    assert!(text.contains("# Not a heading"));
    let output = CellOutput {
        kind: CellOutputKind::Stream {
            stream: StreamName::Stdout,
        },
        representations: vec![OutputRepresentation::PlainText {
            media_type: "text/plain".into(),
            text,
        }],
        provenance: Vec::new(),
    };
    let bytes = serde_json::to_string(&output).unwrap();
    let decoded: CellOutput = serde_json::from_str(&bytes).unwrap();
    assert_eq!(output, decoded);
    assert!(
        matches!(&decoded.representations[..], [OutputRepresentation::PlainText { text, .. }]
        if text.contains("```{python}"))
    );
    assert!(!bytes.contains("markdown-blocks"));
    assert!(!bytes.contains("semantic-reference"));
}

#[test]
fn relocated_inputs_and_repeated_parses_have_identical_portable_output() {
    let first_workspace = support::acceptance_workspace();
    let second_workspace = support::acceptance_workspace();
    assert_ne!(first_workspace.path(), second_workspace.path());
    let parse = |workspace: &support::TestWorkspace| {
        let cell = only_cell(&workspace.read("python/execution/generated-markdown.qmd"));
        let parsed = parse_markdown_fragment(&literal_output(&cell), origin());
        serde_json::to_string_pretty(&parsed).unwrap()
    };
    let first = parse(&first_workspace);
    assert_eq!(first, parse(&first_workspace));
    assert_eq!(first, parse(&second_workspace));
    for path in [
        first_workspace.path(),
        second_workspace.path(),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    ] {
        assert!(!first.contains(path.to_str().unwrap()));
    }
}
