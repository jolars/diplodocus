use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::Duration;

use diplodocus::documents::{AuthoredFormat, parse_authored_document};
use diplodocus::ir::{Block, CodeCell};
use jupyter_protocol::{
    ConnectionInfo, DisplayData, ErrorOutput, ExecuteReply, ExecuteRequest, ExecuteResult,
    ExecutionCount, ExecutionState, InterruptReply, InterruptRequest, JupyterMessage,
    JupyterMessageContent, Media, MediaType, ReplyStatus, ShutdownRequest, Stdio, StreamContent,
    Transient, Transport, UpdateDisplayData,
};
use jupyter_zmq_client::{
    CannedResponse, ClientIoPubConnection, ClientShellConnection, TestKernel, TestKernelConfig,
    create_client_control_connection, create_client_iopub_connection,
    create_client_shell_connection_with_identity, create_kernel_control_connection,
    peek_ports_with_listeners, peer_identity_for_session, read_kernelspec_jsons,
    wait_for_iopub_welcome,
};

mod support;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn zmq_client_discovers_kernelspecs_and_builds_the_launch_command() {
    let workspace = support::TestWorkspace::new();
    workspace.write(
        "kernels/python3/kernel.json",
        r#"{
  "argv": ["python", "-m", "ipykernel_launcher", "-f", "{connection_file}"],
  "display_name": "Python 3",
  "language": "python"
}"#,
    );
    workspace.write(
        "kernels/ir/kernel.json",
        r#"{
  "argv": ["R", "--slave", "-e", "IRkernel::main()", "--args", "{connection_file}"],
  "display_name": "R",
  "language": "R",
  "interrupt_mode": "signal"
}"#,
    );
    workspace.write("kernels/malformed/kernel.json", "not JSON");

    let mut kernels = read_kernelspec_jsons(workspace.path()).await;
    kernels.sort_by(|left, right| left.kernel_name.cmp(&right.kernel_name));

    assert_eq!(
        kernels
            .iter()
            .map(|kernel| kernel.kernel_name.as_str())
            .collect::<Vec<_>>(),
        ["ir", "python3"]
    );
    assert_eq!(
        kernels[0].kernelspec.interrupt_mode.as_deref(),
        Some("signal")
    );
    assert_eq!(kernels[1].kernelspec.interrupt_mode, None);

    let connection_file = Path::new("diplodocus-jupyter-spike.json");
    let command = kernels[1]
        .clone()
        .command(connection_file, None, None)
        .expect("valid kernelspec command");
    let command = command.as_std();
    assert_eq!(command.get_program(), "python");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "-m",
            "ipykernel_launcher",
            "-f",
            "diplodocus-jupyter-spike.json",
        ]
    );
}

#[test]
fn protocol_types_represent_the_required_mime_and_display_update_surface() {
    let display: DisplayData = serde_json::from_value(serde_json::json!({
        "data": {
            "text/plain": "total = 12",
            "text/markdown": "**total:** `12`",
            "text/html": "<strong>total: 12</strong>",
            "image/svg+xml": "<svg xmlns=\"http://www.w3.org/2000/svg\" />",
            "application/x-diplodocus-test": {"total": 12}
        },
        "metadata": {},
        "transient": {"display_id": "total-display"}
    }))
    .expect("protocol MIME bundle");

    assert!(
        display
            .data
            .content
            .iter()
            .any(|media| matches!(media, MediaType::Plain(value) if value == "total = 12"))
    );
    assert!(
        display
            .data
            .content
            .iter()
            .any(|media| matches!(media, MediaType::Markdown(value) if value == "**total:** `12`"))
    );
    assert!(
        display
            .data
            .content
            .iter()
            .any(|media| matches!(media, MediaType::Html(value) if value.contains("<strong>")))
    );
    assert!(
        display
            .data
            .content
            .iter()
            .any(|media| matches!(media, MediaType::Svg(value) if value.starts_with("<svg")))
    );
    assert!(display.data.content.iter().any(|media| {
        matches!(media, MediaType::Other((mime, value))
            if mime == "application/x-diplodocus-test" && value["total"] == 12)
    }));
    assert_eq!(
        display
            .transient
            .as_ref()
            .and_then(|transient| transient.display_id.as_deref()),
        Some("total-display")
    );

    let update = UpdateDisplayData::new(
        MediaType::Plain("total = 13".to_string()).into(),
        "total-display",
    );
    assert_eq!(
        update.transient.display_id.as_deref(),
        Some("total-display")
    );

    let interrupt: JupyterMessage = InterruptRequest {}.into();
    let shutdown: JupyterMessage = ShutdownRequest { restart: false }.into();
    assert_eq!(interrupt.content.message_type(), "interrupt_request");
    assert_eq!(shutdown.content.message_type(), "shutdown_request");
}

#[tokio::test]
async fn zmq_control_channel_carries_message_mode_interruption() {
    let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let (ports, listeners) = peek_ports_with_listeners(ip, 1)
        .await
        .expect("reserve control port");
    let connection_info = ConnectionInfo {
        ip: ip.to_string(),
        transport: Transport::TCP,
        shell_port: 0,
        iopub_port: 0,
        stdin_port: 0,
        control_port: ports[0],
        hb_port: 0,
        key: "spike-key".to_string(),
        signature_scheme: "hmac-sha256".to_string(),
        kernel_name: Some("spike".to_string()),
    };

    drop(listeners);
    let mut kernel_control = create_kernel_control_connection(&connection_info, "kernel-session")
        .await
        .expect("bind kernel control channel");
    let mut client_control = create_client_control_connection(&connection_info, "client-session")
        .await
        .expect("connect client control channel");

    let kernel = tokio::spawn(async move {
        let request = tokio::time::timeout(IO_TIMEOUT, kernel_control.read())
            .await
            .expect("interrupt request timeout")
            .expect("read interrupt request");
        assert!(matches!(
            request.content,
            JupyterMessageContent::InterruptRequest(_)
        ));
        kernel_control
            .send(InterruptReply::new().as_child_of(&request))
            .await
            .expect("send interrupt reply");
    });

    let request: JupyterMessage = InterruptRequest {}.into();
    let request_id = request.header.msg_id.clone();
    client_control
        .send(request)
        .await
        .expect("send interrupt request");
    let reply = tokio::time::timeout(IO_TIMEOUT, client_control.read())
        .await
        .expect("interrupt reply timeout")
        .expect("read interrupt reply");
    assert_eq!(
        reply.parent_header.as_ref().map(|header| &header.msg_id),
        Some(&request_id)
    );
    assert!(matches!(
        reply.content,
        JupyterMessageContent::InterruptReply(reply) if reply.status == ReplyStatus::Ok
    ));
    kernel.await.expect("join control-channel peer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zmq_and_protocol_crates_carry_the_execution_corpus_in_order() {
    for case in [
        CorpusCase {
            path: "python/execution/stateful.qmd",
            stdout: "Python total: 12\n",
            stderr: "Intentional Python stderr\n",
            markdown: "**Python total:** `12`",
            error_name: "RuntimeError",
            error_value: "intentional Python fixture error",
        },
        CorpusCase {
            path: "r/execution/stateful.qmd",
            stdout: "R total: 12\n",
            stderr: "Intentional R stderr\n",
            markdown: "**R total:** `12`",
            error_name: "simpleError",
            error_value: "intentional R fixture error",
        },
    ] {
        exercise_corpus(case).await;
    }
}

struct CorpusCase {
    path: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    markdown: &'static str,
    error_name: &'static str,
    error_value: &'static str,
}

async fn exercise_corpus(case: CorpusCase) {
    let source = support::load_fixture(format!("acceptance/{}", case.path));
    let parsed = parse_authored_document(&source, AuthoredFormat::Qmd);
    let cells = parsed
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::CodeCell(cell) => Some(cell.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), 5, "unexpected corpus shape for {}", case.path);

    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>total = 12</text></svg>";
    let config = TestKernelConfig::new()
        .with_response(cells[0].source.clone(), CannedResponse::default())
        .with_response(
            cells[1].source.clone(),
            response(vec![
                StreamContent::stdout(case.stdout).into(),
                StreamContent::stderr(case.stderr).into(),
            ]),
        )
        .with_response(
            cells[2].source.clone(),
            response(vec![
                ExecuteResult::new(
                    ExecutionCount::new(3),
                    Media::new(vec![
                        MediaType::Markdown(case.markdown.to_string()),
                        MediaType::Plain(case.markdown.to_string()),
                    ]),
                )
                .into(),
            ]),
        )
        .with_response(
            cells[3].source.clone(),
            response(vec![
                DisplayData {
                    data: MediaType::Svg(svg.to_string()).into(),
                    metadata: Default::default(),
                    transient: Some(Transient {
                        display_id: Some("figure".to_string()),
                    }),
                }
                .into(),
                UpdateDisplayData::new(MediaType::Svg(svg.replace("12", "12.0")).into(), "figure")
                    .into(),
            ]),
        )
        .with_response(
            cells[4].source.clone(),
            response(vec![
                ErrorOutput {
                    ename: case.error_name.to_string(),
                    evalue: case.error_value.to_string(),
                    traceback: vec![format!("{}: {}", case.error_name, case.error_value)],
                }
                .into(),
            ]),
        );

    let (kernel, connection_info) = TestKernel::start_ephemeral(config)
        .await
        .expect("start deterministic test kernel");
    let session_id = format!("diplodocus-{}", case.error_name.to_lowercase());
    let identity = peer_identity_for_session(&session_id).expect("valid ZMQ identity");
    let mut shell =
        create_client_shell_connection_with_identity(&connection_info, &session_id, identity)
            .await
            .expect("connect shell channel");
    let mut iopub = create_client_iopub_connection(&connection_info, "", &session_id)
        .await
        .expect("connect IOPub channel");
    assert!(
        wait_for_iopub_welcome(&mut iopub, IO_TIMEOUT)
            .await
            .expect("wait for IOPub subscription")
            .is_some()
    );

    let mut outputs = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        let (reply, cell_outputs) = execute_cell(&mut shell, &mut iopub, cell).await;
        assert_eq!(reply.status, ReplyStatus::Ok);
        assert_eq!(reply.execution_count.value(), index + 1);
        outputs.push(cell_outputs);
    }

    assert!(outputs[0].is_empty());
    assert!(matches!(
        outputs[1].as_slice(),
        [
            JupyterMessageContent::StreamContent(StreamContent {
                name: Stdio::Stdout,
                text: stdout,
            }),
            JupyterMessageContent::StreamContent(StreamContent {
                name: Stdio::Stderr,
                text: stderr,
            }),
        ] if stdout == case.stdout && stderr == case.stderr
    ));
    assert!(matches!(
        outputs[2].as_slice(),
        [JupyterMessageContent::ExecuteResult(result)]
            if result.data.content.iter().any(
                |media| matches!(media, MediaType::Markdown(value) if value == case.markdown)
            )
    ));
    assert!(matches!(
        outputs[3].as_slice(),
        [
            JupyterMessageContent::DisplayData(display),
            JupyterMessageContent::UpdateDisplayData(update),
        ] if display.transient.as_ref().and_then(|value| value.display_id.as_deref()) == Some("figure")
            && update.transient.display_id.as_deref() == Some("figure")
    ));
    assert!(matches!(
        outputs[4].as_slice(),
        [JupyterMessageContent::ErrorOutput(error)]
            if error.ename == case.error_name && error.evalue == case.error_value
    ));

    assert!(
        tokio::time::timeout(Duration::from_millis(25), iopub.read())
            .await
            .is_err(),
        "a stalled read can be bounded by Diplodocus's timeout policy"
    );

    let mut control = create_client_control_connection(&connection_info, &session_id)
        .await
        .expect("connect control channel");
    let shutdown: JupyterMessage = ShutdownRequest { restart: false }.into();
    control.send(shutdown).await.expect("request shutdown");
    let reply = tokio::time::timeout(IO_TIMEOUT, control.read())
        .await
        .expect("shutdown reply timeout")
        .expect("read shutdown reply");
    assert!(matches!(
        reply.content,
        JupyterMessageContent::ShutdownReply(reply)
            if reply.status == ReplyStatus::Ok && !reply.restart
    ));
    tokio::time::timeout(IO_TIMEOUT, kernel)
        .await
        .expect("kernel shutdown timeout")
        .expect("join test kernel")
        .expect("clean test-kernel shutdown");
}

fn response(outputs: Vec<JupyterMessageContent>) -> CannedResponse {
    CannedResponse { outputs }
}

async fn execute_cell(
    shell: &mut ClientShellConnection,
    iopub: &mut ClientIoPubConnection,
    cell: &CodeCell,
) -> (ExecuteReply, Vec<JupyterMessageContent>) {
    let request: JupyterMessage = ExecuteRequest::new(cell.source.clone()).into();
    let request_id = request.header.msg_id.clone();
    shell.send(request).await.expect("send execute request");

    let mut outputs = Vec::new();
    loop {
        let message = tokio::time::timeout(IO_TIMEOUT, iopub.read())
            .await
            .expect("IOPub timeout")
            .expect("read IOPub message");
        let belongs_to_request = message
            .parent_header
            .as_ref()
            .is_some_and(|parent| parent.msg_id == request_id);
        if !belongs_to_request {
            continue;
        }
        match message.content {
            JupyterMessageContent::Status(status)
                if status.execution_state == ExecutionState::Idle =>
            {
                break;
            }
            JupyterMessageContent::StreamContent(_)
            | JupyterMessageContent::DisplayData(_)
            | JupyterMessageContent::UpdateDisplayData(_)
            | JupyterMessageContent::ExecuteResult(_)
            | JupyterMessageContent::ErrorOutput(_) => outputs.push(message.content),
            _ => {}
        }
    }

    let reply = tokio::time::timeout(IO_TIMEOUT, shell.read())
        .await
        .expect("execute reply timeout")
        .expect("read execute reply");
    let JupyterMessageContent::ExecuteReply(reply) = reply.content else {
        panic!("expected execute_reply, got {:?}", reply.content);
    };
    (reply, outputs)
}
