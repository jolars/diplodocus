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
        vec!["build", "--config", "workspace.toml", "--output", "public"],
        vec![
            "serve",
            "--config",
            "workspace.toml",
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
            "workspace.toml",
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
                .contains("workspace.toml")
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
