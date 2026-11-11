mod support;

use snapbox::cmd::{Command, cargo_bin};

use support::golden;

fn diplodocus() -> Command {
    Command::new(cargo_bin("diplodocus"))
}

#[test]
fn root_help_is_stable() {
    diplodocus()
        .arg("--help")
        .assert()
        .success()
        .stdout_eq(golden("cli/root-help.stdout"))
        .stderr_eq("");
}

#[test]
fn subcommand_help_is_stable() {
    for (command, snapshot) in [
        ("build", "cli/build-help.stdout"),
        ("check", "cli/check-help.stdout"),
        ("serve", "cli/serve-help.stdout"),
        ("extract", "cli/extract-help.stdout"),
        ("generate", "cli/generate-help.stdout"),
    ] {
        diplodocus()
            .args([command, "--help"])
            .assert()
            .success()
            .stdout_eq(golden(snapshot))
            .stderr_eq("");
    }
}

#[test]
fn version_matches_package_metadata() {
    diplodocus()
        .arg("--version")
        .assert()
        .success()
        .stdout_eq(concat!("diplodocus ", env!("CARGO_PKG_VERSION"), "\n"))
        .stderr_eq("");
}

#[test]
fn commands_accept_their_explicit_options_and_reach_the_library() {
    let workspace = support::TestWorkspace::new();
    for arguments in [
        vec!["build", "--config", "workspace.yaml", "--output", "public"],
        vec![
            "serve",
            "--config",
            "workspace.yaml",
            "--output",
            "public",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
        ],
        vec![
            "extract",
            "--config",
            "workspace.yaml",
            "--output",
            "documentation.sqlite",
        ],
    ] {
        let output = std::process::Command::new(cargo_bin("diplodocus"))
            .args(arguments)
            .current_dir(workspace.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("workspace.yaml")
        );
        assert!(!workspace.path().join("public").exists());
        assert!(!workspace.path().join("documentation.sqlite").exists());
    }
}

#[test]
fn malformed_input_has_a_usage_exit_without_panicking() {
    for (arguments, snapshot) in [
        (
            vec!["serve", "--port", "not-a-port"],
            "cli/invalid-port.stderr",
        ),
        (vec!["unknown"], "cli/unknown-command.stderr"),
        (Vec::new(), "cli/missing-command.stderr"),
    ] {
        diplodocus()
            .args(arguments)
            .assert()
            .code(2)
            .stdout_eq("")
            .stderr_eq(golden(snapshot));
    }
}

fn static_workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Console\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n");
    root.write("docs/index.md", "# Welcome\n");
    root
}

#[test]
fn finite_commands_keep_operational_output_on_stderr_and_support_quiet() {
    let root = static_workspace();
    for arguments in [
        vec!["check", "--no-progress"],
        vec!["--no-progress", "extract"],
        vec!["build"],
        vec!["generate", "--input", ".diplodocus/documentation.sqlite"],
        vec![
            "generate",
            "--input",
            ".diplodocus/documentation.sqlite",
            "--format",
            "fragments",
            "--output",
            "fragments",
        ],
    ] {
        let output = std::process::Command::new(cargo_bin("diplodocus"))
            .args(&arguments)
            .current_dir(root.path())
            .env("CI", "true")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Finished "), "{stderr}");
        assert!(!stderr.contains('\x1b'));
        let quiet = std::process::Command::new(cargo_bin("diplodocus"))
            .args(&arguments)
            .arg("--quiet")
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            quiet.status.success(),
            "{}",
            String::from_utf8_lossy(&quiet.stderr)
        );
        assert!(quiet.stdout.is_empty());
        assert!(
            quiet.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&quiet.stderr)
        );
    }
}

#[test]
fn color_controls_apply_to_help_usage_errors_and_runtime_diagnostics() {
    for arguments in [
        vec!["--help"],
        vec!["build", "--help"],
        vec!["serve", "--port", "invalid"],
        vec!["check", "--config", "missing-console-config.yaml"],
    ] {
        for before in [true, false] {
            for (choice, colored) in [("always", true), ("never", false)] {
                let mut command = std::process::Command::new(cargo_bin("diplodocus"));
                if before {
                    command.args(["--color", choice]);
                }
                command.args(&arguments);
                if !before {
                    command.arg(format!("--color={choice}"));
                }
                let output = command
                    .env("NO_COLOR", "1")
                    .env("CLICOLOR_FORCE", "1")
                    .output()
                    .unwrap();
                let bytes = if output.status.success() {
                    output.stdout
                } else {
                    output.stderr
                };
                assert_eq!(
                    bytes.contains(&0x1b),
                    colored,
                    "{arguments:?} {choice} before={before}: {}",
                    String::from_utf8_lossy(&bytes)
                );
            }
        }
    }
    let output = std::process::Command::new(cargo_bin("diplodocus"))
        .args([
            "check",
            "--quiet",
            "--color",
            "never",
            "--config",
            "missing-console-config.yaml",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Loading workspace"));
}

#[tokio::test]
async fn watched_extraction_keeps_flushed_json_events_with_and_without_quiet() {
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    for quiet in [false, true] {
        let root = static_workspace();
        let mut command = tokio::process::Command::new(cargo_bin("diplodocus"));
        command
            .current_dir(root.path())
            .args(["extract", "--watch", "--no-progress"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if quiet {
            command.arg("--quiet");
        }
        let mut child = command.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut stderr = child.stderr.take().unwrap();
        let diagnostics = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).await.unwrap();
            bytes
        });
        for generation in 0..2 {
            let line = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let event: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(event["format_version"], 1);
            assert_eq!(event["event"], "snapshot");
            let path = std::path::Path::new(event["path"].as_str().unwrap());
            assert!(path.is_absolute() && path.is_file());
            if generation == 0 {
                root.write("docs/index.md", "# Updated\n");
            }
        }
        child.kill().await.unwrap();
        assert!(lines.next_line().await.unwrap().is_none());
        let stderr = diagnostics.await.unwrap();
        if quiet {
            assert!(stderr.is_empty());
        } else {
            assert!(String::from_utf8_lossy(&stderr).contains("Watching for changes"));
        }
    }
}
