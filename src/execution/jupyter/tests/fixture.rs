//! A subprocess kernel that can misbehave without importing a language runtime.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use jupyter_protocol::{
    ConnectionInfo, ExecuteReply, InterruptReply, JupyterMessage, JupyterMessageContent,
    KernelInfoReply, ReplyStatus, ShutdownReply, Status,
};
use jupyter_zmq_client::{
    KernelIoPubConnection, KernelShellConnection, KernelStdinConnection,
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
        let mut executions = 0;
        let mut active_request = None;
        let mut activity = tokio::time::interval(std::time::Duration::from_millis(10));
        loop {
            tokio::select! {
                _ = activity.tick(), if mode.starts_with("execute-chatty-") && active_request.is_some() => {
                    let request = active_request.as_ref().unwrap();
                    iopub.send(jupyter_protocol::StreamContent::stdout("still running\n").as_child_of(request)).await.unwrap();
                    event(observation, "activity");
                }
                message = shell.read() => {
                    let Ok(message) = message else {
                        if mode == "wrong-key" {
                            // An independently signed broadcast exposes the authentication mismatch.
                            iopub.send(Status::idle().into()).await.unwrap();
                            continue;
                        }
                        break;
                    };
                    if matches!(message.content, JupyterMessageContent::ExecuteRequest(_)) && mode.starts_with("execute-") {
                        executions += 1;
                        active_request = Some(message.clone());
                        if !execute(&mode, observation, executions, &message, &mut shell, &mut iopub, &mut stdin).await {
                            break;
                        }
                        continue;
                    }
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
                            if mode == "execute-slow-shutdown" {
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
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

async fn execute(
    mode: &str,
    observation: &Path,
    ordinal: usize,
    message: &JupyterMessage,
    shell: &mut KernelShellConnection,
    iopub: &mut KernelIoPubConnection,
    stdin: &mut KernelStdinConnection,
) -> bool {
    let JupyterMessageContent::ExecuteRequest(request) = &message.content else {
        unreachable!()
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(observation.with_file_name("requests"))
        .unwrap();
    writeln!(file, "{}", serde_json::to_string(request).unwrap()).unwrap();
    assert_eq!(
        request.code.trim() == "define",
        ordinal == 1,
        "State must belong to one page session"
    );
    event(observation, "execute");
    if mode == "execute-exit" {
        return false;
    }
    iopub
        .send(Status::busy().as_child_of(message))
        .await
        .unwrap();
    if mode == "execute-stdin" {
        stdin
            .send(
                jupyter_protocol::InputRequest {
                    prompt: "unexpected input".into(),
                    password: false,
                }
                .as_child_of(message),
            )
            .await
            .unwrap();
        return true;
    }
    if matches!(mode, "execute-no-terminal" | "execute-chatty-no-terminal") {
        return true;
    }
    if mode == "execute-streams" {
        for stream in [
            jupyter_protocol::StreamContent::stdout("# ordinary stdout\n"),
            jupyter_protocol::StreamContent::stderr("stderr\n"),
        ] {
            iopub.send(stream.as_child_of(message)).await.unwrap();
        }
    }
    if mode == "execute-markdown" {
        for stream in [
            jupyter_protocol::StreamContent::stdout("# Gener"),
            jupyter_protocol::StreamContent::stdout("ated\n"),
            jupyter_protocol::StreamContent::stderr("<stderr>&literal\n"),
        ] {
            iopub.send(stream.as_child_of(message)).await.unwrap();
        }
        let display: jupyter_protocol::DisplayData = serde_json::from_value(json!({
            "data": {
                "text/markdown": "```{python}\nraise RuntimeError('inert')\n```\n",
                "text/plain": "literal fallback"
            },
            "metadata": {}
        }))
        .unwrap();
        iopub.send(display.as_child_of(message)).await.unwrap();
    }
    if mode == "execute-images" {
        let display: jupyter_protocol::DisplayData = serde_json::from_value(json!({
            "data": {"image/svg+xml": "<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>", "text/plain": "a figure"},
            "metadata": {"filename": "../../ignored.svg"}
        })).unwrap();
        iopub.send(display.as_child_of(message)).await.unwrap();
    }
    if matches!(
        mode,
        "execute-generated-html-image" | "execute-generated-markdown-image"
    ) {
        let mut data = json!({
            "image/svg+xml": "<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>",
            "text/plain": "a safe fallback"
        });
        if mode == "execute-generated-html-image" {
            data["text/html"] = json!("<p><img src='missing.png'></p>");
        } else {
            data["text/markdown"] = json!("> ![nested](missing.png)\n");
        }
        let display: jupyter_protocol::DisplayData =
            serde_json::from_value(json!({"data": data, "metadata": {}})).unwrap();
        iopub.send(display.as_child_of(message)).await.unwrap();
    }
    let error = jupyter_protocol::ReplyError {
        ename: "FixtureError".into(),
        evalue: "expected".into(),
        traceback: Vec::new(),
    };
    let mut reply = ExecuteReply::default();
    if ordinal == 1 {
        match mode {
            "execute-error" | "execute-both-errors" => {
                reply.status = ReplyStatus::Error;
                reply.error = Some(Box::new(error.clone()));
            }
            "execute-iopub-error" => {
                iopub
                    .send(
                        jupyter_protocol::ErrorOutput {
                            ename: error.ename,
                            evalue: error.evalue,
                            traceback: error.traceback,
                        }
                        .as_child_of(message),
                    )
                    .await
                    .unwrap();
            }
            "execute-aborted" => reply.status = ReplyStatus::Aborted,
            "execute-malformed-reply" => reply.status = ReplyStatus::Error,
            _ => {}
        }
        if mode == "execute-both-errors" {
            iopub
                .send(
                    jupyter_protocol::ErrorOutput {
                        ename: "FixtureError".into(),
                        evalue: "expected".into(),
                        traceback: Vec::new(),
                    }
                    .as_child_of(message),
                )
                .await
                .unwrap();
        }
    }
    if mode == "execute-wrong-reply" {
        shell
            .send(Status::idle().as_child_of(message))
            .await
            .unwrap();
        return true;
    }
    // A terminal event for another request must never advance this page.
    let mut unrelated = message.clone();
    unrelated.header.msg_id = "unrelated-request".into();
    shell
        .send(reply.clone().as_child_of(&unrelated))
        .await
        .unwrap();
    iopub
        .send(Status::idle().as_child_of(&unrelated))
        .await
        .unwrap();
    let idle_first = matches!(
        mode,
        "execute-idle-first" | "execute-no-reply" | "execute-chatty-no-reply"
    );
    if idle_first {
        iopub
            .send(Status::idle().as_child_of(message))
            .await
            .unwrap();
    } else {
        shell
            .send(reply.clone().as_child_of(message))
            .await
            .unwrap();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_millis(60), async {
        loop {
            let next = shell.read().await.unwrap();
            // Startup may have queued another metadata probe before readiness.
            assert!(
                matches!(next.content, JupyterMessageContent::KernelInfoRequest(_)),
                "The next cell arrived before both terminal events"
            );
        }
    })
    .await;
    if matches!(
        mode,
        "execute-no-idle"
            | "execute-no-reply"
            | "execute-chatty-no-idle"
            | "execute-chatty-no-reply"
    ) {
        return true;
    }
    if idle_first {
        shell.send(reply.as_child_of(message)).await.unwrap();
    } else {
        iopub
            .send(Status::idle().as_child_of(message))
            .await
            .unwrap();
    }
    true
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
