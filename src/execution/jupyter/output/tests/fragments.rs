use super::*;
use crate::documents::{MarkdownFragmentOrigin, parse_markdown_fragment};
use crate::ir::{Block, ProvenanceActivity};

fn asis_cell(ordinal: usize) -> PreparedCell {
    let mut prepared = cell(ordinal);
    prepared.options.execution.output.value = OutputVisibility::AsIs;
    prepared
}

fn accept_asis(reducer: &mut OutputReducer, ordinal: usize, events: Vec<CellEvent>) {
    reducer
        .accept_cell(
            &asis_cell(ordinal),
            CellOutcome::Ok,
            events,
            &mut validate_text,
        )
        .unwrap();
}

fn markdown(output: &ExecutionOutput) -> &[Block] {
    assert_eq!(output.selected_mime_type.as_deref(), Some("text/markdown"));
    match &output.output.representations[0] {
        OutputRepresentation::MarkdownBlocks { blocks, .. } => blocks,
        other => panic!("expected Markdown, got {other:?}"),
    }
}

#[test]
fn asis_joins_only_adjacent_stdout_and_keeps_other_output_literal() {
    let mut reducer = new_reducer();
    accept_asis(
        &mut reducer,
        0,
        vec![
            stream(StreamName::Stdout, "# Ca"),
            stream(StreamName::Stdout, "fé\n"),
            stream(StreamName::Stderr, "<script>stderr</script>\n"),
            stream(StreamName::Stdout, "**bo"),
            stream(StreamName::Stdout, "ld**\n"),
            display(None, "# literal display"),
        ],
    );
    let result = reducer.finish().unwrap();
    let outputs = &result.cells[0].outputs;
    assert_eq!(
        outputs.iter().map(|o| o.slot).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(matches!(markdown(&outputs[0]), [Block::Heading { .. }]));
    assert_eq!(
        outputs[0].representations[0].content_fingerprint,
        Fingerprint {
            algorithm: "sha256".into(),
            value: "848d972fb1a616e377e24946399cd14bc67afaacce0b164bfb2e1b7edc25c373".into(),
        }
    );
    assert_eq!(text(&outputs[1]), "<script>stderr</script>\n");
    assert!(matches!(markdown(&outputs[2]), [Block::Paragraph { .. }]));
    assert_eq!(text(&outputs[3]), "# literal display");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn ignored_messages_preserve_asis_chunks_and_warning_order() {
    let mut reducer = new_reducer();
    accept_asis(
        &mut reducer,
        0,
        vec![
            stream(StreamName::Stdout, "<div>"),
            ignored_message(0),
            stream(StreamName::Stdout, "inert</div>\n"),
            ignored_message(0),
            stream(StreamName::Stderr, "literal"),
        ],
    );
    let result = reducer.finish().unwrap();
    let outputs = &result.cells[0].outputs;
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].diagnostic_indices, [0]);
    assert_eq!(text(&outputs[1]), "literal");
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::UnsupportedAuthoredSyntax,
            DiagnosticCode::UnsupportedKernelMessage,
            DiagnosticCode::UnsupportedKernelMessage,
        ]
    );
}

#[test]
fn every_non_stdout_event_breaks_an_asis_run() {
    for separator in [
        stream(StreamName::Stderr, "warning"),
        display(None, "display"),
        CellEvent::Result {
            bundle: bundle(json!({"text/plain": "result"})),
            display_id: None,
        },
        CellEvent::Error {
            name: "Error".into(),
            message: "message".into(),
            traceback: vec![],
        },
        update(None, "update"),
        CellEvent::Clear { wait: true },
        CellEvent::Clear { wait: false },
    ] {
        let clears = matches!(separator, CellEvent::Clear { .. });
        let mut reducer = new_reducer();
        accept_asis(
            &mut reducer,
            0,
            vec![
                stream(StreamName::Stdout, "```text\n"),
                separator,
                stream(StreamName::Stdout, "# Separate\n"),
            ],
        );
        let result = reducer.finish().unwrap();
        let outputs = &result.cells[0].outputs;
        let last = outputs.last().unwrap();
        assert!(last.slot > 0);
        assert!(matches!(markdown(last), [Block::Heading { .. }]));
        if clears {
            assert_eq!(outputs.len(), 1);
        } else {
            assert!(matches!(markdown(&outputs[0]), [Block::CodeBlock { .. }]));
        }
    }
}

#[test]
fn asis_runs_never_cross_cells_and_deferred_clear_without_output_keeps_the_run() {
    let mut reducer = new_reducer();
    accept_asis(
        &mut reducer,
        0,
        vec![
            stream(StreamName::Stdout, "```text\n"),
            CellEvent::Clear { wait: true },
        ],
    );
    accept_asis(
        &mut reducer,
        1,
        vec![stream(StreamName::Stdout, "# Separate\n")],
    );
    let result = reducer.finish().unwrap();
    assert!(matches!(
        markdown(&result.cells[0].outputs[0]),
        [Block::CodeBlock { .. }]
    ));
    assert!(matches!(
        markdown(&result.cells[1].outputs[0]),
        [Block::Heading { .. }]
    ));
    assert_eq!(result.cells[1].outputs[0].slot, 0);
}

#[test]
fn markdown_mime_parses_strings_and_arrays_and_retains_plain_fallbacks() {
    for payload in [json!("# Café\n"), json!(["# Ca", "fé\n"])] {
        for is_result in [false, true] {
            let mut reducer = new_reducer();
            let bundle = bundle(json!({"text/plain": "literal", "text/markdown": payload}));
            let event = if is_result {
                CellEvent::Result {
                    bundle,
                    display_id: None,
                }
            } else {
                CellEvent::Display {
                    bundle,
                    display_id: None,
                }
            };
            accept(&mut reducer, 0, vec![event]);
            let result = reducer.finish().unwrap();
            let output = &result.cells[0].outputs[0];
            assert!(matches!(markdown(output), [Block::Heading { .. }]));
            assert!(
                matches!(&output.output.representations[1], OutputRepresentation::PlainText { text, .. } if text == "literal")
            );
            assert_eq!(output.output.provenance.len(), 1);
            assert_eq!(output.representations.len(), 2);
            assert!(result.diagnostics.is_empty());
        }
    }
}

#[test]
fn malformed_markdown_payloads_warn_and_fall_back_without_coercion() {
    for payload in [
        json!(null),
        json!(42),
        json!({"text": "bad"}),
        json!(["ok", false]),
    ] {
        for fallback in [false, true] {
            let mut data = json!({"text/markdown": payload});
            if fallback {
                data["text/plain"] = json!("safe");
            }
            let mut reducer = new_reducer();
            accept(
                &mut reducer,
                0,
                vec![CellEvent::Display {
                    bundle: bundle(data),
                    display_id: None,
                }],
            );
            let result = reducer.finish().unwrap();
            assert_eq!(result.diagnostics.len(), 1);
            assert_eq!(
                result.diagnostics[0].code,
                DiagnosticCode::InvalidCellOutput
            );
            let output = &result.cells[0].outputs[0];
            assert_eq!(output.diagnostic_indices, [0]);
            if fallback {
                assert_eq!(text(output), "safe");
            } else {
                assert!(output.unsupported_placeholder().is_some());
            }
        }
    }
}

#[test]
fn generated_fragments_are_isolated_inert_and_attributed_even_when_hidden() {
    let fragment = "---\nexecute: true\njupyter: python3\n---\n\n# Generated {#injected}\n\n> ```{python}\n> #| eval: true\n> raise RuntimeError('must not execute')\n> ```\n\n<script>unsafe()</script>\n\n[`pkg::fit`] and [guide](../guide.qmd).\n";
    for asis in [false, true] {
        let mut prepared = cell(0);
        prepared.options.execution.include.value = false;
        prepared.options.execution.output.value = if asis {
            OutputVisibility::AsIs
        } else {
            OutputVisibility::Hide
        };
        let mut source = page().source;
        source.span = Some(prepared.cell.span);
        let expected = parse_markdown_fragment(
            fragment,
            MarkdownFragmentOrigin {
                collection: page().collection,
                cell: 0,
                output: 1,
                source,
            },
        );
        let event = if asis {
            stream(StreamName::Stdout, fragment)
        } else {
            CellEvent::Display {
                bundle: bundle(json!({"text/markdown": fragment})),
                display_id: None,
            }
        };
        let mut reducer = new_reducer();
        reducer
            .accept_cell(
                &prepared,
                CellOutcome::Ok,
                vec![stream(StreamName::Stderr, "first"), event],
                &mut validate_text,
            )
            .unwrap();
        let result = reducer.finish().unwrap();
        let output = &result.cells[0].outputs[1];
        assert_eq!(output.output.representations[0], expected.representation);
        assert_eq!(output.output.provenance, [expected.provenance]);
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.span))
                .collect::<Vec<_>>(),
            expected
                .diagnostics
                .iter()
                .map(|d| (d.code, d.span))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result
                .execution_diagnostics
                .iter()
                .map(|d| match d {
                    ExecutionDiagnostic::FragmentUnsupported {
                        attribution,
                        source_kind,
                    } => {
                        assert_eq!(attribution.slot, Some(1));
                        assert_eq!(attribution.fragment.unwrap().ordinal, 0);
                        assert_eq!(attribution.fragment.unwrap().byte_length, fragment.len());
                        source_kind.as_str()
                    }
                    other => panic!("Expected a typed fragment warning, got {other:?}"),
                })
                .collect::<Vec<_>>(),
            ["YAML_METADATA", "HTML_BLOCK"]
        );
        assert!(!result.diagnostics.is_empty());
        assert_eq!(
            output.diagnostic_indices,
            (0..result.diagnostics.len()).collect::<Vec<_>>()
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.source.is_none() && d.span.is_some())
        );
        let encoded = serde_json::to_string(&output.output.representations).unwrap();
        assert!(!encoded.contains("\"code-cell\""));
        assert!(!encoded.contains("\"sanitized-html\""));
        assert!(encoded.contains("semantic-reference"));
        assert!(
            crate::rendering::present_cell(&page(), &result.cells[0])
                .unwrap()
                .outputs
                .is_empty()
        );
    }
}

#[test]
fn markdown_updates_keep_the_current_fragment_producer_and_clear_preserves_warnings() {
    let mut reducer = new_reducer();
    accept(&mut reducer, 0, vec![display(Some("id"), "old")]);
    accept(
        &mut reducer,
        1,
        vec![CellEvent::UpdateDisplay {
            bundle: bundle(json!({"text/markdown": "# Replacement\n"})),
            display_id: Some("id".into()),
        }],
    );
    accept_asis(
        &mut reducer,
        2,
        vec![
            stream(StreamName::Stdout, "<script>bad</script>\n"),
            CellEvent::Clear { wait: false },
        ],
    );
    let result = reducer.finish().unwrap();
    let output = &result.cells[0].outputs[0];
    assert!(matches!(markdown(output), [Block::Heading { .. }]));
    assert_eq!(
        (
            output.owning_cell,
            output.producing_cell,
            output.updating_cell,
            output.slot
        ),
        (0, 0, Some(1), 0)
    );
    assert_eq!(output.representations[0].producing_cell, 1);
    assert_eq!(
        output.output.provenance[0].activity,
        ProvenanceActivity::GeneratedMarkdown {
            collection: "guide".into(),
            cell: 1,
            output: 0
        }
    );
    assert!(result.cells[2].outputs.is_empty());
    assert!(!result.diagnostics.is_empty());
}

#[test]
fn empty_markdown_is_an_accepted_empty_fragment() {
    for payload in [json!(""), json!([])] {
        let mut reducer = new_reducer();
        accept(
            &mut reducer,
            0,
            vec![CellEvent::Display {
                bundle: bundle(json!({"text/markdown": payload})),
                display_id: None,
            }],
        );
        let result = reducer.finish().unwrap();
        assert!(markdown(&result.cells[0].outputs[0]).is_empty());
        assert!(result.diagnostics.is_empty());
    }
}

#[test]
fn ordinary_streams_remain_literal_preformatted_text_including_when_hidden() {
    for visibility in [
        OutputVisibility::Show,
        OutputVisibility::Hide,
        OutputVisibility::AsIs,
    ] {
        let mut prepared = cell(0);
        prepared.options.execution.output.value = visibility;
        let mut reducer = new_reducer();
        let literal = "# Heading\n<script>&unsafe</script>\n```{python}\nrun()\n```\n";
        reducer
            .accept_cell(
                &prepared,
                CellOutcome::Ok,
                vec![
                    stream(StreamName::Stderr, literal),
                    stream(StreamName::Stderr, literal),
                ],
                &mut validate_text,
            )
            .unwrap();
        let result = reducer.finish().unwrap();
        assert_eq!(result.cells[0].outputs.len(), 2);
        for output in &result.cells[0].outputs {
            assert_eq!(text(output), literal);
            assert!(output.output.provenance.is_empty());
        }
        if visibility != OutputVisibility::AsIs {
            let mut reducer = new_reducer();
            reducer
                .accept_cell(
                    &prepared,
                    CellOutcome::Ok,
                    vec![
                        stream(StreamName::Stdout, literal),
                        stream(StreamName::Stdout, literal),
                    ],
                    &mut validate_text,
                )
                .unwrap();
            let result = reducer.finish().unwrap();
            assert_eq!(result.cells[0].outputs.len(), 2);
            assert_eq!(text(&result.cells[0].outputs[0]), literal);
        }
    }
}

#[test]
fn asis_uses_the_validator_boundary_before_hiding_or_clearing_output() {
    let mut reducer = new_reducer();
    let mut prepared = asis_cell(0);
    prepared.options.execution.include.value = false;
    let mut calls = 0;
    let failure = reducer
        .accept_cell(
            &prepared,
            CellOutcome::Ok,
            vec![
                stream(StreamName::Stdout, "![image]("),
                stream(StreamName::Stdout, "../../outside.png)"),
                CellEvent::Clear { wait: false },
            ],
            &mut |candidate: OutputCandidate<'_>| {
                calls += 1;
                assert_eq!(candidate.media_type, "text/markdown");
                assert_eq!(candidate.data, &json!("![image](../../outside.png)"));
                Err(candidate.failure(
                    ExecutionFailureKind::AssetOutsideBoundary,
                    "Outside the boundary.",
                ))
            },
        )
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(failure.kind, ExecutionFailureKind::AssetOutsideBoundary);
    assert!(reducer.finish().is_err());
}

#[test]
fn rejected_asis_markdown_retains_a_literal_fallback_and_its_warning() {
    let mut reducer = new_reducer();
    reducer
        .accept_cell(
            &asis_cell(0),
            CellOutcome::Ok,
            vec![stream(StreamName::Stdout, "<unsafe>")],
            &mut |candidate: OutputCandidate<'_>| {
                if candidate.media_type == "text/markdown" {
                    Ok(CandidateValidation {
                        accepted: None,
                        diagnostics: vec![ExecutionDiagnostic::InvalidTextPayload {
                            attribution: candidate.attribution(),
                            media_type: candidate.media_type.into(),
                        }],
                    })
                } else {
                    validate_text(candidate)
                }
            },
        )
        .unwrap();
    let result = reducer.finish().unwrap();
    assert_eq!(text(&result.cells[0].outputs[0]), "<unsafe>");
    assert_eq!(result.cells[0].outputs[0].diagnostic_indices, [0]);
    assert_eq!(result.diagnostics.len(), 1);
}
