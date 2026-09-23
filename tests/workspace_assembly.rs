mod support;

use std::future::pending;
use std::path::Path;

use diplodocus::assembly::{AssemblyError, WorkspaceExecution, assemble_workspace};
use diplodocus::diagnostics::DiagnosticCode;
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::ir::{Block, PackageReference};

fn simple_workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Assembly'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount='guide'\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n");
    root.write("guide/first.qmd", "# First\n\n```{python}\nfrom IPython.display import SVG, display\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\"))\n```\n");
    root
}

#[test]
fn assembly_combines_declared_packages_pages_concepts_and_evidence_portably() {
    let first = support::acceptance_workspace();
    let second = support::acceptance_workspace();
    let a = assemble_workspace(first.path().join("workspace/diplodocus.toml")).unwrap();
    let b = assemble_workspace(second.path().join("workspace/diplodocus.toml")).unwrap();
    assert_eq!(a.workspace(), b.workspace());
    let workspace = a.workspace();
    assert_eq!(workspace.packages.len(), 2);
    assert_eq!(workspace.packages["pyfoo"].items.len(), 26);
    assert_eq!(
        workspace.packages["pyfoo"].version.as_deref(),
        Some("1.9.0")
    );
    assert!(!workspace.packages["rfoo"].items.is_empty());
    assert_eq!(workspace.content_collections.len(), 5);
    assert_eq!(workspace.concepts.len(), 2);
    assert_eq!(workspace.concepts["fit"].members.len(), 2);
    assert!(
        matches!(&workspace.relationships[0].to, PackageReference::External { ecosystem, name } if ecosystem == "cargo" && name == "foo-core")
    );
    assert!(
        workspace
            .repositories
            .values()
            .all(|r| r.declared_input_fingerprint.is_some())
    );
    assert!(
        a.prepared_pages()
            .values()
            .any(|p| p.preparation().is_some_and(|p| p.execution_eligible))
    );
    assert!(
        !serde_json::to_string(workspace)
            .unwrap()
            .contains(first.path().to_str().unwrap())
    );
    assert!(!first.path().join("workspace/.diplodocus").exists());
}

#[test]
fn overlapping_targets_fail_instead_of_selecting_one_fragment() {
    let root = support::acceptance_workspace();
    let config = root.read("workspace/diplodocus.toml").replace(
        "{ id = \"api\", extractor = \"python\", path = \"python/foo\", role = \"public-api\" },",
        "{ id = \"api\", extractor = \"python\", path = \"python/foo\", role = \"public-api\" },\n  { id = \"overlap\", extractor = \"python\", path = \"python/foo\", role = \"public-api\" },",
    );
    root.write("workspace/diplodocus.toml", config);
    let AssemblyError::Diagnostics(diagnostics) =
        assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap_err()
    else {
        panic!("expected assembly diagnostics")
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingExtraction)
    );
}

#[tokio::test]
async fn execution_projects_typed_outputs_and_staging_is_owned_by_the_workspace() {
    let root = simple_workspace();
    let source = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let result = source.execute(settings(stage.path())).await.unwrap();
    assert_eq!(result.executed_pages().len(), 1);
    let page = result.workspace().pages.values().next().unwrap();
    let Block::CodeCell(cell) = &page.document.document.blocks[1] else {
        panic!("cell")
    };
    assert!(!cell.outputs.is_empty());
    let asset = result
        .executed_pages()
        .values()
        .next()
        .unwrap()
        .staged_assets()[0]
        .path
        .clone();
    assert!(asset.exists());
    drop(result);
    assert!(!asset.exists());
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn acceptance_workspace_executes_only_the_four_authorized_python_and_r_pages() {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let result = sources.execute(settings(stage.path())).await.unwrap();
    assert_eq!(result.executed_pages().len(), 4);
    for page in result.executed_pages().values() {
        let record = page.validated().record();
        assert!(record.provenance.is_some());
        assert!(matches!(
            record.page.collection.as_str(),
            "python-execution" | "r-execution"
        ));
        assert!(
            !record
                .provenance
                .as_ref()
                .unwrap()
                .execution
                .tools
                .is_empty()
        );
    }
    let output = serde_json::to_string(result.workspace()).unwrap();
    assert!(output.contains("Python total: 12"));
    assert!(output.contains("R total: 12"));
    assert!(!output.contains(root.path().to_str().unwrap()));
    result.discard().unwrap();
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_later_execution_failure_discards_earlier_successful_assets() {
    let root = simple_workspace();
    root.write(
        "guide/second.qmd",
        "```{python}\nraise RuntimeError('later failure')\n```\n",
    );
    let source = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let failure = source.execute(settings(stage.path())).await.unwrap_err();
    assert!(matches!(failure, AssemblyError::Execution(_)));
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_later_page_cannot_change_an_already_executed_input() {
    let root = simple_workspace();
    root.write("guide/second.qmd", "```{python}\nfrom pathlib import Path\nPath('first.qmd').write_text('changed by the second page')\n```\n");
    let source = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let failure = source.execute(settings(stage.path())).await.unwrap_err();
    assert!(
        matches!(failure, AssemblyError::InputsChanged),
        "{failure:?}"
    );
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

fn settings(parent: &Path) -> WorkspaceExecution<'_> {
    WorkspaceExecution {
        staging_parent: parent,
        deadlines: ExecutionDeadlines::default(),
        cancellation: Box::pin(pending()),
    }
}

#[cfg(unix)]
#[test]
fn configuration_symlink_keeps_the_callers_configuration_directory() {
    let root = simple_workspace();
    root.write("config/settings.toml", root.read("diplodocus.toml"));
    root.remove("diplodocus.toml");
    std::os::unix::fs::symlink("config/settings.toml", root.path().join("diplodocus.toml"))
        .unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert_eq!(sources.paths().configuration_directory, root.path());
    assert_eq!(sources.workspace().pages.len(), 1);
    sources.revalidate().unwrap();
}

#[tokio::test]
async fn disabled_vetoed_and_candidate_free_pages_never_discover_or_stage() {
    for case in 0..3 {
        let root = simple_workspace();
        let config = root.read("diplodocus.toml");
        root.write(
            "diplodocus.toml",
            if case == 0 {
                config.replace(
                    "mode='execute'\nengine='jupyter'\nkernel='python3'",
                    "mode='never'",
                )
            } else {
                config.replace("kernel='python3'", "kernel='not-installed'")
            },
        );
        let text = if case == 1 {
            "---\nexecute: false\n---\n```{python}\nraise Exception('inert')\n```\n"
        } else if case == 2 {
            "```{python}\n#| eval: false\nraise Exception('inert')\n```\n"
        } else {
            "```{python}\nraise Exception('inert')\n```\n"
        };
        root.write("guide/first.qmd", text);
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let missing = root.path().join("must-not-be-created");
        let result = sources.execute(settings(&missing)).await.unwrap();
        assert!(result.executed_pages().is_empty());
        assert!(!missing.exists());
        assert!(!root.path().join(".diplodocus").exists());
    }
}

#[test]
fn invalid_static_inputs_and_unknown_concept_members_stop_assembly() {
    let root = support::acceptance_workspace();
    root.write(
        "workspace/diplodocus.toml",
        root.read("workspace/diplodocus.toml")
            .replace("item = \"foo.fit\"", "item = \"foo.missing\""),
    );
    let AssemblyError::Diagnostics(diagnostics) =
        assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap_err()
    else {
        panic!("diagnostics")
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedItemReference)
    );
    let root = simple_workspace();
    root.write("guide/invalid.qmd", "---\njupyter: python3\n---\n");
    assert!(matches!(
        assemble_workspace(root.path().join("diplodocus.toml")),
        Err(AssemblyError::Diagnostics(_))
    ));
    assert!(!root.path().join(".diplodocus").exists());
}

#[tokio::test]
async fn cancellation_before_a_no_execution_attempt_prevents_success() {
    let root = simple_workspace();
    root.write("guide/first.qmd", "No executable cells.\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let missing = root.path().join("must-not-be-created");
    let mut options = settings(&missing);
    options.cancellation = Box::pin(std::future::ready(()));
    assert!(matches!(
        sources.execute(options).await,
        Err(AssemblyError::Cancelled)
    ));
    assert!(!missing.exists());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_and_drop_discard_prior_pages_and_reap_the_active_kernel() {
    for drop_future in [false, true] {
        let root = simple_workspace();
        root.write("guide/second.qmd", "```{python}\nimport os, time\nfrom pathlib import Path\nPath('../active-pid').write_text(str(os.getpid()))\nwhile True: time.sleep(1)\n```\n");
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let stage = tempfile::tempdir().unwrap();
        let staging_path = stage.path().to_owned();
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut options = settings(&staging_path);
            options.cancellation = Box::pin(async {
                let _ = cancelled.await;
            });
            sources.execute(options).await
        });
        let pid_path = root.path().join("active-pid");
        let pid = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&pid_path)
                    && let Ok(pid) = text.parse::<i32>()
                {
                    break pid;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(!support::files_under(stage.path()).is_empty());
        if drop_future {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            cancel.send(()).unwrap();
            assert!(matches!(
                task.await.unwrap(),
                Err(AssemblyError::Execution(_))
            ));
        }
        let pid = rustix::process::Pid::from_raw(pid).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while rustix::process::test_kill_process(pid) != Err(rustix::io::Errno::SRCH) {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
    }
}
