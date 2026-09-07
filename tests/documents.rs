use polydoc::diagnostics::{DiagnosticCode, Severity};
use polydoc::documents::{AuthoredFormat, parse_authored_document};
use polydoc::ir::{Block, CalloutKind, CellOptionResolution, Inline, TableAlignment};

mod support;

#[test]
fn gfm_profile_builds_typed_ir_and_retains_unsupported_html() {
    let source = "# Guide\n\nSee [`pyfoo::foo.fit`], [ordinary], and [site](https://example.com \"Site\").\n\n| A | B |\n|:--|--:|\n| 1 | 2 |\n\n<div>unsafe</div>\n";
    let parsed = parse_authored_document(source, AuthoredFormat::Gfm);

    assert!(matches!(
        parsed.document.blocks[0],
        Block::Heading { level: 1, .. }
    ));
    let paragraph = parsed
        .document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { inlines, .. } => Some(inlines),
            _ => None,
        })
        .expect("paragraph");
    assert!(paragraph.iter().any(|inline| matches!(
        inline,
        Inline::SemanticReference { target, .. } if target == "pyfoo::foo.fit"
    )));
    assert!(paragraph.iter().any(|inline| matches!(
        inline,
        Inline::Text { value, .. } if value == "[ordinary]"
    )));
    assert!(parsed.document.blocks.iter().any(|block| matches!(
        block,
        Block::Table { alignments, .. }
            if alignments == &[TableAlignment::Left, TableAlignment::Right]
    )));
    assert!(parsed.document.blocks.iter().any(|block| matches!(
        block,
        Block::Unsupported { raw, .. } if raw == "<div>unsafe</div>\n"
    )));
    assert!(parsed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedAuthoredSyntax
            && diagnostic.severity == Severity::Warning
    }));
    assert_eq!(parsed.diagnostics.len(), 1);
}

#[test]
fn qmd_profile_extracts_frontmatter_cells_and_callouts() {
    let source = "---\ntitle: Example\n---\n\n::: {.callout-note}\nRemember this.\n:::\n\n```{python #setup, echo=true}\n#| echo: false\nprint('ok')\n```\n";
    let parsed = parse_authored_document(source, AuthoredFormat::Qmd);

    assert!(parsed.document.frontmatter.is_some());
    assert!(parsed.document.blocks.iter().any(|block| matches!(
        block,
        Block::Callout {
            kind: CalloutKind::Note,
            ..
        }
    )));
    let cell = parsed
        .document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::CodeCell(cell) => Some(cell),
            _ => None,
        })
        .expect("code cell");
    assert_eq!(cell.language.as_deref(), Some("python"));
    assert_eq!(
        cell.identifier.as_ref().map(|id| id.value.as_str()),
        Some("setup")
    );
    assert_eq!(cell.source, "print('ok')\n");
    let echo = cell
        .resolved_options
        .iter()
        .find(|option| option.key == "echo")
        .expect("echo resolution");
    assert!(matches!(
        echo.resolution,
        CellOptionResolution::Resolved { .. }
    ));
}

#[test]
fn malformed_yaml_and_ambiguous_options_produce_stable_diagnostics() {
    let source = "---\ntitle: [broken\n---\n\n```{python, echo=true, echo=false}\npass\n```\n";
    let parsed = parse_authored_document(source, AuthoredFormat::Qmd);

    assert!(parsed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidEmbeddedYaml
            && diagnostic.severity == Severity::Error
    }));
    assert!(parsed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::AmbiguousCellOption
            && diagnostic.severity == Severity::Warning
    }));
    assert!(
        parsed
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeCell(_)))
    );
}

#[test]
fn serialized_document_ir_is_deterministic() {
    let workspace = support::acceptance_workspace();
    let source = workspace.read("python/docs/guide.qmd");
    let parsed = parse_authored_document(&source, AuthoredFormat::Qmd);
    let first = serde_json::to_string_pretty(&parsed).expect("serialize document parse");
    let second = serde_json::to_string_pretty(&parsed).expect("serialize document parse again");
    assert_eq!(first, second);
    assert!(!first.contains(env!("CARGO_MANIFEST_DIR")));
    support::assert_matches_golden(first, "documents/qmd.json");
}

#[test]
fn acceptance_authored_pages_parse_through_the_production_adapter() {
    let workspace = support::acceptance_workspace();
    let gfm = workspace.read("core/docs/index.md");
    let nested_gfm = workspace.read("core/docs/getting-started/workspace.md");
    let qmd = workspace.read("python/docs/guide.qmd");
    let nested_qmd = workspace.read("python/docs/models/fitting.qmd");
    let parsed_gfm = parse_authored_document(&gfm, AuthoredFormat::Gfm);
    let parsed_nested_gfm = parse_authored_document(&nested_gfm, AuthoredFormat::Gfm);
    let parsed_qmd = parse_authored_document(&qmd, AuthoredFormat::Qmd);
    let parsed_nested_qmd = parse_authored_document(&nested_qmd, AuthoredFormat::Qmd);

    assert!(parsed_gfm.document.blocks.iter().any(|block| matches!(
        block,
        Block::Callout {
            kind: CalloutKind::Note,
            ..
        }
    )));
    assert!(parsed_gfm.document.blocks.iter().any(|block| matches!(
        block,
        Block::CodeBlock { language, .. } if language.as_deref() == Some("python")
    )));
    assert!(
        parsed_gfm
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Table { .. }))
    );
    assert!(parsed_gfm.document.blocks.iter().any(|block| {
        matches!(
            block,
            Block::Paragraph { inlines, .. }
                if inlines.iter().any(|inline| matches!(
                    inline,
                    Inline::SemanticReference { target, .. }
                        if target == "pyfoo::foo.fit"
                ))
        )
    }));
    assert!(
        !parsed_gfm
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeCell(_)))
    );
    assert!(
        parsed_qmd
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeCell(_)))
    );
    assert!(parsed_qmd.document.blocks.iter().any(|block| matches!(
        block,
        Block::Unsupported { source_kind, .. } if source_kind == "FENCED_DIV"
    )));
    assert!(parsed_nested_gfm.document.blocks.iter().any(|block| {
        matches!(
            block,
            Block::Paragraph { inlines, .. }
                if inlines.iter().any(|inline| matches!(
                    inline,
                    Inline::Image { target, .. } if target == "../assets/workspace.svg"
                ))
        )
    }));
    assert!(parsed_nested_qmd.document.blocks.iter().any(|block| {
        matches!(
            block,
            Block::Paragraph { inlines, .. }
                if inlines.iter().any(|inline| matches!(
                    inline,
                    Inline::SemanticReference { target, .. }
                        if target == "foo.FooModel.fit"
                ))
        )
    }));
    assert!(
        parsed_nested_qmd
            .document
            .blocks
            .iter()
            .any(|block| matches!(
                block,
                Block::CodeBlock { language, .. } if language.as_deref() == Some("python")
            ))
    );
}

#[test]
fn acceptance_execution_pages_retain_ordered_cells_labels_and_options() {
    let workspace = support::acceptance_workspace();
    let cases = [
        (
            "python/execution/stateful.qmd",
            "python",
            [
                "python-setup",
                "python-streams",
                "python-markdown",
                "python-figure",
                "python-error",
            ],
        ),
        (
            "r/execution/stateful.qmd",
            "r",
            ["r-setup", "r-streams", "r-markdown", "r-figure", "r-error"],
        ),
    ];

    for (source, language, labels) in cases {
        let source = workspace.read(source);
        let parsed = parse_authored_document(&source, AuthoredFormat::Qmd);
        let cells = parsed
            .document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::CodeCell(cell) => Some(cell),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(cells.len(), 5);
        assert!(
            cells
                .iter()
                .all(|cell| cell.language.as_deref() == Some(language))
        );
        assert_eq!(
            cells
                .iter()
                .map(|cell| cell.labels[0].value.as_str())
                .collect::<Vec<_>>(),
            labels
        );
        assert!(cells[1].source.contains("total"));
        assert!(cells[2].source.contains("total"));
        assert!(cells[3].source.contains("total"));
        assert!(cells[4].resolved_options.iter().any(|option| {
            option.key == "error"
                && matches!(option.resolution, CellOptionResolution::Resolved { .. })
        }));
    }
}

#[test]
fn acceptance_execution_authority_variants_have_the_expected_authored_cells() {
    let workspace = support::acceptance_workspace();
    let gfm = workspace.read("core/docs/execution/display-only.md");
    let parsed_gfm = parse_authored_document(&gfm, AuthoredFormat::Gfm);
    assert!(
        parsed_gfm
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeBlock { source, .. }
                if source.contains("GFM fences must stay display-only")))
    );
    assert!(
        !parsed_gfm
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::CodeCell(_)))
    );

    for source in [
        "python/safety/default-never.qmd",
        "python/safety/metadata-cannot-authorize.qmd",
    ] {
        let source = workspace.read(source);
        let parsed = parse_authored_document(&source, AuthoredFormat::Qmd);
        assert_eq!(
            parsed
                .document
                .blocks
                .iter()
                .filter(|block| matches!(block, Block::CodeCell(_)))
                .count(),
            1
        );
    }

    let generated = workspace.read("python/execution/generated-markdown.qmd");
    let parsed_generated = parse_authored_document(&generated, AuthoredFormat::Qmd);
    let generated_cells = parsed_generated
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::CodeCell(cell) => Some(cell),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(generated_cells.len(), 1);
    assert!(
        generated_cells[0]
            .source
            .contains("Generated Markdown must stay inert")
    );
}

#[test]
fn acceptance_output_safety_variants_are_single_cell_documents() {
    let workspace = support::acceptance_workspace();
    let cases = [
        (
            "python/execution/markdown-looking-stdout.qmd",
            "markdown-looking-stdout",
            "# Not a heading",
        ),
        (
            "python/execution/unsafe-html.qmd",
            "unsafe-html",
            "<script>",
        ),
        (
            "python/execution/asset-boundary-escape.qmd",
            "asset-boundary-escape",
            "../../core/docs/assets/workspace.svg",
        ),
    ];

    for (source, label, construct) in cases {
        let source = workspace.read(source);
        let parsed = parse_authored_document(&source, AuthoredFormat::Qmd);
        let cells = parsed
            .document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::CodeCell(cell) => Some(cell),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].language.as_deref(), Some("python"));
        assert_eq!(cells[0].labels[0].value, label);
        assert!(cells[0].source.contains(construct));
    }
}
