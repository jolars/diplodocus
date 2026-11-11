#![cfg(target_os = "linux")]
mod support;

use diplodocus::assembly::{AssemblyError, WorkspaceExecution, assemble_workspace};
use diplodocus::commands::{self, BuildOptions, GenerateOptions};
use diplodocus::diagnostics::{DiagnosticCode, DiagnosticSource};
use diplodocus::execution::{CellOutcome, ExecutionDeadlines, ExecutionFailureKind};
use diplodocus::ir::{ExecutionOrigin, ProvenanceActivity};
use diplodocus::snapshots::Snapshot;

fn workspace(kernel: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Executable includes\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: pages/index.qmd\n    mount: ''\n    format: qmd\n    execution:\n      mode: execute\n      engine: jupyter\n      kernel: {kernel}\n"));
    root
}

fn build(root: &support::TestWorkspace, output: &std::path::Path) -> Snapshot {
    commands::build(BuildOptions {
        config: root.path().join("diplodocus.yaml"),
        output: output.to_owned(),
    })
    .unwrap();
    Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
}

fn origin(snapshot: &Snapshot) -> ExecutionOrigin {
    let id = snapshot.workspace().pages.keys().next().unwrap();
    let ProvenanceActivity::Execution { origin, .. } = snapshot
        .executed_page(id)
        .unwrap()
        .record()
        .provenance
        .as_ref()
        .unwrap()
        .execution
        .activity
    else {
        panic!("expected execution provenance");
    };
    origin
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn nested_python_includes_share_state_invalidate_cache_and_generate_without_sources() {
    let root = workspace("python3");
    let main = "---\nexecute: {echo: false}\n---\n\n# Guide\n\n```{python}\nfrom pathlib import Path\nruns = Path('runs.txt')\nruns.write_text(runs.read_text() + 'run\\n' if runs.exists() else 'run\\n')\nvalue = 0\n```\n\n{{< include /parts/setup.qmd >}}\n\n{{< include /parts/setup.qmd >}}\n\n{{< include /parts/empty.md >}}\n\n```{python}\nprint(f'Final value: {value}')\n```\n";
    root.write("pages/index.qmd", main);
    root.write(
        "pages/parts/setup.qmd",
        "Shared setup.\n\n{{< include ../../shared/step.md >}}\n",
    );
    let partial = "Step.\n\n[Download](data.txt)\n\n![Included figure](figure.svg)\n\n```{python}\nvalue += 1\nprint(value)\n```\n";
    root.write("shared/step.md", partial);
    root.write("shared/data.txt", "Download from the partial.\n");
    root.write("shared/figure.svg", "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='10' height='10'/></svg>");
    root.write("pages/parts/empty.md", "");
    root.write("pages/parts/other-empty.md", "");
    let output = tempfile::tempdir().unwrap();
    let first = build(&root, &output.path().join("first"));
    assert_eq!(origin(&first), ExecutionOrigin::Executed);
    assert_eq!(root.read("pages/runs.txt"), "run\n");
    let id = first.workspace().pages.keys().next().unwrap();
    let record = first.executed_page(id).unwrap().record();
    assert_eq!(record.cells.len(), 4);
    assert!(
        record
            .cells
            .iter()
            .all(|cell| !cell.options.execution.echo.value)
    );
    assert!(
        std::fs::read_to_string(output.path().join("first/index.html"))
            .unwrap()
            .contains("Final value: 2")
    );
    let second = build(&root, &output.path().join("cached"));
    assert_eq!(origin(&second), ExecutionOrigin::Cache);
    assert_eq!(root.read("pages/runs.txt"), "run\n");
    support::assert_output_tree(&output.path().join("first"), &output.path().join("cached"));

    root.write("shared/step.md", partial.replace("Step.", "Edited prose."));
    let third = build(&root, &output.path().join("edited"));
    assert_eq!(origin(&third), ExecutionOrigin::Executed);
    assert_eq!(root.read("pages/runs.txt"), "run\nrun\n");

    // An empty include's identity matters even when expanded bytes do not change.
    root.write(
        "pages/index.qmd",
        main.replace("/parts/empty.md", "/parts/other-empty.md"),
    );
    let fourth = build(&root, &output.path().join("retargeted"));
    assert_eq!(origin(&fourth), ExecutionOrigin::Executed);
    assert_eq!(root.read("pages/runs.txt"), "run\nrun\nrun\n");
    assert_eq!(
        third
            .executed_page(id)
            .unwrap()
            .record()
            .page
            .source_fingerprint,
        fourth
            .executed_page(id)
            .unwrap()
            .record()
            .page
            .source_fingerprint,
    );
    let cached = build(&root, &output.path().join("retargeted-cache"));
    assert_eq!(origin(&cached), ExecutionOrigin::Cache);
    assert_eq!(root.read("pages/runs.txt"), "run\nrun\nrun\n");

    let relocated = workspace("python3");
    for directory in ["pages", "shared", ".diplodocus/cache"] {
        copy_tree(
            &root.path().join(directory),
            &relocated.path().join(directory),
        );
    }
    std::fs::remove_file(relocated.path().join("pages/runs.txt")).unwrap();
    let relocated_snapshot = build(&relocated, &output.path().join("relocated"));
    assert_eq!(origin(&relocated_snapshot), ExecutionOrigin::Cache);
    assert!(!relocated.path().join("pages/runs.txt").exists());
    support::assert_output_tree(
        &output.path().join("retargeted-cache"),
        &output.path().join("relocated"),
    );

    let input = output.path().join("portable.sqlite");
    std::fs::copy(root.path().join(".diplodocus/documentation.sqlite"), &input).unwrap();
    let source_path = root.path().to_owned();
    drop(root);
    assert!(!source_path.exists());
    let loaded = Snapshot::load(&input).unwrap();
    assert_eq!(loaded.documents(), cached.documents());
    commands::generate(GenerateOptions {
        input,
        output: output.path().join("portable"),
    })
    .unwrap();
    support::assert_output_tree(
        &output.path().join("retargeted-cache"),
        &output.path().join("portable"),
    );
}

#[test]
fn included_r_cells_keep_page_options_and_share_the_main_working_directory() {
    let root = workspace("ir");
    root.write("pages/index.qmd", "---\nexecute: {echo: false}\n---\n\n# R guide\n\n```{r}\ncat('run\\n', file = 'runs.txt', append = TRUE)\nvalue <- 40\n```\n\n{{< include ../shared/first.qmd >}}\n\n```{r}\ncat('Final value:', value, '\\n')\n```\n");
    root.write(
        "shared/first.qmd",
        "```{r}\nvalue <- value + 2\n```\n\n{{< include second.md >}}\n",
    );
    root.write("shared/second.md", "```{r}\n#| eval: false\nstop('must not run')\n```\n\n```{r}\n#| error: true\nstop('allowed included error')\n```\n");
    let output = tempfile::tempdir().unwrap();
    let first = build(&root, &output.path().join("first"));
    assert_eq!(origin(&first), ExecutionOrigin::Executed);
    let id = first.workspace().pages.keys().next().unwrap();
    let record = first.executed_page(id).unwrap().record();
    assert!(matches!(
        record.cells[2].outcome,
        CellOutcome::Skipped { .. }
    ));
    assert_eq!(record.cells[3].outcome, CellOutcome::AllowedError);
    assert!(
        record
            .cells
            .iter()
            .all(|cell| !cell.options.execution.echo.value)
    );
    assert!(root.path().join("pages/runs.txt").exists());
    assert!(!root.path().join("shared/runs.txt").exists());
    let cached = build(&root, &output.path().join("cached"));
    assert_eq!(origin(&cached), ExecutionOrigin::Cache);
    assert_eq!(root.read("pages/runs.txt"), "run\n");
}

#[test]
fn execution_warnings_keep_original_locations_on_execution_and_cache_restore() {
    let root = workspace("python3");
    root.write(
        "pages/index.qmd",
        "# Main\n\n{{< include ../shared/warnings.md >}}\n",
    );
    let partial = "Warnings.\n\n```{python}\nfrom IPython.display import display\ndisplay({'text/html': '<script>unsafe()</script>', 'text/plain': 'fallback'}, raw=True)\ndisplay({'text/markdown': '<div>inert</div>'}, raw=True)\n```\n";
    root.write("shared/warnings.md", partial);
    let output = tempfile::tempdir().unwrap();
    let first = build(&root, &output.path().join("first"));
    let cached = build(&root, &output.path().join("cached"));
    assert_eq!(origin(&cached), ExecutionOrigin::Cache);
    assert_eq!(
        first.workspace().diagnostics,
        cached.workspace().diagnostics
    );
    let warning = first
        .workspace()
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnsafeKernelHtml)
        .unwrap();
    assert!(
        matches!(&warning.source, Some(DiagnosticSource::Repository { path, .. }) if path.as_str() == "shared/warnings.md")
    );
    let span = warning.span.unwrap();
    assert!(partial[span.start..span.end].starts_with("```{python}"));
    assert!(warning.message.contains("Included from pages/index.qmd"));
    let fragment = first
        .workspace()
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::UnsupportedAuthoredSyntax)
        .unwrap();
    assert!(fragment.source.is_none());
    assert!(!fragment.message.contains("Included from"));
}

#[tokio::test]
async fn execution_failures_identify_the_nested_partial_and_include_chain() {
    let root = workspace("python3");
    root.write(
        "pages/index.qmd",
        "# Main\n\n{{< include ../shared/first.qmd >}}\n",
    );
    root.write("shared/first.qmd", "First.\n\n{{< include nested.md >}}\n");
    let partial = "Nested.\n\n```{python}\nraise RuntimeError('included failure')\n```\n";
    root.write("shared/nested.md", partial);
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let staging = tempfile::tempdir().unwrap();
    let error = sources
        .execute(WorkspaceExecution {
            staging_parent: staging.path(),
            deadlines: ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        })
        .await
        .unwrap_err();
    let AssemblyError::Execution(failure) = error else {
        panic!("{error:?}");
    };
    assert_eq!(failure.kind, ExecutionFailureKind::CellError);
    let diagnostic = failure
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::ExecutionCellFailed)
        .unwrap();
    assert!(
        matches!(&diagnostic.source, Some(DiagnosticSource::Repository { path, .. }) if path.as_str() == "shared/nested.md")
    );
    let span = diagnostic.span.unwrap();
    assert!(partial[span.start..span.end].starts_with("```{python}"));
    assert!(diagnostic.message.contains("Included from pages/index.qmd"));
    assert!(
        diagnostic
            .message
            .contains("Included from shared/first.qmd")
    );
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_later_page_cannot_change_an_earlier_pages_included_input() {
    let root = workspace("python3");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml")
            .replace("path: pages/index.qmd", "path: pages"),
    );
    root.write(
        "pages/first.qmd",
        "# First\n\n{{< include ../shared/cell.md >}}\n",
    );
    root.write(
        "shared/cell.md",
        "```{python}\nprint('earlier page')\n```\n",
    );
    root.write("pages/last.qmd", "# Last\n\n```{python}\nfrom pathlib import Path\nPath('../shared/cell.md').write_text('Changed by a later page.\\n')\n```\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let staging = tempfile::tempdir().unwrap();
    let error = sources
        .execute(WorkspaceExecution {
            staging_parent: staging.path(),
            deadlines: ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, AssemblyError::InputsChanged), "{error:?}");
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
}

#[test]
fn page_veto_and_collection_authority_apply_to_included_cells() {
    for veto in [false, true] {
        let root = workspace("python3");
        let metadata = if veto {
            "---\nexecute: false\n---\n\n"
        } else {
            ""
        };
        if !veto {
            root.write(
                "diplodocus.yaml",
                root.read("diplodocus.yaml").replace(
                    "mode: execute\n      engine: jupyter\n      kernel: python3",
                    "mode: never",
                ),
            );
        }
        root.write(
            "pages/index.qmd",
            format!("{metadata}# Main\n\n{{{{< include ../shared/cell.md >}}}}\n"),
        );
        root.write(
            "shared/cell.md",
            "```{python}\n#| eval: true\nraise RuntimeError('must not execute')\n```\n",
        );
        let output = tempfile::tempdir().unwrap();
        let snapshot = build(&root, &output.path().join("site"));
        let id = snapshot.workspace().pages.keys().next().unwrap();
        assert!(snapshot.executed_page(id).is_none());
        assert!(!root.path().join(".diplodocus/cache/execution").exists());
    }
}
