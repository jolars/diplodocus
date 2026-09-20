//! A subprocess kernel that can misbehave without importing a language runtime.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use jupyter_protocol::{
    ConnectionInfo, InterruptReply, JupyterMessageContent, KernelInfoReply, ReplyStatus,
    ShutdownReply, Status,
};
use jupyter_zmq_client::{
    create_kernel_control_connection, create_kernel_heartbeat_connection,
    create_kernel_iopub_connection, create_kernel_shell_connection, create_kernel_stdin_connection,
};
use serde_json::json;

fn event(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.with_file_name("events"))
        .unwrap();
    writeln!(file, "{text}").unwrap();
}

#[test]
fn kernel_process() {
    let Ok(mode) = std::env::var("DIPLODOCUS_KERNEL_FIXTURE") else {
        return;
    };
    let observation = std::env::var_os("DIPLODOCUS_FIXTURE_OBSERVATIONS").unwrap();
    let observation = Path::new(&observation);
    let connection = std::env::args()
        .find_map(|arg| arg.strip_prefix("--skip=").map(str::to_owned))
        .unwrap();
    let connection_path = Path::new(&connection);
    let mut info: ConnectionInfo =
        serde_json::from_slice(&std::fs::read(connection_path).unwrap()).unwrap();
    let record = json!({
        "pid": std::process::id(), "connection_file": connection,
        "working_directory": std::env::current_dir().unwrap(),
        "file_mode": std::fs::metadata(connection_path).unwrap().permissions().mode() & 0o777,
        "directory_mode": std::fs::metadata(connection_path.parent().unwrap()).unwrap().permissions().mode() & 0o777,
        "literal": std::env::var("DIPLODOCUS_LITERAL").unwrap(), "connection": info
    });
    let temporary = observation.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&record).unwrap()).unwrap();
    std::fs::rename(temporary, observation).unwrap();
    if mode == "exit" {
        return;
    }
    if mode == "wrong-key" {
        info.key = "wrong-key".into();
    }
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        let mut descendant = if mode == "descendant" {
            let child = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "execution::jupyter::tests::fixture::descendant_process"])
                .env("DIPLODOCUS_DESCENDANT", "1").spawn().unwrap();
            event(observation, &format!("descendant:{}", child.id().unwrap()));
            Some(child)
        } else { None };
        let mut shell = create_kernel_shell_connection(&info, "fixture").await.unwrap();
        let mut iopub = create_kernel_iopub_connection(&info, "fixture").await.unwrap();
        let mut control = create_kernel_control_connection(&info, "fixture").await.unwrap();
        let mut stdin = create_kernel_stdin_connection(&info, "fixture").await.unwrap();
        let mut heartbeat = create_kernel_heartbeat_connection(&info).await.unwrap();
        let mut probes = 0;
        loop {
            tokio::select! {
                message = shell.read() => {
                    let Ok(message) = message else {
                        if mode == "wrong-key" {
                            // An independently signed broadcast exposes the authentication mismatch.
                            iopub.send(Status::idle().into()).await.unwrap();
                            continue;
                        }
                        break;
                    };
                    if !matches!(message.content, JupyterMessageContent::KernelInfoRequest(_)) {
                        event(observation, "execute");
                        panic!("Startup must never submit code");
                    }
                    event(observation, "info");
                    probes += 1;
                    if mode == "stdin" {
                        stdin.send(jupyter_protocol::InputRequest { prompt: "unexpected input".into(), password: false }.as_child_of(&message)).await.unwrap();
                        continue;
                    }
                    let mut reply: KernelInfoReply = serde_json::from_value(json!({
                        "status": "ok", "protocol_version": "5.3", "implementation": "fixture",
                        "implementation_version": "1.0", "language_info": {"name": "Python3", "version": "3.0"},
                        "banner": "", "help_links": []
                    })).unwrap();
                    match mode.as_str() {
                        "wrong-major" => reply.protocol_version = "6.0".into(),
                        "wrong-language" => reply.language_info.name = "R".into(),
                        "empty-version" => reply.implementation_version.clear(),
                        "error-info" => reply.status = ReplyStatus::Error,
                        _ => {}
                    }
                    // Unrelated replies must not satisfy the startup handshake.
                    let mut unrelated = reply.clone().as_child_of(&message);
                    unrelated.parent_header = None;
                    shell.send(unrelated).await.unwrap();
                    if mode == "idle-first" {
                        iopub.send(Status::idle().as_child_of(&message)).await.unwrap();
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    shell.send(reply.as_child_of(&message)).await.unwrap();
                    if mode != "no-iopub" && (mode != "delayed-iopub" || probes > 1) {
                        iopub.send(Status::busy().as_child_of(&message)).await.unwrap();
                        iopub.send(Status::idle().as_child_of(&message)).await.unwrap();
                    }
                }
                message = control.read() => {
                    let Ok(message) = message else { continue; };
                    match message.content {
                        JupyterMessageContent::InterruptRequest(_) => {
                            event(observation, "interrupt");
                            control.send(InterruptReply { status: ReplyStatus::Ok, error: None }.as_child_of(&message)).await.unwrap();
                            iopub.send(Status::idle().as_child_of(&message)).await.unwrap();
                        }
                        JupyterMessageContent::ShutdownRequest(_) => {
                            event(observation, "shutdown");
                            if matches!(mode.as_str(), "ignore-shutdown" | "ignore-term") { continue; }
                            control.send(ShutdownReply { status: ReplyStatus::Ok, restart: false, error: None }.as_child_of(&message)).await.unwrap();
                            if let Some(child) = descendant.as_mut() { child.wait().await.unwrap(); }
                            break;
                        }
                        _ => panic!("unexpected control request"),
                    }
                }
                result = heartbeat.single_heartbeat() => { result.unwrap(); }
                _ = sigint.recv() => {
                    event(observation, "signal-interrupt");
                    iopub.send(Status::idle().into()).await.unwrap();
                }
                _ = sigterm.recv() => {
                    event(observation, "terminate");
                    if mode != "ignore-term" { break; }
                }
            }
        }
    });
}

#[test]
fn descendant_process() {
    if std::env::var_os("DIPLODOCUS_DESCENDANT").is_none() {
        return;
    }
    loop {
        std::thread::park();
    }
}
