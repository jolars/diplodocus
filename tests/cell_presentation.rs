use diplodocus::configuration::{ContentConfiguration, ExecutionMode};
use diplodocus::diagnostics::{
    DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};
use diplodocus::documents::{
    AuthoredFormat, MarkdownFragmentOrigin, parse_markdown_fragment, prepare_collection_document,
};
use diplodocus::execution::{
    CellExecutionResult, CellOutcome, CellSkipReason, ExecutionFailureKind, ExecutionOutput,
    ExecutionPage, OptionOrigin, PreparedCell, validate_figure_options,
};
use diplodocus::ir::{
    AssetReference, CellOutput, CellOutputKind, OutputRepresentation, SourceLocation, StreamName,
};
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::rendering::{present_cell, present_prepared_cell};

fn prepared(options: &str, mode: ExecutionMode) -> (ExecutionPage, PreparedCell) {
    let execution = if mode == ExecutionMode::Execute {
        "[execution]\nmode = 'execute'\nengine = 'jupyter'\nkernel = 'unavailable'"
    } else {
        ""
    };
    let collection: ContentConfiguration = toml::from_str(&format!(
        "id = 'guide'\nowner = 'project'\nrepository = 'docs'\npath = 'guide'\nmount = 'guide'\nformat = 'qmd'\n{execution}"
    )).unwrap();
    let source = format!(
        "---\nexecute: {{echo: false}}\n---\n\n> ```{{python, echo=true}}\n{options}> print('<source>')\n> ```\n"
    );
    let prepared = prepare_collection_document(&source, &collection).unwrap();
    assert!(
        prepared.parsed.diagnostics.is_empty(),
        "{:?}",
        prepared.parsed.diagnostics
    );
    let cell = prepared.preparation.unwrap().cells.remove(0);
    let page = ExecutionPage {
        source: SourceLocation {
            repository: "docs".into(),
            path: DiagnosticPath::try_from("guide/example.qmd").unwrap(),
            span: None,
        },
        collection: "guide".into(),
        working_directory: Some(DiagnosticPath::try_from("guide").unwrap()),
        source_fingerprint: fingerprint_bytes(source.as_bytes()),
        format: AuthoredFormat::Qmd,
        mode,
        page_veto: false,
        parser_version: "0.29.2".into(),
        qmd_policy: "qmd-mvp-v1".into(),
    };
    (page, cell)
}

fn completed(prepared: &PreparedCell, outputs: Vec<ExecutionOutput>) -> CellExecutionResult {
    CellExecutionResult {
        ordinal: prepared.ordinal,
        language: prepared.cell.language.clone(),
        span: prepared.cell.span,
        source_segments: prepared.cell.source_segments.clone(),
        submitted_source_fingerprint: Some(fingerprint_bytes(prepared.cell.source.as_bytes())),
        options: prepared.options.clone(),
        outcome: CellOutcome::Ok,
        outputs,
    }
}

fn output(representations: Vec<OutputRepresentation>, selected: Option<&str>) -> ExecutionOutput {
    ExecutionOutput {
        owning_cell: 0,
        producing_cell: 0,
        updating_cell: None,
        slot: 0,
        output: CellOutput {
            kind: CellOutputKind::Display,
            representations,
            provenance: Vec::new(),
        },
        offered_mime_types: selected.into_iter().map(str::to_owned).collect(),
        selected_mime_type: selected.map(str::to_owned),
        representations: Vec::new(),
        diagnostic_indices: Vec::new(),
    }
}

fn text() -> OutputRepresentation {
    OutputRepresentation::PlainText {
        media_type: "text/plain".into(),
        text: "<output>\n".into(),
    }
}

fn asset(media: &str) -> OutputRepresentation {
    OutputRepresentation::Asset {
        media_type: media.into(),
        asset: AssetReference {
            path: DiagnosticPath::try_from("execution/figure").unwrap(),
            fingerprint: fingerprint_bytes(b"fixture asset"),
        },
    }
}

#[test]
fn presentation_applies_prepared_options_without_changing_source_or_output() {
    for include in [false, true] {
        for echo in [false, true] {
            for visibility in ["false", "true", "asis"] {
                let options = format!(
                    "> #| include: {include}\n> #| echo: {echo}\n> #| output: {visibility}\n"
                );
                let (page, prepared) = prepared(&options, ExecutionMode::Execute);
                let mut stream = output(vec![text()], Some("text/plain"));
                stream.output.kind = CellOutputKind::Stream {
                    stream: StreamName::Stderr,
                };
                let error = output(Vec::new(), None);
                let mut cell = completed(&prepared, vec![stream, error]);
                cell.outputs[1].output.kind = CellOutputKind::Error {
                    name: "ExampleError".into(),
                    message: "<error>".into(),
                    traceback: Vec::new(),
                };
                cell.outcome = CellOutcome::AllowedError;
                cell.options.execution.error.value = true;
                let before = cell.clone();
                let presentation = present_cell(&page, &cell).unwrap();
                assert_eq!(presentation.source.is_some(), include && echo);
                if let Some(source) = presentation.source {
                    assert_eq!(
                        source.iter().map(|s| s.text.as_str()).collect::<String>(),
                        prepared.cell.source
                    );
                }
                assert_eq!(
                    presentation.outputs.len(),
                    if include && visibility != "false" {
                        2
                    } else {
                        0
                    }
                );
                assert_eq!(cell, before);
                let unexecuted = present_prepared_cell(page.mode, &prepared);
                assert_eq!(unexecuted.source.is_some(), include && echo);
                assert!(unexecuted.outputs.is_empty());
            }
        }
    }
}

#[test]
fn never_mode_preserves_source_and_has_no_execution_output() {
    let (page, prepared) = prepared(
        "> #| echo: false\n> #| include: false\n> #| output: asis\n> #| fig-subcap: [Unused]\n",
        ExecutionMode::Never,
    );
    let before = prepared.clone();
    let view = present_prepared_cell(page.mode, &prepared);
    assert!(view.source.is_some());
    assert!(view.outputs.is_empty());
    let mut cell = completed(&prepared, Vec::new());
    cell.outcome = CellOutcome::Skipped {
        reason: CellSkipReason::EvalFalse,
    };
    cell.submitted_source_fingerprint = None;
    assert_eq!(present_cell(&page, &cell).unwrap(), view);
    assert_eq!(prepared, before);
}

#[test]
fn asis_presents_converted_markdown_and_preserves_other_output_semantics() {
    let (page, prepared) = prepared("> #| output: asis\n", ExecutionMode::Execute);
    let fragment = parse_markdown_fragment(
        "**Result**\n\n```{python}\nprint('display only')\n```\n",
        MarkdownFragmentOrigin {
            collection: page.collection.clone(),
            cell: 0,
            output: 0,
            source: SourceLocation {
                span: Some(prepared.cell.span),
                ..page.source.clone()
            },
        },
    );
    let mut stdout = output(vec![fragment.representation], Some("text/markdown"));
    stdout.output.kind = CellOutputKind::Stream {
        stream: StreamName::Stdout,
    };
    let mut stderr = output(vec![text()], Some("text/plain"));
    stderr.output.kind = CellOutputKind::Stream {
        stream: StreamName::Stderr,
    };
    stderr.slot = 1;
    let cell = completed(&prepared, vec![stdout, stderr]);
    let view = present_cell(&page, &cell).unwrap();
    assert_eq!(view.outputs, cell.outputs);
    assert!(matches!(
        view.outputs[0].output.representations[0],
        OutputRepresentation::MarkdownBlocks { .. }
    ));
    assert!(matches!(
        view.outputs[1].output.representations[0],
        OutputRepresentation::PlainText { .. }
    ));
}

#[test]
fn skipped_and_vetoed_cells_still_apply_presentation_without_counting_figures() {
    let (mut page, prepared) = prepared(
        "> #| eval: false\n> #| echo: false\n> #| fig-subcap: [Unused]\n",
        ExecutionMode::Execute,
    );
    for veto in [false, true] {
        page.page_veto = veto;
        for reason in [CellSkipReason::EvalFalse, CellSkipReason::LanguageMismatch] {
            let mut cell = completed(&prepared, Vec::new());
            cell.outcome = CellOutcome::Skipped { reason };
            cell.submitted_source_fingerprint = None;
            assert!(validate_figure_options(&page, std::slice::from_ref(&cell)).is_ok());
            assert!(present_cell(&page, &cell).unwrap().source.is_none());
        }
        assert!(present_prepared_cell(page.mode, &prepared).source.is_none());
    }
}

#[test]
fn subcaptions_count_final_selected_assets_in_slot_order() {
    let (page, prepared) = prepared("> #| fig-subcap: [First, Second]\n", ExecutionMode::Execute);
    let mut first = output(
        vec![asset("image/svg+xml"), asset("image/png"), text()],
        Some("image/svg+xml"),
    );
    first.slot = 2;
    first.updating_cell = Some(3);
    let mut second = output(vec![asset("image/jpeg"), text()], Some("image/jpeg"));
    second.slot = 5;
    let cell = completed(
        &prepared,
        vec![first, output(vec![text()], Some("text/plain")), second],
    );
    assert!(validate_figure_options(&page, std::slice::from_ref(&cell)).is_ok());
    assert_eq!(present_cell(&page, &cell).unwrap().outputs.len(), 3);
    // Clearing and later updates change the final count, even if a slot retains
    // its original identity or advertises an image that was not selected.
    let mut cleared = cell.clone();
    cleared.outputs.remove(0);
    assert!(validate_figure_options(&page, &[cleared]).is_err());
    let mut updated = cell;
    updated.outputs[0] = output(vec![text(), asset("image/png")], Some("text/plain"));
    assert!(validate_figure_options(&page, &[updated]).is_err());
}

#[test]
fn repeated_assets_count_as_separate_figures_and_empty_captions_are_optional() {
    let (page, prepared) = prepared("> #| fig-subcap: [First, Second]\n", ExecutionMode::Execute);
    let figure = output(vec![asset("image/png")], Some("image/png"));
    let mut second = figure.clone();
    second.slot = 1;
    let mut cell = completed(&prepared, vec![figure, second]);
    assert!(validate_figure_options(&page, std::slice::from_ref(&cell)).is_ok());
    cell.options.fig_subcap.value.clear();
    assert!(validate_figure_options(&page, &[cell]).is_ok());
}

#[test]
fn page_validation_reports_every_mismatch_and_falls_back_to_the_cell_span() {
    let (page, prepared) = prepared("> #| fig-subcap: [Missing]\n", ExecutionMode::Execute);
    let mut first = completed(&prepared, Vec::new());
    first.options.fig_subcap.origin = OptionOrigin::Default;
    let mut second = first.clone();
    second.ordinal = 1;
    second.span.start += first.span.end;
    second.span.end += first.span.end;
    let failure = validate_figure_options(&page, &[first.clone(), second.clone()]).unwrap_err();
    assert_eq!(failure.diagnostics.len(), 2);
    assert_eq!(failure.diagnostics[0].span, Some(first.span));
    assert_eq!(failure.diagnostics[1].span, Some(second.span));
    assert!(
        failure
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.related_spans.is_empty())
    );
}

#[test]
fn hidden_and_empty_outputs_fail_with_source_attributed_figure_diagnostics() {
    for behavior in [
        "",
        "> #| output: false\n",
        "> #| include: false\n",
        "> #| error: true\n",
    ] {
        let (page, prepared) = prepared(
            &format!("{behavior}> #| fig-subcap: [Missing]\n"),
            ExecutionMode::Execute,
        );
        let OptionOrigin::Hashpipe { span } = prepared.options.fig_subcap.origin else {
            panic!("hashpipe origin")
        };
        let mut cell = completed(&prepared, Vec::new());
        if cell.options.execution.error.value {
            cell.outcome = CellOutcome::AllowedError;
        }
        let before = cell.clone();
        let failure = present_cell(&page, &cell).unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::OutputValidation);
        assert!(failure.cleanup_diagnostics.is_empty());
        assert_eq!(failure.diagnostics.len(), 1);
        let diagnostic = &failure.diagnostics[0];
        assert_eq!(diagnostic.code, DiagnosticCode::InvalidFigureOptions);
        assert_eq!(diagnostic.code.as_str(), "invalid-figure-options");
        assert_eq!(
            serde_json::to_value(diagnostic.code).unwrap(),
            "invalid-figure-options"
        );
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.span, Some(span));
        assert_eq!(diagnostic.related_spans, [cell.span]);
        assert_eq!(
            diagnostic.source,
            Some(DiagnosticSource::Repository {
                repository: "docs".into(),
                path: page.source.path.clone()
            })
        );
        assert_eq!(
            diagnostic.related_entity,
            Some(DiagnosticEntity::Content { id: "guide".into() })
        );
        assert!(diagnostic.message.contains("1 subcaption"));
        assert!(diagnostic.message.contains("0 selected figure"));
        assert_eq!(cell, before);
    }
}
