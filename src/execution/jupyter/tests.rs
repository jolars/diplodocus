use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::FailureSource;
use super::discovery::{SearchEnvironment, discover_kernel};
use crate::diagnostics::{DiagnosticCode, DiagnosticPath};
use crate::execution::{ExecutionFailureKind, KernelInterruptMode, KernelSearchClass};
use crate::ir::SourceLocation;

mod fixture;

use super::session::start_session;
use crate::execution::{ExecutionContext, ExecutionDeadlines, ExecutionPhase};
use std::future::{pending, ready};
use std::time::Duration;

fn context(root: &Path) -> ExecutionContext<'static> {
    let page = root.join("guide/example.qmd");
    write(&page, "Example.");
    ExecutionContext {
        repository_root: root.to_owned(),
        page_path: page,
        asset_staging_directory: root.join("assets"),
        deadlines: ExecutionDeadlines {
            startup: 3_000,
            shutdown: 500,
            interrupt: 100,
            termination: 100,
            forced_exit: 1_000,
            ..ExecutionDeadlines::default()
        },
        cancellation: Box::pin(pending()),
    }
}

async fn fixture_kernel(root: &Path, mode: &str) -> super::discovery::SelectedKernel {
    let value = json!({
        "argv": [std::env::current_exe().unwrap(), "--exact",
            "execution::jupyter::tests::fixture::kernel_process", "--nocapture",
            "--skip={connection_file}"],
        "display_name": "Fixture", "language": "python", "interrupt_mode": "message",
        "env": {"DIPLODOCUS_KERNEL_FIXTURE": mode,
            "DIPLODOCUS_FIXTURE_OBSERVATIONS": root.join("observations.json"),
            "DIPLODOCUS_LITERAL": "space ; $(not-a-command)"}
    });
    install(&root.join("first"), "fixture", &value);
    discover_kernel("fixture", &environment(root), &source())
        .await
        .unwrap()
}

fn observation(root: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(root.join("observations.json")).unwrap()).unwrap()
}

async fn wait_for_observation(root: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.join("observations.json").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn assert_cleaned(root: &Path) {
    let data = observation(root);
    let connection = PathBuf::from(data["connection_file"].as_str().unwrap());
    let pid = rustix::process::Pid::from_raw(data["pid"].as_i64().unwrap() as i32).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !connection.parent().unwrap().exists()
                && rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("kernel must be reaped and its private connection directory removed");
    assert!(!root.join("assets").exists());
}

#[tokio::test]
async fn startup_validates_an_older_kernel_and_owns_its_launch_resources() {
    let root = TempDir::new().unwrap();
    let mut keys = Vec::new();
    for _ in 0..2 {
        let kernel = fixture_kernel(root.path(), "normal").await;
        let session = start_session(kernel, &mut context(root.path()), source())
            .await
            .unwrap();
        assert_eq!(session.runtime.language, "python");
        assert_eq!(session.runtime.protocol_version, "5.3");
        assert_eq!(session.runtime.implementation, "fixture");
        assert_eq!(session.runtime.implementation_version, "1.0");
        assert_eq!(session.runtime.language_version, "3.0");
        let data = observation(root.path());
        assert_eq!(
            data["working_directory"],
            root.path().join("guide").to_str().unwrap()
        );
        assert_eq!(data["literal"], "space ; $(not-a-command)");
        assert_eq!(data["file_mode"], 0o600);
        assert_eq!(data["directory_mode"], 0o700);
        assert_eq!(data["connection"]["ip"], "127.0.0.1");
        assert_eq!(data["connection"]["signature_scheme"], "hmac-sha256");
        keys.push(data["connection"]["key"].as_str().unwrap().to_owned());
        session.shutdown().await.unwrap();
        assert_cleaned(root.path()).await;
    }
    assert_ne!(keys[0], keys[1]);
    assert!(keys.iter().all(|key| key.len() == 64));
}

#[tokio::test]
async fn startup_retries_information_probes_until_iopub_is_ready() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "delayed-iopub").await;
    let session = start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap();
    session.shutdown().await.unwrap();
    assert_cleaned(root.path()).await;
    let events = std::fs::read_to_string(root.path().join("events")).unwrap();
    assert!(events.lines().filter(|event| *event == "info").count() >= 2);
    assert!(!events.contains("execute"));
}

#[tokio::test]
async fn startup_rejects_invalid_protocol_identity_and_authentication() {
    for mode in [
        "wrong-major",
        "wrong-language",
        "empty-version",
        "error-info",
        "wrong-key",
    ] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), mode).await;
        let failure = start_session(kernel, &mut context(root.path()), source())
            .await
            .err()
            .unwrap();
        assert_eq!(failure.kind, ExecutionFailureKind::Protocol, "{mode}");
        assert!(
            failure.diagnostics[0]
                .message
                .contains(if mode == "wrong-key" {
                    "authentication"
                } else {
                    "incomplete protocol"
                }),
            "{mode}: {failure:?}"
        );
        assert_cleaned(root.path()).await;
        assert!(
            !serde_json::to_string(&failure.diagnostics)
                .unwrap()
                .contains(root.path().to_str().unwrap())
        );
    }
}

#[tokio::test]
async fn startup_times_out_without_iopub_even_if_shell_replies_arrive() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "no-iopub").await;
    let mut context = context(root.path());
    context.deadlines.startup = 300;
    let failure = start_session(kernel, &mut context, source())
        .await
        .err()
        .unwrap();
    assert_eq!(
        failure.kind,
        ExecutionFailureKind::Timeout {
            phase: ExecutionPhase::Startup
        }
    );
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn cancellation_and_dropped_futures_do_not_abandon_children() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "normal").await;
    let mut cancelled = context(root.path());
    cancelled.cancellation = Box::pin(ready(()));
    let failure = start_session(kernel, &mut cancelled, source())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert!(!root.path().join("observations.json").exists());

    let kernel = fixture_kernel(root.path(), "no-iopub").await;
    let mut context = context(root.path());
    let task = tokio::spawn(async move { start_session(kernel, &mut context, source()).await });
    wait_for_observation(root.path()).await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert_cleaned(root.path()).await;

    let kernel = fixture_kernel(root.path(), "normal").await;
    drop(
        start_session(kernel, &mut super::tests::context(root.path()), source())
            .await
            .unwrap(),
    );
    assert_cleaned(root.path()).await;
    assert!(
        std::fs::read_to_string(root.path().join("events"))
            .unwrap()
            .contains("interrupt")
    );
}

#[tokio::test]
async fn process_exit_and_unresponsive_shutdown_are_cleaned_up() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "exit").await;
    assert!(
        start_session(kernel, &mut context(root.path()), source())
            .await
            .is_err()
    );
    assert_cleaned(root.path()).await;
    let kernel = fixture_kernel(root.path(), "ignore-shutdown").await;
    let session = start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap();
    session.shutdown().await.unwrap();
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn declared_python_and_r_start_without_submitting_code() {
    for (name, language) in [("python3", "python"), ("ir", "r")] {
        let root = TempDir::new().unwrap();
        let environment = SearchEnvironment::capture(&source()).unwrap();
        let kernel = discover_kernel(name, &environment, &source())
            .await
            .unwrap();
        let mut context = context(root.path());
        context.deadlines = ExecutionDeadlines::default();
        let session = start_session(kernel, &mut context, source()).await.unwrap();
        assert_eq!(session.runtime.language, language);
        assert!(!session.runtime.implementation_version.is_empty());
        session.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn startup_accepts_idle_before_reply_and_rejects_stdin() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "idle-first").await;
    start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    assert_cleaned(root.path()).await;
    let kernel = fixture_kernel(root.path(), "stdin").await;
    let failure = start_session(kernel, &mut context(root.path()), source())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::InputRequested);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn cancellation_uses_the_declared_signal_mode() {
    let root = TempDir::new().unwrap();
    let mut kernel = fixture_kernel(root.path(), "normal").await;
    kernel.interrupt_mode = KernelInterruptMode::Signal;
    let session = start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap();
    let failure = session.cancel().await.unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert!(failure.cleanup_diagnostics.is_empty());
    assert_cleaned(root.path()).await;
    assert!(
        std::fs::read_to_string(root.path().join("events"))
            .unwrap()
            .contains("signal-interrupt")
    );
}

#[tokio::test]
async fn an_active_startup_cancellation_waits_for_cleanup() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "no-iopub").await;
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let mut context = context(root.path());
    context.cancellation = Box::pin(async {
        let _ = cancelled.await;
    });
    let task = tokio::spawn(async move { start_session(kernel, &mut context, source()).await });
    wait_for_observation(root.path()).await;
    cancel.send(()).unwrap();
    let failure = task.await.unwrap().err().unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn shutdown_terminates_descendants_and_escalates_when_term_is_ignored() {
    for mode in ["descendant", "ignore-term"] {
        let root = TempDir::new().unwrap();
        let kernel = fixture_kernel(root.path(), mode).await;
        let session = start_session(kernel, &mut context(root.path()), source())
            .await
            .unwrap();
        session.shutdown().await.unwrap();
        assert_cleaned(root.path()).await;
        let events = std::fs::read_to_string(root.path().join("events")).unwrap();
        if mode == "descendant" {
            let pid = events
                .lines()
                .find_map(|line| line.strip_prefix("descendant:"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap()),
                Err(rustix::io::Errno::SRCH)
            );
        } else {
            assert!(events.contains("terminate"));
        }
    }
}

#[tokio::test]
async fn launch_resolves_path_before_applying_kernel_environment_overrides() {
    let root = TempDir::new().unwrap();
    let mut kernel = fixture_kernel(root.path(), "normal").await;
    std::fs::create_dir_all(root.path().join("bin")).unwrap();
    std::os::unix::fs::symlink(
        std::env::current_exe().unwrap(),
        root.path().join("bin/runtime"),
    )
    .unwrap();
    kernel.argv[0] = "runtime".into();
    kernel
        .env
        .insert("PATH".into(), "/not-a-runtime-directory".into());
    start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn launch_rejects_missing_executables_invalid_limits_and_escaping_pages() {
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "normal").await;
    let mut invalid = kernel.clone();
    invalid.argv[0] = "missing-executable".into();
    let failure = start_session(invalid, &mut context(root.path()), source())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Startup);
    let mut zero = context(root.path());
    zero.deadlines.startup = 0;
    assert!(
        start_session(kernel.clone(), &mut zero, source())
            .await
            .is_err()
    );
    let mut escape = context(root.path());
    escape.repository_root = root.path().join("first");
    assert!(start_session(kernel, &mut escape, source()).await.is_err());
    assert!(!root.path().join("observations.json").exists());
    assert!(!root.path().join("assets").exists());
}

#[tokio::test]
async fn cleanup_errors_preserve_the_primary_failure() {
    use std::os::unix::fs::PermissionsExt;
    if rustix::process::geteuid().is_root() {
        return;
    }
    let root = TempDir::new().unwrap();
    let kernel = fixture_kernel(root.path(), "normal").await;
    let session = start_session(kernel, &mut context(root.path()), source())
        .await
        .unwrap();
    let data = observation(root.path());
    let directory = Path::new(data["connection_file"].as_str().unwrap())
        .parent()
        .unwrap();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failure = session.cancel().await.unwrap_err();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert_eq!(failure.cleanup_diagnostics.len(), 1);
    assert_eq!(
        failure.cleanup_diagnostics[0].code,
        DiagnosticCode::ExecutionCleanupFailed
    );
    assert_cleaned(root.path()).await;
}

fn source() -> FailureSource {
    FailureSource {
        collection: "guide".into(),
        source: SourceLocation {
            repository: "docs".into(),
            path: DiagnosticPath::try_from("guide/example.qmd").unwrap(),
            span: None,
        },
    }
}

fn environment(root: &Path) -> SearchEnvironment {
    SearchEnvironment {
        jupyter_path: Some(
            std::env::join_paths([root.join("first"), root.join("second")]).unwrap(),
        ),
        jupyter_data_dir: Some(root.join("user")),
        xdg_data_home: Some(root.join("xdg")),
        home: Some(root.join("home")),
        path: Some(std::env::join_paths([root.join("bin")]).unwrap()),
        current_directory: root.to_owned(),
        system_local: root.join("local"),
        system: root.join("system"),
    }
}

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn spec() -> Value {
    json!({"argv": ["runtime", "--connection={connection_file}"],
        "display_name": "Test", "language": "Python3"})
}

fn install(root: &Path, name: &str, value: &Value) -> PathBuf {
    let path = root.join("kernels").join(name).join("kernel.json");
    write(&path, serde_json::to_vec(value).unwrap());
    path
}

#[tokio::test]
async fn discovery_selects_only_the_requested_name_and_reports_shadows() {
    let root = TempDir::new().unwrap();
    let env = environment(root.path());
    install(&root.path().join("first"), "PyThOn3", &spec());
    install(&root.path().join("second"), "python3", &spec());
    write(
        root.path().join("first/kernels/unrelated/kernel.json"),
        "invalid JSON",
    );
    let kernel = discover_kernel("PYTHON3", &env, &source()).await.unwrap();
    assert_eq!(kernel.name, "PYTHON3");
    assert_eq!(kernel.language, "python");
    assert_eq!(kernel.interrupt_mode, KernelInterruptMode::Signal);
    assert_eq!(kernel.directory, root.path().join("first/kernels/PyThOn3"));
    assert_eq!(kernel.search.len(), 5);
    assert_eq!(kernel.search[0].class, KernelSearchClass::JupyterPath);
    assert!(kernel.search[0].selected);
    assert_eq!(kernel.diagnostics.len(), 1);
    assert_eq!(
        kernel.diagnostics[0].code,
        DiagnosticCode::ShadowedKernelspec
    );
    assert!(
        !serde_json::to_string(&kernel.diagnostics)
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
}

#[tokio::test]
async fn discovery_deduplicates_roots_and_resolves_user_directory_precedence() {
    let root = TempDir::new().unwrap();
    let mut env = environment(root.path());
    install(&root.path().join("first"), "python3", &spec());
    std::os::unix::fs::symlink(root.path().join("first"), root.path().join("alias")).unwrap();
    env.jupyter_path = Some(
        std::env::join_paths([root.path().join("first/./"), root.path().join("alias")]).unwrap(),
    );
    let kernel = discover_kernel("python3", &env, &source()).await.unwrap();
    assert_eq!(kernel.search.len(), 4);
    assert!(kernel.diagnostics.is_empty());

    env.jupyter_path = None;
    for (directory, data, xdg) in [
        ("user", true, true),
        ("xdg/jupyter", false, true),
        ("home/.local/share/jupyter", false, false),
    ] {
        env.jupyter_data_dir = data.then(|| root.path().join("user"));
        env.xdg_data_home = xdg.then(|| root.path().join("xdg"));
        install(&root.path().join(directory), "python3", &spec());
        let kernel = discover_kernel("python3", &env, &source()).await.unwrap();
        assert_eq!(
            kernel.directory,
            root.path().join(directory).join("kernels/python3")
        );
        assert_eq!(kernel.search[0].class, KernelSearchClass::UserData);
    }
}

#[tokio::test]
async fn discovery_rejects_ambiguous_missing_and_malformed_selected_specs() {
    let root = TempDir::new().unwrap();
    let env = environment(root.path());
    install(&root.path().join("second"), "python3", &spec());
    let first = install(&root.path().join("first"), "python3", &spec());
    write(&first, "invalid JSON");
    assert_eq!(
        discover_kernel("python3", &env, &source())
            .await
            .err()
            .unwrap()
            .kind,
        ExecutionFailureKind::Startup
    );
    install(&root.path().join("first"), "PYTHON3", &spec());
    let failure = discover_kernel("python3", &env, &source())
        .await
        .err()
        .unwrap();
    assert!(failure.diagnostics[0].message.contains("ambiguous"));
    for name in ["", ".", "..", "../python3", "py/th", "pÿthon", "missing"] {
        assert!(
            discover_kernel(name, &env, &source()).await.is_err(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn discovery_rejects_invalid_launch_fields_before_any_process_starts() {
    let root = TempDir::new().unwrap();
    let env = environment(root.path());
    for (field, value) in [
        ("argv", json!([])),
        ("argv", json!(["", "{connection_file}"])),
        ("argv", json!(["runtime"])),
        ("argv", json!(["{connection_file}"])),
        ("argv", json!(["runtime", "\u{0000}{connection_file}"])),
        ("language", json!(" ")),
        ("interrupt_mode", json!("other")),
        ("env", json!({"VALUE": "${SECRET}"})),
        ("env", json!({"BAD=KEY": "value"})),
        ("env", json!({"VALUE": "\u{0000}"})),
        (
            "metadata",
            json!({"kernel_provisioner": {"provisioner_name": "remote"}}),
        ),
    ] {
        let mut invalid = spec();
        invalid[field] = value;
        install(&root.path().join("first"), "test", &invalid);
        assert!(
            discover_kernel("test", &env, &source()).await.is_err(),
            "{invalid}"
        );
    }
}
