use super::*;
use crate::configuration::ExecutionMode;
use crate::documents::{AuthoredFormat, prepare_collection_document};
use crate::execution::identity::RepositoryFile;
use crate::execution::{
    ExecutionEngine, ExecutionPage, JupyterEngine, PageExecutionRequest, ValidatedRepresentationRef,
};
use crate::ir::{InputFingerprint, SourceLocation};
use crate::provenance::{PANACHE_VERSION, fingerprint_bytes};
use std::collections::BTreeMap;

const SOURCE: &str = "```{python}\ndefine\n```\n\n```{python}\nuse\n```\n";

fn request(root: &Path, source: &str) -> (ExecutionContext<'static>, PageExecutionRequest) {
    let context = context(root);
    write(&context.page_path, source);
    let config = toml::from_str("id='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='fixture'\n").unwrap();
    let preparation = prepare_collection_document(source, &config)
        .unwrap()
        .preparation
        .unwrap();
    (
        context,
        PageExecutionRequest {
            page: ExecutionPage {
                source: super::source().source,
                collection: "guide".into(),
                working_directory: Some("guide".try_into().unwrap()),
                source_fingerprint: fingerprint_bytes(source.as_bytes()),
                format: AuthoredFormat::Qmd,
                mode: ExecutionMode::Execute,
                page_veto: false,
                parser_version: PANACHE_VERSION.into(),
                qmd_policy: "qmd-mvp-v1".into(),
            },
            kernel: "fixture".into(),
            defaults: preparation.defaults,
            cells: preparation.cells,
            declared_environment_inputs: vec![],
        },
    )
}

fn engine(root: &Path) -> JupyterEngine {
    JupyterEngine::new(BTreeMap::from([("docs".into(), root.to_owned())]), vec![])
        .with_search_environment(environment(root))
}

async fn assert_reaped(root: &Path) {
    let observed = observation(root);
    let connection = Path::new(observed["connection_file"].as_str().unwrap());
    let pid = rustix::process::Pid::from_raw(observed["pid"].as_i64().unwrap() as i32).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while connection.parent().unwrap().exists()
            || rustix::process::test_kill_process(pid) != Err(rustix::io::Errno::SRCH)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn ledger_codes() -> Vec<DiagnosticCode> {
    vec![
        DiagnosticCode::UnsupportedKernelMessage,
        DiagnosticCode::UnsafeKernelHtml,
        DiagnosticCode::UnsupportedKernelMessage,
        DiagnosticCode::UnsupportedAuthoredSyntax,
        DiagnosticCode::UnsupportedKernelMessage,
    ]
}

#[tokio::test]
async fn public_engine_preserves_warning_order_and_offsets() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "execute-ledger-order").await;
    let spec: Value =
        serde_json::from_slice(&std::fs::read(kernel.directory.join("kernel.json")).unwrap())
            .unwrap();
    install(&root.path().join("second"), "fixture", &spec);
    let (context, request) = request(root.path(), SOURCE);
    let result = engine(root.path())
        .execute_page(context, &request)
        .await
        .unwrap();
    let validated = result.validated();
    assert_eq!(validated.execution_diagnostic_offset(), 1);
    assert_eq!(
        validated.record().diagnostics[0].code,
        DiagnosticCode::ShadowedKernelspec
    );
    assert_eq!(
        validated
            .diagnostics()
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        ledger_codes()
            .into_iter()
            .cycle()
            .take(10)
            .collect::<Vec<_>>()
    );
    for (cell, base) in [(0, 1), (1, 6)] {
        assert_eq!(
            validated.record().cells[cell].outputs[0].diagnostic_indices,
            [base + 1]
        );
        assert_eq!(
            validated.record().cells[cell].outputs[1].diagnostic_indices,
            [base + 3]
        );
    }
    assert_reaped(root.path()).await;
}

#[tokio::test]
async fn public_engine_retains_each_warning_once_on_output_or_transport_failure() {
    for mode in ["execute-ledger-fatal", "execute-ledger-timeout"] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        let (mut context, request) = request(root.path(), SOURCE);
        context.deadlines.cell = 500;
        let failure = engine(root.path())
            .execute_page(context, &request)
            .await
            .unwrap_err();
        let mut expected = ledger_codes();
        if mode == "execute-ledger-timeout" {
            expected.push(DiagnosticCode::UnsupportedKernelMessage);
            assert_eq!(
                failure.kind,
                ExecutionFailureKind::Timeout {
                    phase: ExecutionPhase::Cell
                }
            );
        } else {
            assert_eq!(failure.kind, ExecutionFailureKind::AssetMissing);
        }
        expected.push(failure.kind.diagnostic_code());
        assert_eq!(
            failure
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>(),
            expected,
            "{mode}"
        );
        assert!(failure.cleanup_diagnostics.is_empty());
        assert_reaped(root.path()).await;
    }
}

#[tokio::test]
async fn public_engine_revalidates_source_and_declared_inputs_after_cleanup() {
    for mode in ["execute-source-change", "execute-environment-change"] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        write(root.path().join("environment.txt"), "original");
        let (context, mut request) = request(root.path(), SOURCE);
        request.declared_environment_inputs.push(InputFingerprint {
            source: SourceLocation {
                repository: "docs".into(),
                path: "environment.txt".try_into().unwrap(),
                span: None,
            },
            fingerprint: fingerprint_bytes(b"original"),
        });
        let engine = JupyterEngine::new(
            BTreeMap::from([("docs".into(), root.path().to_owned())]),
            vec![RepositoryFile::new("docs", Path::new("environment.txt")).unwrap()],
        )
        .with_search_environment(environment(root.path()));
        let staging = context.asset_staging_directory.clone();
        let failure = engine.execute_page(context, &request).await.unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::InputChanged, "{mode}");
        assert!(failure.cleanup_diagnostics.is_empty());
        assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
        assert_reaped(root.path()).await;
    }
}

#[tokio::test]
async fn public_engine_rejects_forged_preparation_and_undeclared_digest_claims_before_launch() {
    for forged_source in [false, true] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-images").await;
        let (context, mut request) = request(root.path(), SOURCE);
        if forged_source {
            request.cells[0].cell.source = "not the authored code".into();
        } else {
            write(root.path().join("environment.txt"), "original");
            request.declared_environment_inputs.push(InputFingerprint {
                source: SourceLocation {
                    repository: "docs".into(),
                    path: "environment.txt".try_into().unwrap(),
                    span: None,
                },
                fingerprint: fingerprint_bytes(b"original"),
            });
        }
        let failure = engine(root.path())
            .execute_page(context, &request)
            .await
            .unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::InputChanged);
        assert!(!root.path().join("observations.json").exists());
        assert!(!root.path().join("assets").exists());
    }
}

#[tokio::test]
async fn public_engine_skips_unmatched_languages_without_resolving_an_executable() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "normal").await;
    edit_kernel(root.path(), &kernel, |spec| {
        spec["argv"][0] = json!("missing-executable")
    })
    .await;
    let (context, request) = request(root.path(), "```{r}\n1\n```\n");
    let result = engine(root.path())
        .execute_page(context, &request)
        .await
        .unwrap();
    assert!(result.validated().record().provenance.is_none());
    assert!(
        result
            .validated()
            .record()
            .cells
            .iter()
            .all(|cell| matches!(
                cell.outcome,
                crate::execution::CellOutcome::Skipped {
                    reason: crate::execution::CellSkipReason::LanguageMismatch
                }
            ))
    );
    assert!(!root.path().join("observations.json").exists());
    assert!(!root.path().join("assets").exists());
}

#[tokio::test]
async fn public_engine_cancellation_and_drop_reap_the_kernel_and_discard_staging() {
    for drop_future in [false, true] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-image-then-hang").await;
        let (mut context, request) = request(root.path(), SOURCE);
        let staging = context.asset_staging_directory.clone();
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        context.cancellation = Box::pin(async {
            let _ = cancelled.await;
        });
        let engine = engine(root.path());
        let task = tokio::spawn(async move { engine.execute_page(context, &request).await });
        tokio::time::timeout(Duration::from_secs(30), async {
            while std::fs::read_to_string(root.path().join("requests"))
                .map(|s| s.lines().count())
                .unwrap_or(0)
                < 2
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(std::fs::read_dir(&staging).unwrap().next().is_some());
        if drop_future {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            cancel.send(()).unwrap();
            let failure = task.await.unwrap().unwrap_err();
            assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
            assert!(failure.cleanup_diagnostics.is_empty());
        }
        assert_reaped(root.path()).await;
        assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn public_engine_retains_validated_outputs_only_after_cleanup() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-images").await;
    let (context, request) = request(root.path(), SOURCE);
    let staging = context.asset_staging_directory.clone();
    let engine = engine(root.path());
    let public: &dyn ExecutionEngine = &engine;
    let result = public.execute_page(context, &request).await.unwrap();
    let record = result.validated().record();
    assert_eq!(record.cells.len(), 2);
    assert_eq!(
        record.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(result.staged_assets().len(), 1);
    let provenance = record.provenance.as_ref().unwrap();
    assert_eq!(provenance.kernel.implementation, "fixture");
    assert_eq!(provenance.kernel.protocol_version, "5.3");
    assert_eq!(
        provenance.execution.tools["diplodocus"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        provenance.components["authored-parser"].version,
        PANACHE_VERSION
    );
    assert!(result.staged_assets()[0].path.starts_with(&staging));
    let observed = observation(root.path());
    let connection = Path::new(observed["connection_file"].as_str().unwrap());
    assert!(!connection.parent().unwrap().exists());
    let pid = rustix::process::Pid::from_raw(observed["pid"].as_i64().unwrap() as i32).unwrap();
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        !serde_json::to_string(record)
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
}

#[tokio::test]
async fn public_engine_stops_on_fatal_output_and_rolls_back() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-generated-html-image").await;
    let (context, request) = request(root.path(), SOURCE);
    let staging = context.asset_staging_directory.clone();
    let failure = engine(root.path())
        .execute_page(context, &request)
        .await
        .unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::AssetMissing);
    assert!(failure.cleanup_diagnostics.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.path().join("requests"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
    let observed = observation(root.path());
    assert!(
        !Path::new(observed["connection_file"].as_str().unwrap())
            .parent()
            .unwrap()
            .exists()
    );
}

#[tokio::test]
async fn public_engine_runs_declared_python_and_r_with_state_and_validated_rich_output() {
    for (kernel, source) in [
        (
            "python3",
            "```{python}\nvalue = 40\n```\n\n```{python}\nprint(value + 2)\nfrom IPython.display import display, HTML, Markdown\ndisplay(HTML('<strong>safe html</strong>'))\ndisplay(Markdown('**safe markdown**'))\n```\n",
        ),
        (
            "ir",
            "```{r}\nvalue <- 40\n```\n\n```{r}\ncat(value + 2, '\\n')\nIRdisplay::display_html('<strong>safe html</strong>')\nIRdisplay::display_markdown('**safe markdown**')\n```\n",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let (mut context, mut request) = request(root.path(), source);
        context.deadlines = ExecutionDeadlines::default();
        request.kernel = kernel.into();
        let engine = JupyterEngine::new(
            BTreeMap::from([("docs".into(), root.path().to_owned())]),
            vec![],
        );
        let result = engine.execute_page(context, &request).await.unwrap();
        let validated = result.validated();
        assert!(
            validated
                .record()
                .provenance
                .as_ref()
                .unwrap()
                .kernel
                .protocol_version
                .starts_with("5.")
        );
        let representations: Vec<_> = validated.record().cells[1]
            .outputs
            .iter()
            .flat_map(|output| {
                (0..output.representations.len())
                    .filter_map(|index| validated.representation(1, output.slot, index))
            })
            .collect();
        assert!(
            representations.iter().any(
                |r| matches!(r, ValidatedRepresentationRef::Text(text) if text.contains("42"))
            ),
            "{kernel}: {representations:?}"
        );
        assert!(
            representations
                .iter()
                .any(|r| matches!(r, ValidatedRepresentationRef::Html(_))),
            "{kernel}: {representations:?}"
        );
        assert!(
            representations
                .iter()
                .any(|r| matches!(r, ValidatedRepresentationRef::Markdown(_))),
            "{kernel}: {representations:?}"
        );
        assert!(result.staged_assets().is_empty());
        assert!(!root.path().join("assets").exists());
    }
}

#[tokio::test]
async fn public_engine_validates_final_updated_figures_before_hiding_output() {
    for replace_with_figure in [true, false] {
        let root = TempDir::new().unwrap();
        let replacement = if replace_with_figure {
            "SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\")"
        } else {
            "HTML('<strong>no figure</strong>')"
        };
        let source = format!(
            "```{{python}}\n#| include: false\n#| fig-subcap: [one]\nfrom IPython.display import display, SVG, HTML, clear_output\nhandle = display(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>\"), display_id=True)\n```\n\n```{{python}}\nclear_output(wait=True)\nprint('cleared')\nclear_output(wait=False)\nhandle.update({replacement})\n```\n"
        );
        let (mut context, mut request) = request(root.path(), &source);
        context.deadlines = ExecutionDeadlines::default();
        request.kernel = "python3".into();
        let staging = context.asset_staging_directory.clone();
        let engine = JupyterEngine::new(
            BTreeMap::from([("docs".into(), root.path().to_owned())]),
            vec![],
        );
        let result = engine.execute_page(context, &request).await;
        if replace_with_figure {
            let result = result.unwrap();
            let validated = result.validated();
            let cells = &validated.record().cells;
            assert!(!cells[0].options.execution.include.value);
            assert_eq!(cells[0].outputs.len(), 1);
            assert_eq!(cells[0].outputs[0].updating_cell, Some(1));
            assert_eq!(
                cells[0].outputs[0].selected_mime_type.as_deref(),
                Some("image/svg+xml")
            );
            assert!(matches!(
                validated.representation(0, 0, 0),
                Some(ValidatedRepresentationRef::Asset(_))
            ));
            assert!(cells[1].outputs.is_empty());
            assert_eq!(result.staged_assets().len(), 1);
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.kind, ExecutionFailureKind::OutputValidation);
            assert!(
                failure
                    .diagnostics
                    .iter()
                    .any(|d| d.code == DiagnosticCode::InvalidFigureOptions)
            );
            assert!(failure.cleanup_diagnostics.is_empty());
            assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
        }
    }
}

#[tokio::test]
async fn public_engine_enforces_startup_and_terminal_deadlines() {
    for (mode, phase) in [
        ("no-iopub", ExecutionPhase::Startup),
        ("execute-no-idle", ExecutionPhase::TerminalSync),
        ("execute-no-reply", ExecutionPhase::TerminalSync),
    ] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        let (mut context, request) = request(root.path(), SOURCE);
        if phase == ExecutionPhase::Startup {
            context.deadlines.startup = 1000;
        }
        context.deadlines.terminal_sync = 300;
        let failure = engine(root.path())
            .execute_page(context, &request)
            .await
            .unwrap_err();
        assert_eq!(
            failure.kind,
            ExecutionFailureKind::Timeout { phase },
            "{mode}"
        );
        assert!(
            failure.cleanup_diagnostics.is_empty(),
            "{mode}: {failure:?}"
        );
        assert_reaped(root.path()).await;
        if phase != ExecutionPhase::Startup {
            let events = std::fs::read_to_string(root.path().join("events")).unwrap();
            assert!(
                events.lines().any(|event| event == "interrupt"),
                "{mode}: {events}"
            );
            assert!(
                events.lines().any(|event| event == "shutdown"),
                "{mode}: {events}"
            );
        }
        assert!(!root.path().join("assets").exists());
    }
}

#[tokio::test]
async fn public_engine_bounds_unresponsive_shutdown_before_returning_success() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-ignore-shutdown").await;
    let (context, request) = request(root.path(), SOURCE);
    let result = engine(root.path())
        .execute_page(context, &request)
        .await
        .unwrap();
    assert_eq!(result.validated().record().cells.len(), 2);
    let events = std::fs::read_to_string(root.path().join("events")).unwrap();
    assert!(events.lines().any(|event| event == "shutdown"));
    assert!(events.lines().any(|event| event == "terminate"));
    assert_reaped(root.path()).await;
}

#[tokio::test]
async fn public_engine_rejects_ineligible_pages_before_reading_source_or_discovering_a_kernel() {
    for case in 0..4 {
        let root = TempDir::new().unwrap();
        let (context, mut request) = request(root.path(), SOURCE);
        std::fs::remove_file(&context.page_path).unwrap();
        match case {
            0 => request.page.mode = ExecutionMode::Never,
            1 => request.page.page_veto = true,
            2 => request.cells.clear(),
            _ => {
                for cell in &mut request.cells {
                    cell.options.execution.eval.value = false;
                }
            }
        }
        let failure = engine(root.path())
            .execute_page(context, &request)
            .await
            .unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::Startup);
        assert_eq!(failure.diagnostics.len(), 1);
        assert!(!root.path().join("assets").exists());
    }
}

fn assert_private_data_absent(record: &crate::execution::PageExecutionRecord, root: &Path) {
    let observed = observation(root);
    let connection = &observed["connection"];
    let forbidden_numbers = [
        observed["pid"].as_u64().unwrap(),
        connection["shell_port"].as_u64().unwrap(),
        connection["iopub_port"].as_u64().unwrap(),
        connection["stdin_port"].as_u64().unwrap(),
        connection["control_port"].as_u64().unwrap(),
        connection["hb_port"].as_u64().unwrap(),
    ];
    fn inspect(value: &Value, numbers: &[u64]) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    assert!(
                        !matches!(
                            key.as_str(),
                            "connection"
                                | "connection_file"
                                | "pid"
                                | "process_id"
                                | "port"
                                | "shell_port"
                                | "iopub_port"
                                | "stdin_port"
                                | "control_port"
                                | "hb_port"
                                | "timestamp"
                                | "date"
                                | "created_at"
                                | "started_at"
                                | "finished_at"
                        ),
                        "private field: {key}"
                    );
                    if key != "deadlines_ms" {
                        inspect(value, numbers);
                    }
                }
            }
            Value::Array(values) => values.iter().for_each(|value| inspect(value, numbers)),
            Value::Number(number) => {
                assert!(!number.as_u64().is_some_and(|n| numbers.contains(&n)))
            }
            Value::String(text) => assert!(!numbers.iter().any(|n| text == &n.to_string())),
            _ => {}
        }
    }
    let provenance = serde_json::to_value(record.provenance.as_ref().unwrap()).unwrap();
    inspect(&provenance, &forbidden_numbers);
    let serialized = provenance.to_string();
    let executable = std::env::current_exe().unwrap();
    let connection_path = Path::new(observed["connection_file"].as_str().unwrap());
    for private in [
        root.to_str().unwrap(),
        executable.to_str().unwrap(),
        connection_path.parent().unwrap().to_str().unwrap(),
        connection["key"].as_str().unwrap(),
        observed["literal"].as_str().unwrap(),
    ] {
        assert!(!private.is_empty());
        assert!(
            !serialized.contains(private),
            "private data in provenance: {private}"
        );
    }
}

#[tokio::test]
async fn cache_hit_restores_assets_without_submitting_cells() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-images").await;
    let engine = engine(root.path()).with_cache_root(root.path().join("cache"));
    let (context, input) = request(root.path(), SOURCE);
    let first = engine.execute_page(context, &input).await.unwrap();
    assert_private_data_absent(first.validated().record(), root.path());
    let first_observation = observation(root.path());
    let submitted = std::fs::read(root.path().join("requests")).unwrap();
    let bytes = std::fs::read(&first.staged_assets()[0].path).unwrap();
    std::fs::remove_dir_all(root.path().join("assets")).unwrap();
    let (context, input) = request(root.path(), SOURCE);
    let second = engine.execute_page(context, &input).await.unwrap();
    assert_eq!(
        std::fs::read(root.path().join("requests")).unwrap(),
        submitted
    );
    assert_eq!(
        std::fs::read(&second.staged_assets()[0].path).unwrap(),
        bytes
    );
    assert_private_data_absent(second.validated().record(), root.path());
    assert_ne!(
        first_observation["connection_file"],
        observation(root.path())["connection_file"]
    );
    let mut expected = serde_json::to_value(first.validated().record()).unwrap();
    let actual = serde_json::to_value(second.validated().record()).unwrap();
    // Only activity origin changes when the complete producing result is restored.
    fn mark_cached(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if fields.get("kind") == Some(&json!("execution"))
                    && fields.get("origin") == Some(&json!("executed"))
                {
                    fields.insert("origin".into(), json!("cache"));
                }
                fields.values_mut().for_each(mark_cached);
            }
            Value::Array(values) => values.iter_mut().for_each(mark_cached),
            _ => {}
        }
    }
    mark_cached(&mut expected);
    assert_eq!(actual, expected);
    assert_reaped(root.path()).await;

    // Independent execution must not acquire fresh timestamps or session identifiers.
    std::fs::remove_dir_all(root.path().join("cache")).unwrap();
    let (context, input) = request(root.path(), SOURCE);
    let fresh = engine.execute_page(context, &input).await.unwrap();
    assert_private_data_absent(fresh.validated().record(), root.path());
    assert_eq!(fresh.validated().record(), first.validated().record());
    assert_reaped(root.path()).await;
}

fn cache_manifest(root: &Path) -> PathBuf {
    let mut entries = std::fs::read_dir(root.join("cache/v1/sha256")).unwrap();
    entries
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("manifest.json")
}

#[tokio::test]
async fn cache_rejection_reexecutes_once_and_never_falls_back_on_failure() {
    for fails in [false, true] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-images").await;
        let engine = engine(root.path()).with_cache_root(root.path().join("cache"));
        let (context, input) = request(root.path(), SOURCE);
        let first = engine.execute_page(context, &input).await.unwrap();
        let manifest = cache_manifest(root.path());
        let value: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let asset = manifest.parent().unwrap().join("assets/sha256").join(
            value["result"]["assets"][0]["digest"]
                .as_str()
                .unwrap()
                .strip_prefix("sha256:")
                .unwrap(),
        );
        write(&asset, "corrupt asset");
        std::fs::remove_dir_all(root.path().join("assets")).unwrap();
        if fails {
            write(root.path().join("fail-execution"), "fail");
        }
        let (context, input) = request(root.path(), SOURCE);
        let result = engine.execute_page(context, &input).await;
        let diagnostics = if fails {
            let failure = result.unwrap_err();
            assert_eq!(failure.kind, ExecutionFailureKind::Protocol);
            assert_eq!(std::fs::read(&asset).unwrap(), b"corrupt asset");
            failure.diagnostics
        } else {
            let result = result.unwrap();
            let mut actual = result.validated().portable_record();
            assert_eq!(
                actual.diagnostics.remove(0).code,
                DiagnosticCode::InvalidExecutionCache
            );
            // Current warnings shift references, but never enter the cache ledger.
            for cell in &mut actual.cells {
                for output in &mut cell.outputs {
                    for index in &mut output.diagnostic_indices {
                        *index -= 1;
                    }
                }
            }
            assert_eq!(&actual, first.validated().record());
            result.validated().record().diagnostics.clone()
        };
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::InvalidExecutionCache)
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("requests"))
                .unwrap()
                .lines()
                .count(),
            if fails { 3 } else { 4 }
        );
        assert_reaped(root.path()).await;
    }
}

#[tokio::test]
async fn cache_hit_replays_warnings_with_current_discovery_offset() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "execute-ledger-order").await;
    let spec: Value =
        serde_json::from_slice(&std::fs::read(kernel.directory.join("kernel.json")).unwrap())
            .unwrap();
    install(&root.path().join("second"), "fixture", &spec);
    let engine = engine(root.path()).with_cache_root(root.path().join("cache"));
    let (context, input) = request(root.path(), SOURCE);
    let first = engine.execute_page(context, &input).await.unwrap();
    let (context, input) = request(root.path(), SOURCE);
    let second = engine.execute_page(context, &input).await.unwrap();
    assert_eq!(
        first.validated().diagnostics(),
        second.validated().diagnostics()
    );
    assert_eq!(
        first.validated().record().cells,
        second.validated().record().cells
    );
    assert_eq!(
        first.validated().record().diagnostics,
        second.validated().record().diagnostics
    );
    assert_eq!(second.validated().execution_diagnostic_offset(), 1);
    assert_eq!(
        std::fs::read_to_string(root.path().join("requests"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_reaped(root.path()).await;
}

#[tokio::test]
async fn cache_invalidates_prose_runtime_and_declared_environment() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-images").await;
    write(root.path().join("environment.txt"), "first");
    let engine = JupyterEngine::new(
        BTreeMap::from([("docs".into(), root.path().to_owned())]),
        vec![RepositoryFile::new("docs", Path::new("environment.txt")).unwrap()],
    )
    .with_search_environment(environment(root.path()))
    .with_cache_root(root.path().join("cache"));
    for round in 0..5 {
        if round == 2 {
            write(root.path().join("runtime-version"), "3.1");
        }
        if round == 3 {
            write(root.path().join("environment.txt"), "second");
        }
        let source = if round == 0 {
            SOURCE.into()
        } else {
            format!("Edited prose.\n\n{SOURCE}")
        };
        let (context, mut input) = request(root.path(), &source);
        input.declared_environment_inputs.push(InputFingerprint {
            source: SourceLocation {
                repository: "docs".into(),
                path: "environment.txt".try_into().unwrap(),
                span: None,
            },
            fingerprint: fingerprint_bytes(
                &std::fs::read(root.path().join("environment.txt")).unwrap(),
            ),
        });
        let result = engine.execute_page(context, &input).await.unwrap();
        assert!(
            result
                .validated()
                .record()
                .diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::ExecutionCacheUnavailable)
        );
        let expected = if round == 4 { 8 } else { (round + 1) * 2 };
        assert_eq!(
            std::fs::read_to_string(root.path().join("requests"))
                .unwrap()
                .lines()
                .count(),
            expected
        );
    }
    assert_reaped(root.path()).await;
}

#[tokio::test]
async fn cache_work_remains_supervised_and_is_joined_before_failure() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for cancel in [true, false] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), "normal").await;
        let mut context = context(root.path());
        let session = start_session(kernel, &mut context, super::source())
            .await
            .unwrap();
        let completed = Arc::new(AtomicBool::new(false));
        let flag = completed.clone();
        let work = async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            flag.store(true, Ordering::SeqCst);
        };
        if cancel {
            context.cancellation = Box::pin(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
            });
        } else {
            let observed = observation(root.path());
            let pid =
                rustix::process::Pid::from_raw(observed["pid"].as_i64().unwrap() as i32).unwrap();
            rustix::process::kill_process(pid, rustix::process::Signal::KILL).unwrap();
        }
        let result = session.supervise(work, &mut context.cancellation).await;
        let failure = match result {
            Err(failure) => failure,
            Ok(_) => panic!("cache work survived cancellation or child death"),
        };
        assert_eq!(
            failure.kind,
            if cancel {
                ExecutionFailureKind::Cancelled
            } else {
                ExecutionFailureKind::Protocol
            }
        );
        assert!(completed.load(Ordering::SeqCst));
        assert!(!root.path().join("requests").exists());
        assert_reaped(root.path()).await;
    }
}

#[tokio::test]
async fn cache_hit_requires_current_inputs_and_successful_asset_staging() {
    for change_source in [true, false] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-images").await;
        let engine = engine(root.path()).with_cache_root(root.path().join("cache"));
        let (context, input) = request(root.path(), SOURCE);
        engine.execute_page(context, &input).await.unwrap();
        let manifest = cache_manifest(root.path());
        let original = std::fs::read(&manifest).unwrap();
        std::fs::remove_dir_all(root.path().join("assets")).unwrap();
        if change_source {
            write(root.path().join("mutate-on-shutdown"), "change");
        } else {
            write(root.path().join("assets"), "sentinel");
        }
        let (context, input) = request(root.path(), SOURCE);
        let failure = engine.execute_page(context, &input).await.unwrap_err();
        if change_source {
            assert_eq!(failure.kind, ExecutionFailureKind::InputChanged);
        } else {
            assert_eq!(
                std::fs::read(root.path().join("assets")).unwrap(),
                b"sentinel"
            );
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("requests"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), original);
        assert_reaped(root.path()).await;
    }
}
