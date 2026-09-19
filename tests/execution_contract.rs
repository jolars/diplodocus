use std::collections::{BTreeMap, BTreeSet};
use std::future::{pending, ready};
use std::path::PathBuf;
use std::sync::Mutex;

use diplodocus::configuration::{ExecutionEngine as EngineSelector, ExecutionMode};
use diplodocus::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticPath, DiagnosticSource, Severity,
};
use diplodocus::documents::{AuthoredFormat, parse_authored_document};
use diplodocus::execution::*;
use diplodocus::ir::{
    AssetReference, Block, CellOutput, CellOutputKind, ExecutionOrigin, KernelProvenance,
    OutputRepresentation, ProvenanceActivity, SanitizerProvenance, SourceLocation, SourceSpan,
    StreamName, UnvalidatedHtml,
};
use diplodocus::provenance::{ExecutionObservation, fingerprint_bytes};

fn source() -> SourceLocation {
    SourceLocation {
        repository: "docs".into(),
        path: DiagnosticPath::try_from("guide/example.qmd").unwrap(),
        span: Some(SourceSpan { start: 10, end: 40 }),
    }
}

fn context() -> ExecutionContext<'static> {
    ExecutionContext {
        repository_root: PathBuf::from("/unavailable/checkout"),
        page_path: PathBuf::from("/unavailable/checkout/guide/example.qmd"),
        asset_staging_directory: PathBuf::from("/unavailable/staging"),
        deadlines: ExecutionDeadlines::default(),
        cancellation: Box::pin(pending()),
    }
}

#[test]
fn defaults_match_the_authored_execution_policy() {
    let options = EffectiveCellOptions::default();
    assert!(options.execution.eval.value);
    assert!(options.execution.echo.value);
    assert!(options.execution.include.value);
    assert!(!options.execution.error.value);
    assert_eq!(options.execution.output.value, OutputVisibility::Show);
    assert_eq!(options.label.value, None);
    assert_eq!(options.fig_alt.value, None);
    assert_eq!(options.fig_cap.value, None);
    assert!(options.fig_subcap.value.is_empty());
    assert_eq!(options.execution.eval.origin, OptionOrigin::Default);
    assert_eq!(options.label.origin, OptionOrigin::Default);

    let deadlines = serde_json::to_value(ExecutionDeadlines::default()).unwrap();
    assert_eq!(
        deadlines,
        serde_json::json!({
            "startup": 30_000, "cell": 60_000, "terminal_sync": 5_000,
            "interrupt": 5_000, "shutdown": 5_000, "termination": 5_000,
            "forced_exit": 5_000,
        })
    );
}

#[test]
fn typed_options_preserve_origins_and_ordered_captions() {
    let mut options = EffectiveCellOptions::default();
    options.execution.output = EffectiveOption {
        value: OutputVisibility::AsIs,
        origin: OptionOrigin::Hashpipe {
            span: SourceSpan { start: 12, end: 27 },
        },
    };
    options.label = EffectiveOption {
        value: Some("setup".into()),
        origin: OptionOrigin::FenceIdentifier {
            span: SourceSpan { start: 4, end: 10 },
        },
    };
    options.fig_subcap.value = vec!["Second alphabetically".into(), "First".into()];
    let encoded = serde_json::to_value(&options).unwrap();
    assert_eq!(encoded["execution"]["output"]["value"], "as-is");
    assert_eq!(
        encoded["execution"]["output"]["origin"]["span"]["start"],
        12
    );
    assert_eq!(
        serde_json::from_value::<EffectiveCellOptions>(encoded).unwrap(),
        options
    );
}

struct FakeEngine {
    reply: Mutex<Option<Result<PageExecutionResult, ExecutionFailure>>>,
}

impl ExecutionEngine for FakeEngine {
    fn capabilities(&self) -> ExecutionCapabilities {
        ExecutionCapabilities {
            languages: BTreeSet::from(["python".into(), "r".into()]),
            media_types: BTreeSet::from(["text/plain".into()]),
            features: BTreeSet::from([ExecutionFeature::PageSession]),
        }
    }

    fn requirements(&self) -> ExecutionRequirements {
        ExecutionRequirements {
            operating_systems: BTreeSet::from(["linux".into()]),
            protocol: KernelProtocolRequirement {
                name: "jupyter".into(),
                major: 5,
            },
        }
    }

    fn execute_page<'a>(
        &'a self,
        context: ExecutionContext<'a>,
        request: &'a PageExecutionRequest,
    ) -> ExecutionFuture<'a> {
        Box::pin(async move {
            assert_eq!(request.kernel, "not-an-installed-kernel");
            tokio::task::yield_now().await;
            tokio::select! {
                biased;
                () = context.cancellation => Err(failure(ExecutionFailureKind::Cancelled)),
                reply = ready(self.reply.lock().unwrap().take().unwrap()) => reply,
            }
        })
    }
}

fn failure(kind: ExecutionFailureKind) -> ExecutionFailure {
    ExecutionFailure {
        kind,
        diagnostics: vec![kind.to_diagnostic("examples", source())],
        cleanup_diagnostics: Vec::new(),
    }
}

#[test]
fn descriptive_methods_need_neither_paths_nor_a_runtime() {
    let engine: &dyn ExecutionEngine = &FakeEngine {
        reply: Mutex::new(None),
    };
    assert_eq!(
        engine.capabilities().languages,
        BTreeSet::from(["python".into(), "r".into()])
    );
    assert_eq!(engine.requirements().protocol.major, 5);
    assert_eq!(
        engine.requirements().operating_systems,
        BTreeSet::from(["linux".into()])
    );
}

#[test]
fn timeout_diagnostics_retain_phase_and_portable_source() {
    let diagnostic = ExecutionFailureKind::Timeout {
        phase: ExecutionPhase::TerminalSync,
    }
    .to_diagnostic("examples", source());
    assert_eq!(diagnostic.code, DiagnosticCode::ExecutionTimeout);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.span, source().span);
    assert!(diagnostic.message.contains("terminal synchronization"));
    let encoded = serde_json::to_string(&diagnostic).unwrap();
    assert!(encoded.contains("guide/example.qmd"));
    assert!(!encoded.contains("/unavailable"));
}

fn request() -> PageExecutionRequest {
    let authored = "```{python}\nx = 12\n```\n\n```{python}\n#| error: true\nraise ValueError('expected')\n```\n\n```{python}\n#| eval: false\nnever_run()\n```\n";
    let parsed = parse_authored_document(authored, AuthoredFormat::Qmd);
    assert!(parsed.diagnostics.is_empty());
    let cells = parsed
        .document
        .blocks
        .into_iter()
        .enumerate()
        .map(|(ordinal, block)| {
            let Block::CodeCell(cell) = block else {
                panic!("fixture cell")
            };
            let mut options = EffectiveCellOptions::default();
            if ordinal == 1 {
                options.execution.error = EffectiveOption {
                    value: true,
                    origin: OptionOrigin::Hashpipe {
                        span: cell.options[0].span,
                    },
                };
            }
            if ordinal == 2 {
                options.execution.eval = EffectiveOption {
                    value: false,
                    origin: OptionOrigin::Hashpipe {
                        span: cell.options[0].span,
                    },
                };
            }
            PreparedCell {
                ordinal,
                cell,
                options,
            }
        })
        .collect();
    PageExecutionRequest {
        page: ExecutionPage {
            source: SourceLocation {
                span: None,
                ..source()
            },
            collection: "examples".into(),
            working_directory: Some(DiagnosticPath::try_from("guide").unwrap()),
            source_fingerprint: fingerprint_bytes(authored.as_bytes()),
            format: AuthoredFormat::Qmd,
            mode: ExecutionMode::Execute,
            page_veto: false,
            parser_version: "0.29.0".into(),
            qmd_policy: "qmd-mvp-v1".into(),
        },
        kernel: "not-an-installed-kernel".into(),
        defaults: ExecutionDefaults::default(),
        cells,
        declared_environment_inputs: Vec::new(),
    }
}

fn record(request: &PageExecutionRequest) -> PageExecutionRecord {
    let execution = ExecutionObservation {
        engine: EngineSelector::Jupyter,
        kernel: KernelProvenance {
            name: request.kernel.clone(),
            language: Some("python".into()),
            language_version: None,
            version: None,
        },
        origin: ExecutionOrigin::Executed,
        tools: BTreeMap::from([
            ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
            ("jupyter".into(), env!("CARGO_PKG_VERSION").into()),
        ]),
        declared_environment_inputs: request.declared_environment_inputs.clone(),
    }
    .into_provenance(
        Some(DiagnosticSource::Repository {
            repository: request.page.source.repository.clone(),
            path: request.page.source.path.clone(),
        }),
        None,
    );
    let cells = request
        .cells
        .iter()
        .map(|prepared| CellExecutionResult {
            ordinal: prepared.ordinal,
            language: prepared.cell.language.clone(),
            span: prepared.cell.span,
            source_segments: prepared.cell.source_segments.clone(),
            submitted_source_fingerprint: prepared
                .options
                .execution
                .eval
                .value
                .then(|| fingerprint_bytes(prepared.cell.source.as_bytes())),
            options: prepared.options.clone(),
            outcome: match prepared.ordinal {
                0 => CellOutcome::Ok,
                1 => CellOutcome::AllowedError,
                _ => CellOutcome::Skipped {
                    reason: CellSkipReason::EvalFalse,
                },
            },
            outputs: if prepared.ordinal == 1 {
                vec![error_output()]
            } else {
                Vec::new()
            },
        })
        .collect();
    PageExecutionRecord {
        page: request.page.clone(),
        defaults: request.defaults.clone(),
        cells,
        diagnostics: Vec::new(),
        assets: Vec::new(),
        provenance: Some(PageExecutionProvenance {
            execution,
            engine_build_fingerprint: fingerprint_bytes(b"fixture executable"),
            components: BTreeMap::from([
                (
                    "transport".into(),
                    ExecutionComponent {
                        name: "fake".into(),
                        version: "1".into(),
                    },
                ),
                (
                    "authored-parser".into(),
                    ExecutionComponent {
                        name: "panache-parser".into(),
                        version: "0.29.0".into(),
                    },
                ),
            ]),
            policies: ExecutionPolicies::default(),
            kernel: KernelExecutionProvenance {
                spec_fingerprint: fingerprint_bytes(b"fixture spec"),
                launch_fingerprint: fingerprint_bytes(b"fixture launch"),
                search: vec![KernelSearchLocation {
                    class: KernelSearchClass::JupyterPath,
                    ordinal: 0,
                    selected: true,
                }],
                interrupt_mode: KernelInterruptMode::Message,
                implementation: "fixture".into(),
                protocol_version: "5.3".into(),
            },
            platform: ExecutionPlatform {
                os: "linux".into(),
                architecture: "x86_64".into(),
                target: "x86_64-unknown-linux-gnu".into(),
            },
            deadlines_ms: ExecutionDeadlines::default(),
        }),
    }
}

fn text_output(cell: usize, slot: usize, text: &str) -> ExecutionOutput {
    ExecutionOutput {
        owning_cell: cell,
        producing_cell: cell,
        updating_cell: None,
        slot,
        output: CellOutput {
            kind: CellOutputKind::Stream {
                stream: StreamName::Stdout,
            },
            representations: vec![OutputRepresentation::PlainText {
                media_type: "text/plain".into(),
                text: text.into(),
            }],
            provenance: Vec::new(),
        },
        offered_mime_types: BTreeSet::from(["text/plain".into()]),
        selected_mime_type: Some("text/plain".into()),
        representations: vec![RepresentationEvidence {
            content_fingerprint: fingerprint_bytes(text.as_bytes()),
            producing_cell: cell,
            policy: None,
        }],
        diagnostic_indices: Vec::new(),
    }
}

fn error_output() -> ExecutionOutput {
    let mut output = text_output(1, 0, "expected");
    output.output.kind = CellOutputKind::Error {
        name: "ValueError".into(),
        message: "expected".into(),
        traceback: Vec::new(),
    };
    output.selected_mime_type = None;
    output
}

#[tokio::test]
async fn trait_object_returns_a_complete_page_without_changing_prepared_input() {
    let request = request();
    let original = request.clone();
    let mut record = record(&request);
    record.cells[0].outputs.push(text_output(0, 0, "12\n"));
    let expected = record.clone();
    let engine: Box<dyn ExecutionEngine> = Box::new(FakeEngine {
        reply: Mutex::new(Some(Ok(PageExecutionResult {
            record,
            staged_assets: Vec::new(),
        }))),
    });
    let result = engine.execute_page(context(), &request).await.unwrap();
    assert_eq!(result.record, expected);
    assert_eq!(request, original);
    assert_eq!(result.record.cells[1].outcome, CellOutcome::AllowedError);
    assert!(result.record.diagnostics.is_empty());
    assert!(result.record.cells[2].outputs.is_empty());
    assert!(
        result.record.cells[2]
            .submitted_source_fingerprint
            .is_none()
    );
}

#[tokio::test]
async fn primary_and_cleanup_failures_survive_the_async_boundary() {
    let mut expected = failure(ExecutionFailureKind::Timeout {
        phase: ExecutionPhase::Cell,
    });
    expected
        .cleanup_diagnostics
        .push(ExecutionFailureKind::Cleanup.to_diagnostic("examples", source()));
    let engine: &dyn ExecutionEngine = &FakeEngine {
        reply: Mutex::new(Some(Err(expected.clone()))),
    };
    let actual = engine
        .execute_page(context(), &request())
        .await
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(
        actual.cleanup_diagnostics[0].code,
        DiagnosticCode::ExecutionCleanupFailed
    );
}

#[tokio::test]
async fn cancellation_is_an_awaited_input_and_needs_no_production_runtime_type() {
    let request = request();
    let engine = FakeEngine {
        reply: Mutex::new(Some(Ok(PageExecutionResult {
            record: record(&request),
            staged_assets: Vec::new(),
        }))),
    };
    let mut context = context();
    context.cancellation = Box::pin(ready(()));
    let failure = engine.execute_page(context, &request).await.unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert_eq!(
        failure.diagnostics[0].code,
        DiagnosticCode::ExecutionCancelled
    );
}

#[test]
fn portable_records_preserve_output_updates_slot_gaps_and_diagnostic_indices() {
    let mut record = record(&request());
    let mut updated = text_output(0, 2, "updated by cell 1");
    updated.output.kind = CellOutputKind::Display;
    updated.updating_cell = Some(1);
    updated.representations[0].producing_cell = 1;
    updated
        .offered_mime_types
        .insert("application/x-unsupported".into());
    updated.diagnostic_indices = vec![0];
    record.diagnostics.push(Diagnostic::new(
        DiagnosticCode::UnsupportedCellOutput,
        Severity::Warning,
        "Unknown display update was ignored.",
    ));
    record.cells[0].outputs = vec![text_output(0, 0, "first"), updated];
    let encoded = serde_json::to_value(&record).unwrap();
    let restored: PageExecutionRecord = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored, record);
    let outputs = &restored.cells[0].outputs;
    assert_eq!(
        outputs.iter().map(|output| output.slot).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        (
            outputs[1].owning_cell,
            outputs[1].producing_cell,
            outputs[1].updating_cell
        ),
        (0, 0, Some(1))
    );
    assert_eq!(outputs[1].representations[0].producing_cell, 1);
    assert_eq!(
        restored.diagnostics[outputs[1].diagnostic_indices[0]].code,
        DiagnosticCode::UnsupportedCellOutput
    );
}

#[test]
fn staging_paths_stay_out_of_portable_assets_and_provenance() {
    let mut record = record(&request());
    let fingerprint = fingerprint_bytes(b"fixture asset");
    let reference = AssetReference {
        path: DiagnosticPath::try_from(format!(
            "execution/examples/figure/{}.svg",
            fingerprint.value
        ))
        .unwrap(),
        fingerprint,
    };
    record.assets.push(ExecutionAsset {
        reference: reference.clone(),
        media_type: "image/svg+xml".into(),
        byte_size: 13,
    });
    let mut figure = text_output(0, 0, "figure fallback");
    figure.output.kind = CellOutputKind::Display;
    figure.output.representations.insert(
        0,
        OutputRepresentation::Asset {
            media_type: "image/svg+xml".into(),
            asset: reference.clone(),
        },
    );
    figure.representations.insert(
        0,
        RepresentationEvidence {
            content_fingerprint: reference.fingerprint.clone(),
            producing_cell: 0,
            policy: Some("svg-mvp-v1".into()),
        },
    );
    figure.offered_mime_types.insert("image/svg+xml".into());
    figure.selected_mime_type = Some("image/svg+xml".into());
    record.cells[0].outputs.push(figure);
    let result = PageExecutionResult {
        record,
        staged_assets: vec![StagedExecutionAsset {
            reference,
            path: PathBuf::from("/private/session-123/figure.svg"),
        }],
    };
    let bytes = serde_json::to_string(&result.record).unwrap();
    for excluded in [
        "/private",
        "session-123",
        "connection_file",
        "display_id",
        "execution_count",
        "staged_assets",
    ] {
        assert!(!bytes.contains(excluded), "{excluded}");
    }
    let restored: PageExecutionRecord = serde_json::from_str(&bytes).unwrap();
    assert_eq!(
        restored.assets[0].reference,
        result.staged_assets[0].reference
    );
    assert_eq!(restored.assets[0].byte_size, 13);
}

#[test]
fn html_deserialization_still_returns_an_untrusted_candidate() {
    let mut record = record(&request());
    let mut output = text_output(0, 0, "fallback");
    output.output.kind = CellOutputKind::Display;
    output.output.representations.insert(
        0,
        OutputRepresentation::HtmlCandidate {
            media_type: "text/html".into(),
            html: UnvalidatedHtml::new("<script>untrusted()</script>"),
            policy: "html-mvp-v1".into(),
            sanitizer: SanitizerProvenance {
                name: "fixture".into(),
                version: "1".into(),
            },
        },
    );
    output.representations.insert(
        0,
        RepresentationEvidence {
            content_fingerprint: fingerprint_bytes(b"<script>untrusted()</script>"),
            producing_cell: 0,
            policy: Some("html-mvp-v1".into()),
        },
    );
    output.offered_mime_types.insert("text/html".into());
    output.selected_mime_type = Some("text/html".into());
    record.cells[0].outputs.push(output);
    let restored: PageExecutionRecord =
        serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap();
    let OutputRepresentation::HtmlCandidate { html, .. } =
        &restored.cells[0].outputs[0].output.representations[0]
    else {
        panic!("untrusted candidate")
    };
    assert_eq!(html.as_untrusted_str(), "<script>untrusted()</script>");
}

#[test]
fn cache_origin_does_not_replace_producer_observations() {
    let executed = record(&request());
    let mut restored = executed.clone();
    let activity = &mut restored.provenance.as_mut().unwrap().execution.activity;
    let ProvenanceActivity::Execution { origin, kernel, .. } = activity else {
        panic!("execution")
    };
    *origin = ExecutionOrigin::Cache;
    assert_eq!(kernel.language_version, None);
    assert_eq!(kernel.version, None);
    let mut expected = serde_json::to_value(&executed).unwrap();
    expected["provenance"]["execution"]["activity"]["origin"] = "cache".into();
    assert_eq!(serde_json::to_value(restored).unwrap(), expected);
}

#[test]
fn an_unexecuted_page_has_no_fabricated_kernel_observations() {
    let mut record = record(&request());
    record.provenance = None;
    for cell in &mut record.cells {
        cell.outcome = CellOutcome::Skipped {
            reason: CellSkipReason::LanguageMismatch,
        };
        cell.submitted_source_fingerprint = None;
        cell.outputs.clear();
    }
    let encoded = serde_json::to_value(&record).unwrap();
    assert!(encoded["provenance"].is_null());
    assert_eq!(
        serde_json::from_value::<PageExecutionRecord>(encoded).unwrap(),
        record
    );
}

#[test]
fn metadata_maps_and_sets_serialize_independently_of_insertion_order() {
    let first = record(&request());
    let mut second = first.clone();
    let components = &mut second.provenance.as_mut().unwrap().components;
    *components = components.clone().into_iter().rev().collect();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let capabilities = FakeEngine {
        reply: Mutex::new(None),
    }
    .capabilities();
    let mut reversed = capabilities.clone();
    reversed.languages = capabilities.languages.iter().rev().cloned().collect();
    assert_eq!(
        serde_json::to_vec(&capabilities).unwrap(),
        serde_json::to_vec(&reversed).unwrap()
    );
}

#[test]
fn failure_codes_use_the_shared_diagnostic_serialization() {
    for kind in [
        ExecutionFailureKind::Startup,
        ExecutionFailureKind::Protocol,
        ExecutionFailureKind::InputRequested,
        ExecutionFailureKind::CellError,
        ExecutionFailureKind::Timeout {
            phase: ExecutionPhase::ForcedExit,
        },
        ExecutionFailureKind::Cancelled,
        ExecutionFailureKind::OutputValidation,
        ExecutionFailureKind::AssetOutsideBoundary,
        ExecutionFailureKind::AssetMissing,
        ExecutionFailureKind::AssetCollision,
        ExecutionFailureKind::Cleanup,
    ] {
        let diagnostic = kind.to_diagnostic("examples", source());
        let encoded = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(encoded["code"], kind.diagnostic_code().as_str());
        assert_eq!(
            serde_json::from_value::<Diagnostic>(encoded).unwrap(),
            diagnostic
        );
    }
}
