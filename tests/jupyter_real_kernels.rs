use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use diplodocus::documents::{AuthoredFormat, parse_authored_document};
use diplodocus::ir::{Block, CodeCell};
use jupyter_protocol::{
    ConnectionInfo, ExecuteReply, ExecuteRequest, ExecutionState, JupyterMessage,
    JupyterMessageContent, KernelInfoReply, KernelInfoRequest, Media, MediaType, ReplyStatus,
    ShutdownRequest, Stdio as JupyterStdio, Transport,
};
use jupyter_zmq_client::{
    ClientIoPubConnection, ClientShellConnection, create_client_control_connection,
    create_client_iopub_connection, create_client_shell_connection_with_identity, find_kernelspec,
    peek_ports_with_listeners, peer_identity_for_session, wait_for_iopub_welcome,
};

#[path = "support/execution_observation.rs"]
mod execution_observation;
mod support;

const IO_TIMEOUT: Duration = Duration::from_secs(15);
const KERNEL_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declared_python_and_r_kernels_execute_the_acceptance_corpus() {
    for case in [
        RealKernelCase {
            kernel: "python3",
            path: "python/execution/stateful.qmd",
            language: "python",
            implementation: "ipython",
            stdout: "Python total: 12",
            stderr: "Intentional Python stderr",
            markdown: "**Python total:** `12`",
            error: Some(("RuntimeError", "intentional Python fixture error")),
            svg_text: "total = 12",
            workspace: support::acceptance_workspace,
            snapshot: "spikes/execution/real-python3.json",
        },
        RealKernelCase {
            kernel: "ir",
            path: "r/execution/stateful.qmd",
            language: "R",
            implementation: "IRkernel",
            stdout: "R total: 12",
            stderr: "Intentional R stderr",
            markdown: "**R total:** `12`",
            error: Some(("ERROR", "intentional R fixture error")),
            svg_text: "total = 12",
            workspace: support::acceptance_workspace,
            snapshot: "spikes/execution/real-ir.json",
        },
    ] {
        exercise_real_kernel(case).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declared_python_kernel_executes_the_real_documentation_example() {
    exercise_real_kernel(RealKernelCase {
        kernel: "python3",
        path: "docs/examples/stateful.qmd",
        language: "python",
        implementation: "ipython",
        stdout: "Total: 12",
        stderr: "Example stderr",
        markdown: "**Total:** `12`",
        error: None,
        svg_text: "Total: 12",
        workspace: support::own_documentation_workspace,
        snapshot: "dogfood/execution.json",
    })
    .await;
}

struct RealKernelCase {
    kernel: &'static str,
    path: &'static str,
    language: &'static str,
    implementation: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    markdown: &'static str,
    error: Option<(&'static str, &'static str)>,
    svg_text: &'static str,
    workspace: fn() -> support::TestWorkspace,
    snapshot: &'static str,
}

async fn exercise_real_kernel(case: RealKernelCase) {
    let source_workspace = (case.workspace)();
    let source = source_workspace.read(case.path);
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
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(
        cells.len(),
        4 + usize::from(case.error.is_some()),
        "unexpected corpus shape for {}",
        case.path
    );

    let kernelspec = find_kernelspec(case.kernel).await.unwrap_or_else(|error| {
        panic!("declared `{}` kernel is unavailable: {error}", case.kernel)
    });
    let declared_data_dir =
        std::env::var_os("JUPYTER_PATH").expect("the declared environment should set JUPYTER_PATH");
    assert_eq!(
        kernelspec.path,
        Path::new(&declared_data_dir)
            .join("kernels")
            .join(case.kernel),
        "kernel must come from the declared environment"
    );
    assert_eq!(kernelspec.kernelspec.language, case.language);
    assert!(
        kernelspec
            .kernelspec
            .argv
            .iter()
            .any(|argument| argument == "{connection_file}"),
        "declared `{}` kernelspec cannot accept a connection file",
        case.kernel
    );

    let workspace = support::TestWorkspace::new();
    let connection_path = workspace
        .path()
        .join(format!("kernel-{}.json", case.kernel));
    let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let (ports, listeners) = peek_ports_with_listeners(ip, 5)
        .await
        .expect("reserve real-kernel ports");
    let connection_info = ConnectionInfo {
        transport: Transport::TCP,
        ip: ip.to_string(),
        stdin_port: ports[0],
        control_port: ports[1],
        hb_port: ports[2],
        shell_port: ports[3],
        iopub_port: ports[4],
        signature_scheme: "hmac-sha256".to_string(),
        key: format!("diplodocus-{}-kernel-key", case.kernel),
        kernel_name: Some(case.kernel.to_string()),
    };
    tokio::fs::write(
        &connection_path,
        serde_json::to_vec(&connection_info).expect("serialize kernel connection information"),
    )
    .await
    .expect("write kernel connection file");

    let mut process = kernelspec
        .command(&connection_path, Some(Stdio::null()), Some(Stdio::null()))
        .expect("build kernel launch command")
        .current_dir(workspace.path())
        .kill_on_drop(true)
        .spawn()
        .unwrap_or_else(|error| panic!("start declared `{}` kernel: {error}", case.kernel));
    drop(listeners);

    let session_id = format!("diplodocus-real-{}", case.kernel);
    let identity = peer_identity_for_session(&session_id).expect("valid ZMQ peer identity");
    let mut shell =
        create_client_shell_connection_with_identity(&connection_info, &session_id, identity)
            .await
            .expect("connect real-kernel shell channel");
    let mut iopub = create_client_iopub_connection(&connection_info, "", &session_id)
        .await
        .expect("connect real-kernel IOPub channel");
    wait_for_iopub_welcome(&mut iopub, Duration::from_millis(500))
        .await
        .expect("establish real-kernel IOPub subscription");

    let info = kernel_info(&mut shell).await;
    assert_eq!(info.status, ReplyStatus::Ok);
    assert_eq!(info.implementation, case.implementation);
    assert_eq!(info.language_info.name, case.language);
    assert!(!info.implementation_version.is_empty());
    assert!(!info.language_info.version.is_empty());

    let mut outputs = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        let execution = execute_cell(&mut shell, &mut iopub, cell).await;
        assert_eq!(execution.0.execution_count.value(), index + 1);
        outputs.push(execution);
    }

    assert_eq!(outputs[0].0.status, ReplyStatus::Ok);
    assert!(outputs[0].1.is_empty());
    assert_eq!(outputs[1].0.status, ReplyStatus::Ok);
    assert!(
        has_stream(&outputs[1].1, JupyterStdio::Stdout, case.stdout),
        "missing stdout for `{}` in {:?}",
        case.kernel,
        outputs[1].1
    );
    assert!(
        has_stream(&outputs[1].1, JupyterStdio::Stderr, case.stderr),
        "missing stderr for `{}` in {:?}",
        case.kernel,
        outputs[1].1
    );
    assert_eq!(outputs[2].0.status, ReplyStatus::Ok);
    assert!(has_media(&outputs[2].1, |media| {
        matches!(media, MediaType::Markdown(value) if value == case.markdown)
    }));
    assert_eq!(outputs[3].0.status, ReplyStatus::Ok);
    assert!(has_media(&outputs[3].1, |media| {
        matches!(media, MediaType::Svg(value) if value.contains(case.svg_text))
    }));
    if let Some((error_name, error_value)) = case.error {
        assert_eq!(outputs[4].0.status, ReplyStatus::Error);
        assert!(
            outputs[4].1.iter().any(|output| {
                matches!(output, JupyterMessageContent::ErrorOutput(error)
                if error.ename == error_name && error.evalue.contains(error_value))
            }),
            "missing error output for `{}` in {:?}",
            case.kernel,
            outputs[4].1
        );
        let reply_error = outputs[4]
            .0
            .error
            .as_ref()
            .expect("error reply should retain the kernel error");
        assert_eq!(reply_error.ename, error_name);
        assert!(reply_error.evalue.contains(error_value));
    }

    let mut control = create_client_control_connection(&connection_info, &session_id)
        .await
        .expect("connect real-kernel control channel");
    control
        .send(ShutdownRequest { restart: false }.into())
        .await
        .expect("request real-kernel shutdown");
    let reply = tokio::time::timeout(IO_TIMEOUT, control.read())
        .await
        .expect("shutdown reply timeout")
        .expect("read shutdown reply");
    assert!(matches!(
        reply.content,
        JupyterMessageContent::ShutdownReply(reply)
            if reply.status == ReplyStatus::Ok && !reply.restart
    ));
    let status = tokio::time::timeout(KERNEL_TIMEOUT, process.wait())
        .await
        .expect("kernel process exit timeout")
        .expect("wait for kernel process");
    assert!(status.success(), "kernel process exited with {status}");

    let mut observation = execution_observation::ExecutionObservation::default();
    support::assert_json_golden(
        &serde_json::json!({
            "schema": "execution-spike-observation-v1",
            "producer": "real-kernel",
            "path": case.path,
            "kernel": {
                "name": case.kernel,
                "implementation": info.implementation,
                "implementation_version": info.implementation_version,
                "language": info.language_info.name,
                "language_version": info.language_info.version,
                "protocol_version": info.protocol_version,
            },
            "cells": cells.iter().zip(&outputs).enumerate().map(|(ordinal, (cell, (reply, outputs)))| {
                observation.cell(ordinal, cell, reply, outputs)
            }).collect::<Vec<_>>(),
        }),
        case.snapshot,
    );
}

async fn kernel_info(shell: &mut ClientShellConnection) -> KernelInfoReply {
    let request: JupyterMessage = KernelInfoRequest {}.into();
    let request_id = request.header.msg_id.clone();
    shell.send(request).await.expect("send kernel_info_request");

    loop {
        let reply = tokio::time::timeout(KERNEL_TIMEOUT, shell.read())
            .await
            .expect("kernel startup timeout")
            .expect("read kernel_info_reply");
        if reply
            .parent_header
            .as_ref()
            .is_none_or(|parent| parent.msg_id != request_id)
        {
            continue;
        }
        let JupyterMessageContent::KernelInfoReply(reply) = reply.content else {
            panic!("expected kernel_info_reply, got {:?}", reply.content);
        };
        return *reply;
    }
}

async fn execute_cell(
    shell: &mut ClientShellConnection,
    iopub: &mut ClientIoPubConnection,
    cell: &CodeCell,
) -> (ExecuteReply, Vec<JupyterMessageContent>) {
    let request: JupyterMessage = ExecuteRequest::new(cell.source.clone()).into();
    let request_id = request.header.msg_id.clone();
    shell.send(request).await.expect("send execute_request");

    let mut outputs = Vec::new();
    loop {
        let message = tokio::time::timeout(IO_TIMEOUT, iopub.read())
            .await
            .expect("IOPub timeout")
            .expect("read IOPub message");
        if message
            .parent_header
            .as_ref()
            .is_none_or(|parent| parent.msg_id != request_id)
        {
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
        .expect("read execute_reply");
    let JupyterMessageContent::ExecuteReply(reply) = reply.content else {
        panic!("expected execute_reply, got {:?}", reply.content);
    };
    (reply, outputs)
}

fn has_stream(outputs: &[JupyterMessageContent], name: JupyterStdio, text: &str) -> bool {
    outputs.iter().any(|output| {
        matches!(output, JupyterMessageContent::StreamContent(stream)
            if std::mem::discriminant(&stream.name) == std::mem::discriminant(&name)
                && stream.text.contains(text))
    })
}

fn has_media(outputs: &[JupyterMessageContent], predicate: impl Fn(&MediaType) -> bool) -> bool {
    outputs
        .iter()
        .filter_map(output_media)
        .any(|media| media.content.iter().any(&predicate))
}

fn output_media(output: &JupyterMessageContent) -> Option<&Media> {
    match output {
        JupyterMessageContent::DisplayData(output) => Some(&output.data),
        JupyterMessageContent::UpdateDisplayData(output) => Some(&output.data),
        JupyterMessageContent::ExecuteResult(output) => Some(&output.data),
        _ => None,
    }
}
