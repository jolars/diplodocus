mod support;

use snapbox::cmd::{Command, cargo_bin};

use support::golden;

fn polydoc() -> Command {
    Command::new(cargo_bin("polydoc"))
}

#[test]
fn root_help_is_stable() {
    polydoc()
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
    ] {
        polydoc()
            .args([command, "--help"])
            .assert()
            .success()
            .stdout_eq(golden(snapshot))
            .stderr_eq("");
    }
}

#[test]
fn version_is_stable() {
    polydoc()
        .arg("--version")
        .assert()
        .success()
        .stdout_eq(golden("cli/version.stdout"))
        .stderr_eq("");
}

#[test]
fn commands_accept_their_explicit_options_and_reach_the_library() {
    for (arguments, snapshot) in [
        (
            vec!["build", "--config", "workspace.toml", "--output", "public"],
            "cli/build-not-implemented.stderr",
        ),
        (
            vec!["check", "--config", "workspace.toml"],
            "cli/check-not-implemented.stderr",
        ),
        (
            vec![
                "serve",
                "--config",
                "workspace.toml",
                "--output",
                "public",
                "--host",
                "0.0.0.0",
                "--port",
                "9000",
            ],
            "cli/serve-not-implemented.stderr",
        ),
    ] {
        polydoc()
            .args(arguments)
            .assert()
            .code(1)
            .stdout_eq("")
            .stderr_eq(golden(snapshot));
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
        polydoc()
            .args(arguments)
            .assert()
            .code(2)
            .stdout_eq("")
            .stderr_eq(golden(snapshot));
    }
}
