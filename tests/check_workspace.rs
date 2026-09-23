mod support;

use diplodocus::commands::{CheckOptions, check};
use snapbox::cmd::{Command, cargo_bin};

#[test]
fn check_runs_the_static_pipeline_with_missing_kernels_and_leaves_files_unchanged() {
    let root = support::acceptance_workspace();
    let config = root
        .read("workspace/diplodocus.toml")
        .replace(
            "kernel = \"python3\"",
            "kernel = \"must-not-be-discovered\"",
        )
        .replace("kernel = \"ir\"", "kernel = \"also-missing\"");
    root.write("workspace/diplodocus.toml", config);
    root.write("python/execution/must-not-run.qmd", "```{python}\nfrom pathlib import Path\nPath('execution-marker').write_text('executed')\n```\n");
    let before = tree(root.path());
    check(CheckOptions {
        config: root.path().join("workspace/diplodocus.toml"),
    })
    .unwrap();
    assert_eq!(tree(root.path()), before);
    Command::new(cargo_bin("diplodocus"))
        .current_dir(root.path())
        .env("PATH", "")
        .args(["check", "--config", "workspace/diplodocus.toml"])
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
            .args(["check", "--config", "workspace/diplodocus.toml"])
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
