mod support;

use diplodocus::commands::{CheckOptions, check};
use diplodocus::diagnostics::{DiagnosticCode, Severity};
use snapbox::cmd::{Command, cargo_bin};

#[test]
fn check_runs_the_static_pipeline_with_missing_kernels_and_leaves_files_unchanged() {
    let root = support::acceptance_workspace();
    let config = root
        .read("workspace/diplodocus.yaml")
        .replace("kernel: \"python3\"", "kernel: \"must-not-be-discovered\"")
        .replace("kernel: \"ir\"", "kernel: \"also-missing\"");
    root.write("workspace/diplodocus.yaml", config);
    root.write("python/execution/must-not-run.qmd", "```{python}\nfrom pathlib import Path\nPath('execution-marker').write_text('executed')\n```\n");
    let before = tree(root.path());
    check(CheckOptions {
        config: root.path().join("workspace/diplodocus.yaml"),
    })
    .unwrap();
    assert_eq!(tree(root.path()), before);
    Command::new(cargo_bin("diplodocus"))
        .current_dir(root.path())
        .args(["check", "--config", "workspace/diplodocus.yaml"])
        .assert()
        .success()
        .stdout_eq("");
    assert_eq!(tree(root.path()), before);
}

#[test]
fn check_reports_reference_failures_repeatedly_without_publishing() {
    let root = support::acceptance_workspace();
    root.write("core/docs/bad.md", "[`missing-name`]\n");
    let before = tree(root.path());
    let mut previous = None;
    for _ in 0..2 {
        let result = std::process::Command::new(cargo_bin("diplodocus"))
            .current_dir(root.path())
            .args(["check", "--config", "workspace/diplodocus.yaml"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unresolved-item-reference"));
        if let Some(previous) = previous {
            assert_eq!(result.stderr, previous);
        }
        previous = Some(result.stderr);
        assert_eq!(tree(root.path()), before);
    }
}

#[test]
fn check_warns_about_duplicate_visible_titles_without_writing_files() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Titles\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n");
    root.write("docs/index.md", "# Repeated title\n");
    root.write("docs/guide/index.md", "# Repeated   title\n");
    let before = tree(root.path());
    let options = CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    };
    let report = check(options.clone()).unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    let warning = &report.diagnostics[0];
    assert_eq!(warning.code, DiagnosticCode::DuplicatePageTitle);
    assert_eq!(warning.severity, Severity::Warning);
    assert!(warning.message.contains("guide/index.html"));
    assert!(warning.message.contains("index.html"));
    assert_eq!(check(options.clone()).unwrap(), report);
    assert_eq!(tree(root.path()), before);
    let output = std::process::Command::new(cargo_bin("diplodocus"))
        .current_dir(root.path())
        .args(["check"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("warning[duplicate-page-title]"));
    root.write("docs/guide/index.md", "# Guide\n");
    assert!(check(options).unwrap().diagnostics.is_empty());
}

fn tree(root: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>)> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        files: &mut Vec<(std::path::PathBuf, Option<Vec<u8>>)>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                files.push((relative, None));
                walk(root, &path, files);
            } else {
                files.push((relative, Some(std::fs::read(path).unwrap())));
            }
        }
    }
    let mut files = vec![];
    walk(root, root, &mut files);
    files.sort();
    files
}
