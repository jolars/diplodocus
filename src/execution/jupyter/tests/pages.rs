use super::*;
use crate::configuration::ExecutionMode;
use crate::documents::{AuthoredFormat, prepare_collection_document};
use crate::execution::{CellOutcome, CellSkipReason, ExecutionPage, PageExecutionRequest};
use crate::provenance::fingerprint_bytes;

use super::super::execution::CellEvent;
use super::super::output::text::validate_text;
use super::super::output::{ErrorContext, OutputReducer};
use super::super::page::{execute_page, execute_page_with_environment};

fn request(authored: &str) -> PageExecutionRequest {
    let collection = toml::from_str(
        "id = 'guide'\nowner = 'project'\nrepository = 'docs'\npath = 'guide'\nmount = 'guide'\nformat = 'qmd'\n[execution]\nmode = 'execute'\nengine = 'jupyter'\nkernel = 'fixture'\n",
    ).unwrap();
    let prepared = prepare_collection_document(authored, &collection).unwrap();
    assert!(
        prepared.parsed.diagnostics.is_empty(),
        "{:?}",
        prepared.parsed.diagnostics
    );
    let prepared = prepared.preparation.unwrap();
    PageExecutionRequest {
        page: ExecutionPage {
            source: source().source,
            collection: "guide".into(),
            working_directory: Some(DiagnosticPath::try_from("guide").unwrap()),
            source_fingerprint: fingerprint_bytes(authored.as_bytes()),
            format: AuthoredFormat::Qmd,
            mode: ExecutionMode::Execute,
            page_veto: prepared.page_veto,
            parser_version: "0.29.2".into(),
            qmd_policy: "qmd-mvp-v1".into(),
        },
        kernel: "fixture".into(),
        defaults: prepared.defaults,
        cells: prepared.cells,
        declared_environment_inputs: Vec::new(),
    }
}

fn two_cells() -> PageExecutionRequest {
    request("```{python}\ndefine\n```\n\n```{python}\nuse\n```\n")
}

fn submitted(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("requests"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn asset_boundary_failure_stops_before_the_next_cell_and_reaps_the_kernel() {
    use super::super::output::images::validate_with_assets;
    use crate::execution::assets::PageAssetStore;

    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "execute-images").await;
    let request = two_cells();
    let outside = TempDir::new().unwrap();
    let boundary = root.path().join("staging");
    std::os::unix::fs::symlink(outside.path(), &boundary).unwrap();
    let mut assets =
        PageAssetStore::new(request.page.clone(), root.path().to_owned(), boundary).unwrap();
    let mut reducer = OutputReducer::new(
        request.page.clone(),
        ErrorContext::new(root.path().to_owned()),
    );
    let mut context = context(root.path());
    let session = start_session(kernel, &mut context, source()).await.unwrap();
    let failure = session
        .execute_with(
            request.cells.clone(),
            &mut context.cancellation,
            |mut completed| {
                let result = reducer.accept_cell(
                    &request.cells[completed.ordinal],
                    completed.outcome,
                    std::mem::take(&mut completed.events),
                    &mut |candidate| validate_with_assets(candidate, &mut assets),
                );
                ready(result.map(|()| completed))
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::AssetOutsideBoundary);
    assert_eq!(submitted(root.path()).len(), 1);
    assert!(reducer.finish().is_err());
    assets.rollback().unwrap();
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn figures_are_staged_incrementally_and_retained_after_kernel_cleanup() {
    use super::super::output::images::validate_with_assets;
    use crate::execution::assets::PageAssetStore;

    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "execute-images").await;
    let request = two_cells();
    let mut assets = PageAssetStore::new(
        request.page.clone(),
        root.path().to_owned(),
        root.path().join("staging"),
    )
    .unwrap();
    let mut reducer = OutputReducer::new(
        request.page.clone(),
        ErrorContext::new(root.path().to_owned()),
    );
    let mut context = context(root.path());
    let session = start_session(kernel, &mut context, source()).await.unwrap();
    session
        .execute_with(
            request.cells.clone(),
            &mut context.cancellation,
            |mut completed| {
                assert_eq!(submitted(root.path()).len(), completed.ordinal + 1);
                let result = reducer.accept_cell(
                    &request.cells[completed.ordinal],
                    completed.outcome,
                    std::mem::take(&mut completed.events),
                    &mut |candidate| validate_with_assets(candidate, &mut assets),
                );
                ready(result.map(|()| completed))
            },
        )
        .await
        .unwrap();
    assert_cleaned(root.path()).await;
    let reduced = reducer.finish().unwrap();
    let retained = assets.retain(&reduced.retained_assets).unwrap();
    assert_eq!(retained.assets.len(), 1);
    assert_eq!(reduced.cells.len(), 2);
    assert!(
        reduced
            .cells
            .iter()
            .all(|cell| cell.outputs[0].selected_mime_type.as_deref() == Some("image/svg+xml"))
    );
    let bytes = std::fs::read(&retained.staged_assets[0].path).unwrap();
    assert_eq!(
        retained.assets[0].reference.fingerprint,
        fingerprint_bytes(&bytes)
    );
}

#[tokio::test]
async fn pending_asset_validation_remains_cancellable_and_supervised() {
    use super::super::output::images::validate_with_assets;
    use crate::execution::assets::PageAssetStore;

    for action in ["cancel", "drop", "exit"] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), "execute-images").await;
        let request = two_cells();
        let mut context = context(root.path());
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        context.cancellation = Box::pin(async {
            let _ = cancelled.await;
        });
        let session = start_session(kernel, &mut context, source()).await.unwrap();
        let mut assets = PageAssetStore::new(
            request.page.clone(),
            root.path().to_owned(),
            root.path().join("staging"),
        )
        .unwrap();
        let mut reducer = OutputReducer::new(
            request.page.clone(),
            ErrorContext::new(root.path().to_owned()),
        );
        let (started, processing) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut started = Some(started);
            let result = session
                .execute_with(
                    request.cells.clone(),
                    &mut context.cancellation,
                    |mut completed| {
                        reducer
                            .accept_cell(
                                &request.cells[completed.ordinal],
                                completed.outcome,
                                std::mem::take(&mut completed.events),
                                &mut |candidate| validate_with_assets(candidate, &mut assets),
                            )
                            .unwrap();
                        started.take().unwrap().send(()).unwrap();
                        pending::<Result<_, crate::execution::ExecutionFailure>>()
                    },
                )
                .await;
            assets.rollback().unwrap();
            result
        });
        tokio::time::timeout(Duration::from_secs(5), processing)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submitted(root.path()).len(), 1);
        match action {
            "cancel" => cancel.send(()).unwrap(),
            "drop" => task.abort(),
            "exit" => {
                let pid = rustix::process::Pid::from_raw(
                    observation(root.path())["pid"].as_i64().unwrap() as i32,
                )
                .unwrap();
                rustix::process::kill_process(pid, rustix::process::Signal::KILL).unwrap();
            }
            _ => unreachable!(),
        }
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap();
        if action == "drop" {
            assert!(result.unwrap_err().is_cancelled());
        } else {
            assert_eq!(
                result.unwrap().unwrap_err().kind,
                if action == "cancel" {
                    ExecutionFailureKind::Cancelled
                } else {
                    ExecutionFailureKind::Protocol
                }
            );
        }
        assert_cleaned(root.path()).await;
        assert_eq!(
            std::fs::read_dir(root.path().join("staging"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(submitted(root.path()).len(), 1);
    }
}

#[tokio::test]
async fn a_page_submits_exact_source_sequentially_in_one_fresh_session() {
    for mode in ["execute-reply-first", "execute-idle-first"] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        let request = request(
            "```{Python3}\ndefine\n```\n\n> ```{python}\n> use\n> ```\n\n- ```{python}\n  use again\n  ```\n",
        );
        assert_eq!(request.cells.len(), 3);
        let original = request.clone();
        let mut pids = Vec::new();
        for _ in 0..2 {
            let result = execute_page_with_environment(
                context(root.path()),
                &request,
                &environment(root.path()),
            )
            .await
            .unwrap();
            assert_eq!(
                result
                    .cells
                    .iter()
                    .map(|cell| cell.ordinal)
                    .collect::<Vec<_>>(),
                [0, 1, 2]
            );
            assert!(
                result
                    .cells
                    .iter()
                    .all(|cell| cell.outcome == CellOutcome::Ok)
            );
            assert_eq!(result.runtime.unwrap().language, "python");
            assert_eq!(result.kernel.unwrap().name, "fixture");
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedKernelMessage)
            );
            assert!(
                result
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.span.is_some())
            );
            pids.push(observation(root.path())["pid"].clone());
            assert_cleaned(root.path()).await;
        }
        assert_ne!(pids[0], pids[1]);
        assert_eq!(request, original);
        let submitted = submitted(root.path());
        assert_eq!(submitted.len(), 6);
        for (index, message) in submitted.iter().enumerate() {
            assert_eq!(message["code"], request.cells[index % 3].cell.source);
            assert_eq!(message["silent"], false);
            assert_eq!(message["store_history"], true);
            assert_eq!(message["allow_stdin"], false);
            assert_eq!(message["stop_on_error"], true);
            assert_eq!(message["user_expressions"], json!({}));
        }
    }
}

#[tokio::test]
async fn skipped_cells_do_not_submit_or_start_another_language() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-reply-first").await;
    let mut request = request(
        "```{r}\nwrong language\n```\n\n```{python}\nskip\n```\n\n```{python3}\ndefine\n```\n\n```{python}\nuse\n```\n",
    );
    request.cells[0].options.execution.eval.value = false;
    request.cells[1].options.execution.eval.value = false;
    request.cells[2].options.execution.echo.value = false;
    request.cells[2].options.execution.include.value = false;
    request.cells[2].options.execution.output.value = crate::execution::OutputVisibility::Hide;
    let result =
        execute_page_with_environment(context(root.path()), &request, &environment(root.path()))
            .await
            .unwrap();
    assert_eq!(
        result.cells[0].outcome,
        CellOutcome::Skipped {
            reason: CellSkipReason::LanguageMismatch
        }
    );
    assert_eq!(
        result.cells[1].outcome,
        CellOutcome::Skipped {
            reason: CellSkipReason::EvalFalse
        }
    );
    assert!(result.cells[..2].iter().all(|cell| cell.events.is_empty()));
    assert_eq!(submitted(root.path()).len(), 2);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn hidden_cells_execute_and_collect_output_using_prepared_options() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-streams").await;
    let request = request(
        "---\nexecute: {eval: false, echo: false, output: false}\n---\n\n```{python}\nskip\n```\n\n```{python, eval=true}\n#| include: false\ndefine\n```\n\n```{python, eval=true, output=asis}\nuse\n```\n",
    );
    let result =
        execute_page_with_environment(context(root.path()), &request, &environment(root.path()))
            .await
            .unwrap();
    assert_eq!(
        result.cells[0].outcome,
        CellOutcome::Skipped {
            reason: CellSkipReason::EvalFalse
        }
    );
    assert!(result.cells[0].events.is_empty());
    for cell in &result.cells[1..] {
        assert_eq!(cell.outcome, CellOutcome::Ok);
        assert!(matches!(cell.events.as_slice(), [
            CellEvent::Stream { name: crate::ir::StreamName::Stdout, text: stdout },
            CellEvent::Stream { name: crate::ir::StreamName::Stderr, text: stderr },
        ] if stdout == "# ordinary stdout\n" && stderr == "stderr\n"));
    }
    let submitted = submitted(root.path());
    assert_eq!(submitted.len(), 2);
    assert!(submitted.iter().all(|message| message["silent"] == false));
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn protocol_text_output_respects_prepared_asis_and_inert_markdown() {
    use crate::ir::{Block, CellOutputKind, OutputRepresentation, StreamName};

    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-markdown").await;
    let request = request("```{python}\n#| output: asis\ndefine\n```\n\n```{python}\nuse\n```\n");
    let result =
        execute_page_with_environment(context(root.path()), &request, &environment(root.path()))
            .await
            .unwrap();
    let mut reducer =
        OutputReducer::new(request.page.clone(), ErrorContext::new(root.path().into()));
    for (prepared, executed) in request.cells.iter().zip(result.cells) {
        reducer
            .accept_cell(
                prepared,
                executed.outcome,
                executed.events,
                &mut validate_text,
            )
            .unwrap();
    }
    let result = reducer.finish().unwrap();
    let asis = &result.cells[0].outputs;
    let ordinary = &result.cells[1].outputs;
    assert_eq!(asis.len(), 3);
    assert_eq!(ordinary.len(), 4);
    assert!(
        matches!(&asis[0].output.representations[0], OutputRepresentation::MarkdownBlocks { blocks, .. }
        if matches!(blocks.as_slice(), [Block::Heading { .. }]))
    );
    assert!(
        matches!(&ordinary[0].output.representations[0], OutputRepresentation::PlainText { text, .. } if text == "# Gener")
    );
    assert!(
        matches!(&ordinary[1].output.representations[0], OutputRepresentation::PlainText { text, .. } if text == "ated\n")
    );
    for outputs in [asis, ordinary] {
        let stderr = &outputs[outputs.len() - 2];
        assert_eq!(
            stderr.output.kind,
            CellOutputKind::Stream {
                stream: StreamName::Stderr
            }
        );
        let OutputRepresentation::PlainText { text, .. } = &stderr.output.representations[0] else {
            panic!()
        };
        assert_eq!(
            crate::rendering::render_preformatted_text(text),
            "<pre><code>&lt;stderr&gt;&amp;literal\n</code></pre>"
        );
        let display = outputs.last().unwrap();
        assert_eq!(display.selected_mime_type.as_deref(), Some("text/markdown"));
        assert!(
            matches!(&display.output.representations[0], OutputRepresentation::MarkdownBlocks { blocks, .. }
            if matches!(blocks.as_slice(), [Block::CodeBlock { source, .. }] if source == "raise RuntimeError('inert')\n"))
        );
        assert_eq!(display.output.representations.len(), 2);
        assert_eq!(display.output.provenance.len(), 1);
    }
    assert!(result.diagnostics.is_empty());
    assert_eq!(submitted(root.path()).len(), 2);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn hiding_output_does_not_override_prepared_error_policy() {
    for visibility in ["output: false", "include: false"] {
        for allow_error in [false, true] {
            let root = TempDir::new().unwrap();
            fixture_kernel(root.path(), "execute-both-errors").await;
            let request = request(&format!(
                "---\nexecute: {{error: {allow_error}}}\n---\n\n```{{python}}\n#| {visibility}\ndefine\n```\n\n```{{python}}\nuse\n```\n",
            ));
            let result = execute_page_with_environment(
                context(root.path()),
                &request,
                &environment(root.path()),
            )
            .await;
            if allow_error {
                let result = result.unwrap();
                assert_eq!(result.cells[0].outcome, CellOutcome::AllowedError);
                assert_eq!(result.cells[1].outcome, CellOutcome::Ok);
                assert!(matches!(
                    result.cells[0].events.as_slice(),
                    [CellEvent::Error { .. }]
                ));
                assert_eq!(submitted(root.path()).len(), 2);
            } else {
                assert_eq!(result.err().unwrap().kind, ExecutionFailureKind::CellError);
                assert_eq!(submitted(root.path()).len(), 1);
            }
            assert_cleaned(root.path()).await;
        }
    }
}

#[tokio::test]
async fn pages_without_candidates_do_not_launch() {
    for mode in ["empty", "eval-false", "mismatch", "veto", "never"] {
        let root = TempDir::new().unwrap();
        let mut request = two_cells();
        match mode {
            "empty" => request.cells.clear(),
            "eval-false" => request
                .cells
                .iter_mut()
                .for_each(|cell| cell.options.execution.eval.value = false),
            "mismatch" => {
                fixture_kernel(root.path(), "normal").await;
                request
                    .cells
                    .iter_mut()
                    .for_each(|cell| cell.cell.language = Some("r".into()));
            }
            "veto" => request.page.page_veto = true,
            "never" => request.page.mode = ExecutionMode::Never,
            _ => unreachable!(),
        }
        let result = execute_page_with_environment(
            context(root.path()),
            &request,
            &environment(root.path()),
        )
        .await;
        if matches!(mode, "veto" | "never") {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert!(result.runtime.is_none());
            assert!(
                result
                    .cells
                    .iter()
                    .all(|cell| matches!(cell.outcome, CellOutcome::Skipped { .. }))
            );
        }
        assert!(!root.path().join("observations.json").exists());
    }
}

#[tokio::test]
async fn failures_stop_submission_and_are_attributed_to_the_active_cell() {
    for (mode, expected) in [
        ("execute-error", ExecutionFailureKind::CellError),
        ("execute-iopub-error", ExecutionFailureKind::CellError),
        ("execute-aborted", ExecutionFailureKind::CellError),
        ("execute-malformed-reply", ExecutionFailureKind::Protocol),
        ("execute-stdin", ExecutionFailureKind::InputRequested),
        ("execute-exit", ExecutionFailureKind::Protocol),
        ("execute-wrong-reply", ExecutionFailureKind::Protocol),
        (
            "execute-no-terminal",
            ExecutionFailureKind::Timeout {
                phase: ExecutionPhase::Cell,
            },
        ),
        (
            "execute-no-idle",
            ExecutionFailureKind::Timeout {
                phase: ExecutionPhase::TerminalSync,
            },
        ),
        (
            "execute-no-reply",
            ExecutionFailureKind::Timeout {
                phase: ExecutionPhase::TerminalSync,
            },
        ),
    ] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        let mut context = context(root.path());
        context.deadlines.cell = 600;
        context.deadlines.terminal_sync = 150;
        let mut request = two_cells();
        if !matches!(mode, "execute-error" | "execute-iopub-error") {
            request.cells[0].options.execution.error.value = true;
        }
        let failure = execute_page_with_environment(context, &request, &environment(root.path()))
            .await
            .err()
            .unwrap();
        assert_eq!(failure.kind, expected, "{mode}: {failure:?}");
        assert_eq!(
            failure.diagnostics.last().unwrap().span,
            Some(request.cells[0].cell.span)
        );
        assert!(
            failure.cleanup_diagnostics.is_empty(),
            "{mode}: {failure:?}"
        );
        assert_eq!(submitted(root.path()).len(), 1, "{mode}");
        assert_cleaned(root.path()).await;
    }
}

#[tokio::test]
async fn ongoing_output_cannot_extend_cell_or_terminal_deadlines() {
    for (mode, phase) in [
        ("execute-chatty-no-terminal", ExecutionPhase::Cell),
        ("execute-chatty-no-idle", ExecutionPhase::TerminalSync),
        ("execute-chatty-no-reply", ExecutionPhase::TerminalSync),
    ] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), mode).await;
        let mut context = context(root.path());
        context.deadlines.cell = 500;
        context.deadlines.terminal_sync = 200;
        let session = start_session(kernel, &mut context, source()).await.unwrap();
        let failure = tokio::time::timeout(
            Duration::from_secs(3),
            session.execute(two_cells().cells, &mut context.cancellation),
        )
        .await
        .expect("Ongoing output must not extend the deadline or cleanup.")
        .unwrap_err();
        assert_eq!(
            failure.kind,
            ExecutionFailureKind::Timeout { phase },
            "{mode}"
        );
        assert!(failure.cleanup_diagnostics.is_empty(), "{failure:?}");
        assert_eq!(submitted(root.path()).len(), 1);
        assert_cleaned(root.path()).await;
        let events = std::fs::read_to_string(root.path().join("events")).unwrap();
        let events: Vec<_> = events.lines().collect();
        assert!(events.contains(&"activity"), "{mode}");
        let interrupt = events
            .iter()
            .position(|event| *event == "interrupt")
            .unwrap();
        let shutdown = events
            .iter()
            .position(|event| *event == "shutdown")
            .unwrap();
        assert!(interrupt < shutdown, "{mode}");
    }
}

#[tokio::test]
async fn the_remaining_cell_deadline_bounds_either_terminal_order() {
    for mode in ["execute-no-idle", "execute-no-reply"] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), mode).await;
        let mut context = context(root.path());
        context.deadlines.cell = 200;
        context.deadlines.terminal_sync = 1_000;
        let session = start_session(kernel, &mut context, source()).await.unwrap();
        let failure = session
            .execute(two_cells().cells, &mut context.cancellation)
            .await
            .unwrap_err();
        assert_eq!(
            failure.kind,
            ExecutionFailureKind::Timeout {
                phase: ExecutionPhase::Cell
            },
            "{mode}"
        );
        assert_eq!(submitted(root.path()).len(), 1);
        assert!(failure.cleanup_diagnostics.is_empty());
        assert_cleaned(root.path()).await;
    }
}

#[tokio::test]
async fn allowed_language_errors_keep_the_same_session_alive() {
    for mode in [
        "execute-error",
        "execute-iopub-error",
        "execute-both-errors",
    ] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), mode).await;
        let mut request = two_cells();
        request.cells[0].options.execution.error.value = true;
        let result = execute_page_with_environment(
            context(root.path()),
            &request,
            &environment(root.path()),
        )
        .await
        .unwrap();
        assert_eq!(result.cells[0].outcome, CellOutcome::AllowedError);
        assert_eq!(result.cells[1].outcome, CellOutcome::Ok);
        assert_eq!(
            result.cells[0]
                .events
                .iter()
                .filter(|event| matches!(event, CellEvent::Error { .. }))
                .count(),
            1
        );
        assert_eq!(submitted(root.path()).len(), 2);
        let mut outputs = OutputReducer::new(
            request.page.clone(),
            ErrorContext::new(root.path().to_path_buf()),
        );
        for (prepared, executed) in request.cells.iter().zip(result.cells) {
            outputs
                .accept_cell(
                    prepared,
                    executed.outcome,
                    executed.events,
                    &mut validate_text,
                )
                .unwrap();
        }
        let outputs = outputs.finish().unwrap();
        assert!(outputs.diagnostics.is_empty());
        assert_eq!(outputs.cells[0].outputs.len(), 1);
        assert!(matches!(
            outputs.cells[0].outputs[0].output.kind,
            crate::ir::CellOutputKind::Error { .. }
        ));
        assert_cleaned(root.path()).await;
    }
}

#[tokio::test]
async fn cancellation_during_cleanup_is_still_cancellation() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-slow-shutdown").await;
    let mut context = context(root.path());
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    context.cancellation = Box::pin(async {
        let _ = cancelled.await;
    });
    let environment = environment(root.path());
    let task = tokio::spawn(async move {
        execute_page_with_environment(context, &two_cells(), &environment).await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = std::fs::read_to_string(root.path().join("events")).unwrap_or_default();
            if events.contains("shutdown") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.send(()).unwrap();
    let failure = task.await.unwrap().err().unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn invalid_prepared_order_is_rejected_before_discovery() {
    for ordinal in [false, true] {
        let root = TempDir::new().unwrap();
        let mut request = two_cells();
        if ordinal {
            request.cells[0].ordinal = 1;
        } else {
            let first = request.cells[0].cell.clone();
            request.cells[0].cell = request.cells[1].cell.clone();
            request.cells[1].cell = first;
        }
        let failure = execute_page_with_environment(
            context(root.path()),
            &request,
            &environment(root.path()),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(failure.kind, ExecutionFailureKind::Startup);
        assert!(failure.diagnostics[0].message.contains("authored order"));
        assert!(!root.path().join("observations.json").exists());
    }
}

async fn wait_for_submission(root: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.join("requests").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancellation_and_dropping_an_executing_page_reap_the_kernel() {
    for abort in [false, true] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-no-terminal").await;
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let mut context = context(root.path());
        context.cancellation = Box::pin(async {
            let _ = cancelled.await;
        });
        let environment = environment(root.path());
        let task = tokio::spawn(async move {
            execute_page_with_environment(context, &two_cells(), &environment).await
        });
        wait_for_submission(root.path()).await;
        if abort {
            task.abort();
            assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        } else {
            cancel.send(()).unwrap();
            assert_eq!(
                task.await.unwrap().err().unwrap().kind,
                ExecutionFailureKind::Cancelled
            );
        }
        assert_cleaned(root.path()).await;
        assert_eq!(submitted(root.path()).len(), 1);
        assert!(
            std::fs::read_to_string(root.path().join("events"))
                .unwrap()
                .contains("interrupt")
        );
    }
}

#[tokio::test]
async fn declared_python_and_r_retain_definitions_and_imports_only_within_a_page() {
    let cases = [
        (
            "python3",
            "```{python}\nassert 'page_function' not in globals()\nimport math\ndef page_function(x):\n    return math.sqrt(x)\n```\n\n```{python}\nassert page_function(144) == 12\npage_value = page_function(81)\n```\n\n```{python}\nassert page_value == 9\nprint(int(page_value))\n```\n",
            "9\n",
        ),
        (
            "ir",
            "```{r}\nstopifnot(!exists('page_function'))\nlibrary(splines)\npage_function <- function(x) ns(x, df = 2)\n```\n\n```{r}\nstopifnot('package:splines' %in% search())\npage_value <- page_function(1:5)\n```\n\n```{r}\nstopifnot(identical(dim(page_value), c(5L, 2L)))\ncat(nrow(page_value), '\\n', sep = '')\n```\n",
            "5\n",
        ),
    ];
    for (kernel, authored, expected) in cases {
        let root = TempDir::new().unwrap();
        let mut request = request(authored);
        request.kernel = kernel.into();
        for _ in 0..2 {
            let mut context = context(root.path());
            context.deadlines = ExecutionDeadlines::default();
            let result = execute_page(context, &request).await.unwrap();
            assert_eq!(result.cells.len(), 3);
            assert!(
                result
                    .cells
                    .iter()
                    .all(|cell| cell.outcome == CellOutcome::Ok)
            );
            assert!(result.runtime.is_some());
            let stdout = result.cells[2]
                .events
                .iter()
                .filter_map(|event| match event {
                    CellEvent::Stream {
                        name: crate::ir::StreamName::Stdout,
                        text,
                    } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(stdout, expected);
        }
    }
}
