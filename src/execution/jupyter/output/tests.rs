use super::text::validate_text;
use super::*;
use crate::configuration::ExecutionMode;
use crate::diagnostics::DiagnosticPath;
use crate::documents::{AuthoredFormat, prepare_collection_document};
use crate::execution::{CellSkipReason, ExecutionDefaults, OutputVisibility};
use crate::ir::{SourceLocation, SourceSpan, StreamName};
use serde_json::json;

mod fragments;
mod images;

fn page() -> ExecutionPage {
    ExecutionPage {
        source: SourceLocation {
            repository: "docs".into(),
            path: DiagnosticPath::try_from("guide/example.qmd").unwrap(),
            span: None,
        },
        collection: "guide".into(),
        working_directory: Some(DiagnosticPath::try_from("guide").unwrap()),
        source_fingerprint: fingerprint_bytes(b"authored"),
        format: AuthoredFormat::Qmd,
        mode: ExecutionMode::Execute,
        page_veto: false,
        parser_version: "0.29.2".into(),
        qmd_policy: "qmd-mvp-v1".into(),
    }
}

fn cell(ordinal: usize) -> PreparedCell {
    let collection = toml::from_str(
        "id = 'guide'\nowner = 'project'\nrepository = 'docs'\npath = 'guide'\nmount = 'guide'\nformat = 'qmd'\n[execution]\nmode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n",
    ).unwrap();
    let mut cell = prepare_collection_document("```{python}\nx = 1\n```\n", &collection)
        .unwrap()
        .preparation
        .unwrap()
        .cells
        .remove(0);
    cell.ordinal = ordinal;
    cell.cell.span = SourceSpan {
        start: ordinal * 30,
        end: ordinal * 30 + 25,
    };
    cell
}

fn new_reducer() -> OutputReducer {
    OutputReducer::new(page(), ErrorContext::new("/checkout".into()))
}

fn stream(name: StreamName, text: &str) -> CellEvent {
    CellEvent::Stream {
        name,
        text: text.into(),
    }
}

fn bundle(data: Value) -> MimeBundle {
    MimeBundle {
        data,
        metadata: Map::new(),
    }
}

fn display(id: Option<&str>, text: &str) -> CellEvent {
    CellEvent::Display {
        bundle: bundle(json!({"text/plain": text})),
        display_id: id.map(str::to_owned),
    }
}

fn update(id: Option<&str>, text: &str) -> CellEvent {
    CellEvent::UpdateDisplay {
        bundle: bundle(json!({"text/plain": text})),
        display_id: id.map(str::to_owned),
    }
}

fn accept(reducer: &mut OutputReducer, ordinal: usize, events: Vec<CellEvent>) {
    reducer
        .accept_cell(&cell(ordinal), CellOutcome::Ok, events, &mut validate_text)
        .unwrap();
}

fn text(output: &ExecutionOutput) -> &str {
    match &output.output.representations[0] {
        OutputRepresentation::PlainText { text, .. } => text,
        other => panic!("unexpected representation: {other:?}"),
    }
}

#[test]
fn typed_outputs_preserve_protocol_order_and_exact_stream_bytes() {
    let mut reducer = new_reducer();
    let prepared = cell(0);
    let events = vec![
        stream(StreamName::Stdout, "# <b>literal</b>\n"),
        display(None, "display"),
        stream(StreamName::Stderr, "warning\n"),
        CellEvent::Error {
            name: "ValueError".into(),
            message: "bad input".into(),
            traceback: vec!["a frame".into()],
        },
        CellEvent::Result {
            bundle: bundle(json!({"text/plain": "42"})),
            display_id: None,
        },
    ];
    reducer
        .accept_cell(
            &prepared,
            CellOutcome::AllowedError,
            events,
            &mut validate_text,
        )
        .unwrap();
    let result = reducer.finish().unwrap();
    let cell = &result.cells[0];
    assert_eq!(cell.outcome, CellOutcome::AllowedError);
    assert_eq!(
        cell.submitted_source_fingerprint,
        Some(fingerprint_bytes(prepared.cell.source.as_bytes()))
    );
    assert_eq!(cell.source_segments, prepared.cell.source_segments);
    assert_eq!(
        cell.outputs.iter().map(|o| o.slot).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(text(&cell.outputs[0]), "# <b>literal</b>\n");
    assert_eq!(text(&cell.outputs[1]), "display");
    assert_eq!(text(&cell.outputs[2]), "warning\n");
    assert!(
        matches!(&cell.outputs[3].output.kind, CellOutputKind::Error { name, .. } if name == "ValueError")
    );
    assert!(cell.outputs[3].selected_mime_type.is_none());
    assert!(cell.outputs[3].representations.is_empty());
    assert_eq!(text(&cell.outputs[4]), "42");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn updates_replace_every_live_slot_across_cells_without_reordering() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            display(Some("private-id"), "old"),
            stream(StreamName::Stdout, "between"),
            display(Some("private-id"), "old too"),
        ],
    );
    accept(
        &mut reducer,
        1,
        vec![
            CellEvent::Result {
                bundle: bundle(json!({"text/plain": "result"})),
                display_id: Some("private-id".into()),
            },
            update(Some("private-id"), "new"),
        ],
    );
    let result = reducer.finish().unwrap();
    for (owner, slot) in [(0, 0), (0, 2), (1, 0)] {
        let output = &result.cells[owner].outputs[slot];
        assert_eq!(text(output), "new");
        assert_eq!(
            (output.owning_cell, output.producing_cell, output.slot),
            (owner, owner, slot)
        );
        assert_eq!(output.updating_cell, Some(1));
        assert_eq!(output.representations[0].producing_cell, 1);
        assert_eq!(
            output.representations[0].content_fingerprint,
            fingerprint_bytes(b"new")
        );
    }
    assert_eq!(text(&result.cells[0].outputs[1]), "between");
    assert_eq!(result.cells[1].outputs.len(), 1);
    assert!(
        !serde_json::to_string(&result.cells)
            .unwrap()
            .contains("private-id")
    );
}

#[test]
fn clearing_removes_only_current_cell_registrations_and_retains_slot_gaps() {
    let mut reducer = new_reducer();
    accept(&mut reducer, 0, vec![display(Some("shared"), "earlier")]);
    accept(
        &mut reducer,
        1,
        vec![
            display(Some("shared"), "cleared"),
            CellEvent::Clear { wait: false },
            stream(StreamName::Stdout, "survives"),
            update(Some("shared"), "updated"),
        ],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(text(&result.cells[0].outputs[0]), "updated");
    assert_eq!(text(&result.cells[1].outputs[0]), "survives");
    assert_eq!(result.cells[1].outputs[0].slot, 1);
    assert_eq!(result.cells[1].outputs.len(), 1);
}

#[test]
fn deferred_clear_waits_for_output_including_updates_and_expires_with_the_cell() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            display(Some("shared"), "earlier"),
            CellEvent::Clear { wait: true },
        ],
    );
    accept(
        &mut reducer,
        1,
        vec![
            display(Some("shared"), "clear me"),
            CellEvent::Clear { wait: true },
            CellEvent::Clear { wait: true },
            update(Some("shared"), "new"),
            stream(StreamName::Stdout, "after"),
        ],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(text(&result.cells[0].outputs[0]), "new");
    assert_eq!(result.cells[1].outputs.len(), 1);
    assert_eq!(
        (
            result.cells[1].outputs[0].slot,
            text(&result.cells[1].outputs[0])
        ),
        (1, "after")
    );
}

#[test]
fn immediate_clear_cancels_a_pending_clear() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            display(None, "old"),
            CellEvent::Clear { wait: true },
            CellEvent::Clear { wait: false },
            display(None, "one"),
            display(None, "two"),
        ],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(
        result.cells[0]
            .outputs
            .iter()
            .map(|o| (o.slot, text(o)))
            .collect::<Vec<_>>(),
        [(1, "one"), (2, "two")]
    );
}

#[test]
fn unknown_missing_and_cleared_display_ids_warn_without_inventing_slots() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            display(Some("gone"), "old"),
            CellEvent::Clear { wait: false },
            update(Some("gone"), "x"),
            update(Some("unknown"), "y"),
            update(None, "z"),
        ],
    );
    let result = reducer.finish().unwrap();
    assert!(result.cells[0].outputs.is_empty());
    assert_eq!(result.diagnostics.len(), 3);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::UnsupportedCellOutput
                && d.span == Some(cell(0).cell.span))
    );
}

#[test]
fn unsupported_and_empty_bundles_have_payload_free_placeholders() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            CellEvent::Display {
                bundle: bundle(
                    json!({"application/javascript": "secret active payload", "application/json": {"x": 1}}),
                ),
                display_id: None,
            },
            CellEvent::Display {
                bundle: bundle(json!({})),
                display_id: None,
            },
        ],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(result.diagnostics.len(), 2);
    for (index, output) in result.cells[0].outputs.iter().enumerate() {
        assert!(output.unsupported_placeholder().is_some());
        assert_eq!(output.diagnostic_indices, [index]);
    }
    assert_eq!(
        result.cells[0].outputs[0]
            .offered_mime_types
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["application/javascript", "application/json"]
    );
    assert!(
        !serde_json::to_string(&result.cells)
            .unwrap()
            .contains("secret active payload")
    );
}

#[test]
fn every_supported_candidate_is_validated_in_policy_order_even_when_hidden() {
    let mut reducer = new_reducer();
    let mut prepared = cell(0);
    prepared.options.execution.include.value = false;
    prepared.options.execution.output.value = OutputVisibility::Hide;
    let mut observed = Vec::new();
    let mut validator = |candidate: OutputCandidate<'_>| {
        observed.push(candidate.media_type.to_owned());
        assert_eq!(candidate.cell.ordinal, 0);
        assert_eq!(candidate.slot, 0);
        assert_eq!(candidate.metadata["arbitrary"], "metadata");
        if candidate.media_type == "text/plain" {
            validate_text(candidate)
        } else {
            Ok(CandidateValidation {
                accepted: None,
                diagnostics: vec![
                    candidate.warning(DiagnosticCode::InvalidCellOutput, "Rejected candidate."),
                ],
            })
        }
    };
    let data = MIME_PREFERENCE
        .iter()
        .map(|mime| ((*mime).to_owned(), json!("payload")))
        .collect::<Map<_, _>>();
    reducer
        .accept_cell(
            &prepared,
            CellOutcome::Ok,
            vec![CellEvent::Display {
                bundle: MimeBundle {
                    data: Value::Object(data),
                    metadata: json!({"arbitrary": "metadata"})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
                display_id: None,
            }],
            &mut validator,
        )
        .unwrap();
    let result = reducer.finish().unwrap();
    assert_eq!(observed, MIME_PREFERENCE);
    assert_eq!(
        result.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        result.cells[0].outputs[0].diagnostic_indices,
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn a_specific_rejection_does_not_get_a_redundant_unsupported_warning() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![CellEvent::Display {
            bundle: bundle(json!({"text/plain": 123})),
            display_id: None,
        }],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].code,
        DiagnosticCode::InvalidCellOutput
    );
    assert!(
        result.cells[0].outputs[0]
            .unsupported_placeholder()
            .is_some()
    );
}

#[test]
fn clear_and_update_do_not_erase_validation_warnings() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            CellEvent::Display {
                bundle: bundle(json!({"text/plain": false})),
                display_id: Some("id".into()),
            },
            update(Some("id"), "valid"),
            CellEvent::Clear { wait: false },
        ],
    );
    let result = reducer.finish().unwrap();
    assert!(result.cells[0].outputs.is_empty());
    assert_eq!(result.diagnostics.len(), 1);
}

#[test]
fn fatal_validation_is_never_a_fallback_even_for_an_unknown_update() {
    for event in [false, true] {
        let mut reducer = new_reducer();
        let bundle = bundle(json!({"image/png": "fatal", "text/plain": "fallback"}));
        let event = if event {
            CellEvent::UpdateDisplay {
                bundle,
                display_id: Some("unknown".into()),
            }
        } else {
            CellEvent::Display {
                bundle,
                display_id: None,
            }
        };
        let mut calls = 0;
        let failure = reducer
            .accept_cell(
                &cell(0),
                CellOutcome::Ok,
                vec![event, CellEvent::Clear { wait: false }],
                &mut |candidate: OutputCandidate<'_>| {
                    calls += 1;
                    Err(candidate.failure(
                        ExecutionFailureKind::AssetOutsideBoundary,
                        "Invalid asset boundary.",
                    ))
                },
            )
            .unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::AssetOutsideBoundary);
        assert_eq!(calls, 1);
        assert!(reducer.finish().is_err());
    }
}

#[test]
fn skipped_cells_preserve_authored_evidence_without_source_digest_or_outputs() {
    let mut reducer = new_reducer();
    let prepared = cell(0);
    reducer
        .accept_cell(
            &prepared,
            CellOutcome::Skipped {
                reason: CellSkipReason::EvalFalse,
            },
            vec![],
            &mut validate_text,
        )
        .unwrap();
    let result = reducer.finish().unwrap();
    assert!(result.cells[0].outputs.is_empty());
    assert!(result.cells[0].submitted_source_fingerprint.is_none());
    assert_eq!(
        result.cells[0].options.execution,
        ExecutionDefaults::default()
    );
}

#[test]
fn invalid_cell_sequences_and_skipped_events_fail_closed() {
    for (prepared, outcome, events) in [
        (cell(1), CellOutcome::Ok, vec![]),
        (
            cell(0),
            CellOutcome::Skipped {
                reason: CellSkipReason::EvalFalse,
            },
            vec![display(None, "invalid")],
        ),
    ] {
        let mut reducer = new_reducer();
        assert!(
            reducer
                .accept_cell(&prepared, outcome, events, &mut validate_text)
                .is_err()
        );
        assert!(reducer.finish().is_err());
    }
}

#[test]
fn errors_strip_controls_and_normalize_frame_paths_without_rewriting_authored_text() {
    let mut reducer = new_reducer();
    let message = "literal /some/user/path and café";
    accept(&mut reducer, 0, vec![CellEvent::Error {
        name: "\u{1b}[31mValueError\u{1b}[0m".into(), message: message.into(),
        traceback: vec![
            "\u{1b}[31m  File \"/checkout/guide/example.qmd\", line 2\u{1b}[0m".into(),
            "  File \"/private/runtime/lib.py\", line 7".into(),
            "File /checkout/lib/helper.py:12, in helper()".into(),
            "\u{1b}]8;;https://example.com\u{7}ordinary source /some/user/path\u{1b}]8;;\u{1b}\\".into(),
            "File C:\\private\\lib.py:4, in helper()".into(),
        ],
    }]);
    let result = reducer.finish().unwrap();
    let CellOutputKind::Error {
        name,
        message: actual,
        traceback,
    } = &result.cells[0].outputs[0].output.kind
    else {
        panic!()
    };
    assert_eq!(name, "ValueError");
    assert_eq!(actual, message);
    assert_eq!(
        traceback,
        &[
            "  File \"guide/example.qmd\", line 2",
            "  File \"<external-frame>\", line 7",
            "File lib/helper.py:12, in helper()",
            "ordinary source /some/user/path",
            "File <external-frame>:4, in helper()",
        ]
    );
}

// These assets stand in for the separately tested media validator and staging
// boundary; the reducer must only retain references to surviving alternatives.
fn validate_fixture_asset(
    candidate: OutputCandidate<'_>,
) -> Result<CandidateValidation, ExecutionFailure> {
    if candidate.media_type == "text/plain" {
        return validate_text(candidate);
    }
    let bytes = candidate.data.as_str().unwrap().as_bytes();
    let fingerprint = fingerprint_bytes(bytes);
    let asset = AssetReference {
        path: DiagnosticPath::try_from(format!("execution-output/{}.png", fingerprint.value))
            .unwrap(),
        fingerprint: fingerprint.clone(),
    };
    Ok(CandidateValidation {
        accepted: Some(AcceptedRepresentation {
            representation: OutputRepresentation::Asset {
                media_type: candidate.media_type.into(),
                asset,
            },
            content_fingerprint: fingerprint,
            policy: Some("fixture-media-v1".into()),
            provenance: Vec::new(),
        }),
        diagnostics: Vec::new(),
    })
}

#[test]
fn alternatives_keep_policy_order_and_prune_only_unreferenced_assets() {
    let mut reducer = new_reducer();
    reducer.accept_cell(&cell(0), CellOutcome::Ok, vec![
        CellEvent::Display { bundle: bundle(json!({"text/plain": "fallback", "image/png": "png", "image/svg+xml": "svg"})), display_id: Some("first".into()) },
        CellEvent::Result { bundle: bundle(json!({"image/png": "png"})), display_id: Some("second".into()) },
    ], &mut validate_fixture_asset).unwrap();
    accept(&mut reducer, 1, vec![update(Some("first"), "replaced")]);
    let result = reducer.finish().unwrap();
    assert_eq!(result.retained_assets.len(), 1);
    assert_eq!(
        result.retained_assets[0].fingerprint,
        fingerprint_bytes(b"png")
    );
    assert_eq!(
        result.cells[0].outputs[1].selected_mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(
        result.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("text/plain")
    );

    let mut reducer = new_reducer();
    reducer
        .accept_cell(
            &cell(0),
            CellOutcome::Ok,
            vec![CellEvent::Display {
                bundle: bundle(
                    json!({"text/plain": "fallback", "image/png": "png", "image/svg+xml": "svg"}),
                ),
                display_id: None,
            }],
            &mut validate_fixture_asset,
        )
        .unwrap();
    let result = reducer.finish().unwrap();
    let output = &result.cells[0].outputs[0];
    assert_eq!(output.selected_mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(
        output
            .output
            .representations
            .iter()
            .map(representation_media_type)
            .collect::<Vec<_>>(),
        ["image/svg+xml", "image/png", "text/plain"]
    );
    assert_eq!(output.representations.len(), 3);
    assert_eq!(result.retained_assets.len(), 2);
}

#[test]
fn final_figure_counts_use_updated_slots_even_when_the_owner_is_hidden() {
    let mut prepared = cell(0);
    prepared.options.execution.include.value = false;
    prepared.options.fig_subcap.value = vec!["one".into()];
    let mut reducer_with_options = new_reducer();
    reducer_with_options
        .accept_cell(
            &prepared,
            CellOutcome::Ok,
            vec![display(Some("figure"), "not yet a figure")],
            &mut validate_text,
        )
        .unwrap();
    reducer_with_options
        .accept_cell(
            &cell(1),
            CellOutcome::Ok,
            vec![CellEvent::UpdateDisplay {
                bundle: bundle(json!({"image/png": "png", "image/svg+xml": "svg"})),
                display_id: Some("figure".into()),
            }],
            &mut validate_fixture_asset,
        )
        .unwrap();
    assert!(reducer_with_options.finish().is_ok());

    let mut reducer = new_reducer();
    reducer
        .accept_cell(
            &prepared,
            CellOutcome::Ok,
            vec![CellEvent::Display {
                bundle: bundle(json!({"image/png": "png"})),
                display_id: Some("figure".into()),
            }],
            &mut validate_fixture_asset,
        )
        .unwrap();
    accept(&mut reducer, 1, vec![update(Some("figure"), "no figure")]);
    let failure = reducer.finish().err().unwrap();
    assert_eq!(
        failure.diagnostics[0].code,
        DiagnosticCode::InvalidFigureOptions
    );
    assert_eq!(failure.diagnostics[0].span, Some(prepared.cell.span));
}

#[test]
fn malformed_bundles_and_text_arrays_have_deterministic_results() {
    let mut reducer = new_reducer();
    accept(
        &mut reducer,
        0,
        vec![
            CellEvent::Display {
                bundle: bundle(json!(false)),
                display_id: None,
            },
            CellEvent::Result {
                bundle: bundle(
                    json!({"text/plain": ["first\n", "second"], "application/json": {"ignored": true}}),
                ),
                display_id: None,
            },
        ],
    );
    let result = reducer.finish().unwrap();
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].code,
        DiagnosticCode::InvalidCellOutput
    );
    assert_eq!(text(&result.cells[0].outputs[1]), "first\nsecond");
    assert!(result.cells[0].outputs[1].diagnostic_indices.is_empty());
}

#[test]
fn mismatched_validator_media_fails_instead_of_fabricating_evidence() {
    let mut reducer = new_reducer();
    let failure = reducer
        .accept_cell(
            &cell(0),
            CellOutcome::Ok,
            vec![display(None, "text")],
            &mut |candidate: OutputCandidate<'_>| {
                let mut accepted = validate_text(candidate)?;
                let OutputRepresentation::PlainText { media_type, .. } =
                    &mut accepted.accepted.as_mut().unwrap().representation
                else {
                    panic!()
                };
                *media_type = "text/html".into();
                Ok(accepted)
            },
        )
        .unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::OutputValidation);
    assert!(reducer.finish().is_err());
}

#[test]
fn multiline_frames_normalize_runtime_context_and_preserve_source_lines() {
    let errors = ErrorContext::new("/checkout".into());
    let CellOutputKind::Error { traceback, .. } = errors.normalize("Error".into(), "message".into(), vec![
        "Traceback:\n  File \"/checkout/guide/test.py\", line 1\n    print('/private/authored')\nFile /checkout/../outside.py:12\nFile /checkout-other/lib.py:4".into(),
        "Cell In[42], line 3\n    x = 42".into(),
        "\u{9b}31m\u{1b}Pprivate terminal payload\u{1b}\\Frame\u{9b}0m\u{7}".into(),
    ]) else { panic!() };
    assert_eq!(
        traceback,
        [
            "Traceback:\n  File \"guide/test.py\", line 1\n    print('/private/authored')\nFile <external-frame>:12\nFile <external-frame>:4",
            "Cell <cell>, line 3\n    x = 42",
            "Frame",
        ]
    );
}
