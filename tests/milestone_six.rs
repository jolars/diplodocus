#![cfg(target_os = "linux")]
mod support;

use std::collections::BTreeMap;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::time::Duration;

use diplodocus::configuration::ExecutionMode;
use diplodocus::diagnostics::DiagnosticCode;
use diplodocus::documents::{AuthoredFormat, prepare_collection_document};
use diplodocus::execution::{
    CellOutcome, CellSkipReason, ExecutionContext, ExecutionDeadlines, ExecutionEngine,
    ExecutionFailureKind, ExecutionPage, ExecutionPhase, JupyterEngine, PageExecutionRequest,
};
use diplodocus::ir::{ExecutionOrigin, OutputRepresentation, ProvenanceActivity, SourceLocation};
use diplodocus::provenance::{PANACHE_VERSION, fingerprint_bytes};
use diplodocus::snapshots::Snapshot;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum Kernel {
    Python,
    R,
}

impl Kernel {
    fn name(self) -> &'static str {
        match self {
            Self::Python => "python3",
            Self::R => "ir",
        }
    }

    fn language(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::R => "r",
        }
    }

    fn setup(self) -> &'static str {
        match self {
            Self::Python => {
                "import os\nfrom pathlib import Path\nfrom ipykernel.connect import get_connection_file\nfrom IPython.display import display\nPath('process.txt').write_text(f'{os.getpid()}\\n{get_connection_file()}\\n')\ndisplay({'image/svg+xml': '<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"3\"/></svg>'}, raw=True)"
            }
            Self::R => {
                "writeLines(c(as.character(Sys.getpid()), commandArgs(trailingOnly = TRUE)[1]), 'process.txt')\nIRdisplay::display_svg('<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"3\"/></svg>')"
            }
        }
    }

    fn hang(self) -> &'static str {
        match self {
            Self::Python => {
                "import time\ntry:\n    Path('ready').write_text('ready')\n    time.sleep(120)\nexcept KeyboardInterrupt:\n    Path('interrupted').write_text('yes')\n    raise"
            }
            Self::R => {
                "tryCatch({writeLines('ready', 'ready'); Sys.sleep(120)}, interrupt = function(e) {writeLines('yes', 'interrupted'); stop(e)})"
            }
        }
    }

    fn fail(self) -> &'static str {
        match self {
            Self::Python => "raise RuntimeError('stop here')",
            Self::R => "stop('stop here')",
        }
    }

    fn next(self) -> &'static str {
        match self {
            Self::Python => "Path('next-cell').write_text('must not run')",
            Self::R => "writeLines('must not run', 'next-cell')",
        }
    }

    fn page(self, middle: &str) -> String {
        let language = self.language();
        format!(
            "# Failure boundary\n\n```{{{language}}}\n{}\n```\n\n```{{{language}}}\n{middle}\n```\n\n```{{{language}}}\n{}\n```\n",
            self.setup(),
            self.next()
        )
    }
}

fn configuration(kernel: &str) -> String {
    format!(
        "[project]\nname='Execution acceptance'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='{kernel}'\n"
    )
}

fn process(root: &Path) -> (rustix::process::Pid, PathBuf) {
    let text = std::fs::read_to_string(root.join("guide/process.txt")).unwrap();
    let mut lines = text.lines();
    let pid = rustix::process::Pid::from_raw(lines.next().unwrap().parse().unwrap()).unwrap();
    let connection = PathBuf::from(lines.next().unwrap());
    assert!(
        connection.is_absolute(),
        "kernel must report its connection file"
    );
    (pid, connection)
}

fn assert_reaped(root: &Path) {
    let (pid, connection) = process(root);
    assert_eq!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        !connection.parent().unwrap().exists(),
        "private connection directory survived"
    );
}

fn mark_executed(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("kind") == Some(&json!("execution")) && fields.contains_key("origin") {
                fields.insert("origin".into(), json!("executed"));
            }
            fields.values_mut().for_each(mark_executed);
        }
        Value::Array(values) => values.iter_mut().for_each(mark_executed),
        _ => {}
    }
}

fn build(root: &Path, output: &str) {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["build", "--config", "diplodocus.toml", "--output", output])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn python_and_r_cli_builds_match_reviewed_outputs_and_restore_complete_results() {
    for kernel in [Kernel::Python, Kernel::R] {
        let root = support::TestWorkspace::new();
        root.write("diplodocus.toml", configuration(kernel.name()));
        root.write(
            "guide/index.qmd",
            support::load_fixture(format!("execution-kernels/{}.qmd", kernel.language())),
        );
        build(root.path(), "fresh");
        assert_reaped(root.path());
        let database = root.path().join(".diplodocus/documentation.sqlite");
        let fresh = Snapshot::load(&database).unwrap();
        let id = fresh.workspace().pages.keys().next().unwrap();
        let record = fresh.executed_page(id).unwrap().record();
        assert_eq!(record.cells.len(), 6);
        assert_eq!(record.cells[3].outcome, CellOutcome::AllowedError);
        assert_eq!(record.cells[4].outcome, CellOutcome::Ok);
        assert_eq!(
            record.cells[5].outcome,
            CellOutcome::Skipped {
                reason: CellSkipReason::EvalFalse
            }
        );
        assert_eq!(
            record
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>(),
            [DiagnosticCode::UnsupportedCellOutput]
        );
        assert!(
            record.cells[2].outputs[0]
                .unsupported_placeholder()
                .is_some()
        );
        assert_eq!(
            record.cells[2].outputs[1].selected_mime_type.as_deref(),
            Some("text/plain")
        );
        assert_eq!(record.assets.len(), 1);
        assert!(matches!(
            record.provenance.as_ref().unwrap().execution.activity,
            ProvenanceActivity::Execution {
                origin: ExecutionOrigin::Executed,
                ..
            }
        ));
        // Verify build versions before making the output oracle independent of releases.
        let mut cells = record.cells.clone();
        for cell in &mut cells {
            for output in &mut cell.outputs {
                for provenance in &mut output.output.provenance {
                    support::normalize_build_tool_versions(&mut provenance.tools, &["diplodocus"]);
                }
                for representation in &mut output.output.representations {
                    if let OutputRepresentation::HtmlCandidate { sanitizer, .. } = representation {
                        assert_eq!(sanitizer.name, "diplodocus-html-sanitizer");
                        assert_eq!(sanitizer.version, env!("CARGO_PKG_VERSION"));
                        sanitizer.version = "[DIPLODOCUS_VERSION]".into();
                    }
                }
            }
        }
        support::assert_json_golden(
            &json!({"cells":cells, "diagnostics":record.diagnostics, "assets":record.assets}),
            format!("milestone-six/{}.json", kernel.language()),
        );
        let html = root.read("fresh/index.html");
        assert!(html.contains("<strong>safe html</strong>"));
        assert!(html.contains("safe fallback"));
        assert!(!html.contains("rejected payload"));
        build(root.path(), "cached");
        let cached = Snapshot::load(&database).unwrap();
        let restored = cached.executed_page(id).unwrap().record();
        assert!(matches!(
            restored.provenance.as_ref().unwrap().execution.activity,
            ProvenanceActivity::Execution {
                origin: ExecutionOrigin::Cache,
                ..
            }
        ));
        let mut restored = serde_json::to_value(restored).unwrap();
        mark_executed(&mut restored);
        assert_eq!(restored, serde_json::to_value(record).unwrap());
        assert_eq!(cached.assets(), fresh.assets());
        assert_eq!(root.read("guide/runs.txt"), "executed\n");
        support::assert_output_tree(&root.path().join("fresh"), &root.path().join("cached"));
    }
}

fn request(
    root: &Path,
    kernel: Kernel,
    source: &str,
) -> (ExecutionContext<'static>, PageExecutionRequest) {
    let config = toml::from_str(&format!("id='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='{}'\n", kernel.name())).unwrap();
    let prepared = prepare_collection_document(source, &config)
        .unwrap()
        .preparation
        .unwrap();
    let context = ExecutionContext {
        repository_root: root.to_owned(),
        page_path: root.join("guide/index.qmd"),
        asset_staging_directory: root.join("staging"),
        deadlines: ExecutionDeadlines::default(),
        cancellation: Box::pin(pending()),
    };
    let request = PageExecutionRequest {
        page: ExecutionPage {
            source: SourceLocation {
                repository: "docs".into(),
                path: "guide/index.qmd".try_into().unwrap(),
                span: None,
            },
            collection: "guide".into(),
            working_directory: Some("guide".try_into().unwrap()),
            source_fingerprint: fingerprint_bytes(source.as_bytes()),
            format: AuthoredFormat::Qmd,
            mode: ExecutionMode::Execute,
            page_veto: false,
            parser_version: PANACHE_VERSION.into(),
            qmd_policy: "qmd-mvp-v1".into(),
        },
        kernel: kernel.name().into(),
        defaults: prepared.defaults,
        cells: prepared.cells,
        declared_environment_inputs: vec![],
    };
    (context, request)
}

async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(45), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("execution observation deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_and_r_timeouts_cancellation_and_drop_interrupt_and_discard_partial_assets() {
    for kernel in [Kernel::Python, Kernel::R] {
        for action in ["timeout", "cancel", "drop"] {
            let root = support::TestWorkspace::new();
            let source = kernel.page(kernel.hang());
            root.write("guide/index.qmd", &source);
            let (mut context, request) = request(root.path(), kernel, &source);
            let active_span = request.cells[1].cell.span;
            if action == "timeout" {
                context.deadlines.cell = 3_000;
            }
            let (cancel, cancelled) = tokio::sync::oneshot::channel();
            context.cancellation = Box::pin(async {
                let _ = cancelled.await;
            });
            let engine = JupyterEngine::new(
                BTreeMap::from([("docs".into(), root.path().to_owned())]),
                vec![],
            )
            .with_cache_root(root.path().join("cache"));
            let mut tasks = tokio::task::JoinSet::new();
            tasks.spawn(async move { engine.execute_page(context, &request).await });
            until(|| root.path().join("guide/ready").exists()).await;
            assert_eq!(support::files_under(&root.path().join("staging")).len(), 1);
            if action == "drop" {
                tasks.abort_all();
                assert!(tasks.join_next().await.unwrap().unwrap_err().is_cancelled());
                until(|| {
                    let (pid, connection) = process(root.path());
                    rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
                        && !connection.parent().unwrap().exists()
                })
                .await;
            } else {
                if action == "cancel" {
                    cancel.send(()).unwrap();
                }
                let failure = tokio::time::timeout(Duration::from_secs(35), tasks.join_next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .unwrap_err();
                let expected = if action == "timeout" {
                    ExecutionFailureKind::Timeout {
                        phase: ExecutionPhase::Cell,
                    }
                } else {
                    ExecutionFailureKind::Cancelled
                };
                assert_eq!(failure.kind, expected, "{kernel:?} {action}");
                assert!(failure.cleanup_diagnostics.is_empty(), "{failure:?}");
                assert_eq!(failure.diagnostics.last().unwrap().span, Some(active_span));
            }
            assert_reaped(root.path());
            assert_eq!(
                root.read("guide/interrupted").trim(),
                "yes",
                "{kernel:?} {action}"
            );
            assert!(!root.path().join("guide/next-cell").exists());
            assert!(support::files_under(&root.path().join("staging")).is_empty());
            if root.path().join("cache").exists() {
                assert!(support::files_under(&root.path().join("cache")).is_empty());
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_and_r_cell_errors_reap_the_kernel_and_discard_earlier_figures() {
    for kernel in [Kernel::Python, Kernel::R] {
        let root = support::TestWorkspace::new();
        let source = kernel.page(kernel.fail());
        root.write("guide/index.qmd", &source);
        let (context, request) = request(root.path(), kernel, &source);
        let engine = JupyterEngine::new(
            BTreeMap::from([("docs".into(), root.path().to_owned())]),
            vec![],
        )
        .with_cache_root(root.path().join("cache"));
        let failure = engine.execute_page(context, &request).await.unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::CellError);
        assert!(failure.cleanup_diagnostics.is_empty());
        assert_eq!(
            failure.diagnostics.last().unwrap().span,
            Some(request.cells[1].cell.span)
        );
        assert_reaped(root.path());
        assert!(!root.path().join("guide/next-cell").exists());
        assert!(support::files_under(&root.path().join("staging")).is_empty());
        if root.path().join("cache").exists() {
            assert!(support::files_under(&root.path().join("cache")).is_empty());
        }
    }
}

#[test]
fn missing_kernel_fails_cli_builds_without_publication() {
    for kernel in [Kernel::Python, Kernel::R] {
        let root = support::TestWorkspace::new();
        root.write("guide/index.qmd", kernel.page(kernel.fail()));
        root.write(
            "diplodocus.toml",
            configuration("diplodocus-absent-acceptance-kernel"),
        );
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .args(["build", "--config", "diplodocus.toml", "--output", "site"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(
            stderr.contains("execution-startup-failed") && stderr.contains("not found"),
            "{stderr}"
        );
        for path in [
            "guide/process.txt",
            "guide/next-cell",
            "site",
            ".diplodocus",
        ] {
            assert!(!root.path().join(path).exists(), "unexpected {path}");
        }
    }
}
