//! Authenticated channel startup with a protocol-level readiness handshake.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::future::Future;
use std::io;
use std::time::Duration;

use jupyter_protocol::{
    ConnectionInfo, ExecutionState, InterruptRequest, JupyterMessage, JupyterMessageContent,
    KernelInfoReply, KernelInfoRequest, ReplyStatus, ShutdownRequest,
};
use jupyter_zmq_client::{
    ClientControlConnection, ClientHeartbeatConnection, ClientIoPubConnection,
    ClientShellConnection, ClientStdinConnection, RuntimeError, create_client_control_connection,
    create_client_heartbeat_connection, create_client_iopub_connection,
    create_client_shell_connection_with_identity, create_client_stdin_connection_with_identity,
    peer_identity_for_session,
};

use super::FailureSource;
use super::discovery::normalize_language;
use super::session::KernelRuntime;
use crate::execution::{ExecutionFailure, ExecutionFailureKind};

pub(super) struct Channels {
    pub shell: ClientShellConnection,
    pub iopub: ClientIoPubConnection,
    pub control: ClientControlConnection,
    pub stdin: ClientStdinConnection,
    pub heartbeat: ClientHeartbeatConnection,
}

impl Channels {
    pub async fn connect(
        info: &ConnectionInfo,
        source: &FailureSource,
    ) -> Result<Self, ExecutionFailure> {
        let session = random_token(source)?;
        let identity = peer_identity_for_session(&session).map_err(|_| protocol_failure(source))?;
        let (shell, iopub, control, stdin, heartbeat) = tokio::try_join!(
            retry(|| create_client_shell_connection_with_identity(
                info,
                &session,
                identity.clone()
            )),
            retry(|| create_client_iopub_connection(info, "", &session)),
            retry(|| create_client_control_connection(info, &session)),
            retry(|| create_client_stdin_connection_with_identity(
                info,
                &session,
                identity.clone()
            )),
            retry(|| create_client_heartbeat_connection(info)),
        )
        .map_err(|_| protocol_failure(source))?;
        Ok(Self {
            shell,
            iopub,
            control,
            stdin,
            heartbeat,
        })
    }

    pub async fn handshake(
        &mut self,
        language: &str,
        source: &FailureSource,
    ) -> Result<KernelRuntime, ExecutionFailure> {
        self.heartbeat
            .single_heartbeat()
            .await
            .map_err(|_| protocol_failure(source))?;
        let mut probes = HashSet::new();
        let mut replies = HashMap::new();
        let mut idle = HashSet::new();
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Repeating metadata requests recovers a lost initial PUB subscription.
                    let message: JupyterMessage = KernelInfoRequest {}.into();
                    probes.insert(message.header.msg_id.clone());
                    self.shell.send(message).await.map_err(|_| protocol_failure(source))?;
                }
                message = self.shell.read() => {
                    let message = message.map_err(|_| protocol_failure(source))?;
                    if let Some(parent) = message.parent_header.as_ref().filter(|parent| probes.contains(&parent.msg_id)) {
                        let JupyterMessageContent::KernelInfoReply(reply) = message.content else {
                            return Err(protocol_failure(source));
                        };
                        let runtime = validate_info(*reply, language, source)?;
                        if idle.contains(&parent.msg_id) { return Ok(runtime); }
                        replies.insert(parent.msg_id.clone(), runtime);
                    }
                }
                message = self.iopub.read() => {
                    let message = message.map_err(|_| protocol_failure(source))?;
                    if let Some(parent) = message.parent_header.as_ref().filter(|parent| probes.contains(&parent.msg_id))
                        && matches!(message.content, JupyterMessageContent::Status(status)
                            if status.execution_state == ExecutionState::Idle) {
                        if let Some(runtime) = replies.remove(&parent.msg_id) { return Ok(runtime); }
                        idle.insert(parent.msg_id.clone());
                    }
                }
                message = self.stdin.read() => {
                    let message = message.map_err(|_| protocol_failure(source))?;
                    if matches!(message.content, JupyterMessageContent::InputRequest(_)) {
                        return Err(source.failure(ExecutionFailureKind::InputRequested, "The kernel requested interactive input during startup."));
                    }
                }
            }
        }
    }

    pub async fn interrupt(&mut self) -> Result<(), ()> {
        let request: JupyterMessage = InterruptRequest {}.into();
        let id = request.header.msg_id.clone();
        self.control.send(request).await.map_err(|_| ())?;
        let mut replied = false;
        let mut idle = false;
        while !replied || !idle {
            tokio::select! {
                message = self.control.read() => {
                    let message = message.map_err(|_| ())?;
                    if message.parent_header.as_ref().is_some_and(|parent| parent.msg_id == id) {
                        if !matches!(message.content, JupyterMessageContent::InterruptReply(reply) if reply.status == ReplyStatus::Ok) {
                            return Err(());
                        }
                        replied = true;
                    }
                }
                message = self.iopub.read() => {
                    let message = message.map_err(|_| ())?;
                    if matches!(message.content, JupyterMessageContent::Status(status) if status.execution_state == ExecutionState::Idle) {
                        idle = true;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ()> {
        let request: JupyterMessage = ShutdownRequest { restart: false }.into();
        let id = request.header.msg_id.clone();
        self.control.send(request).await.map_err(|_| ())?;
        loop {
            let message = self.control.read().await.map_err(|_| ())?;
            if message
                .parent_header
                .as_ref()
                .is_some_and(|parent| parent.msg_id == id)
            {
                return match message.content {
                    JupyterMessageContent::ShutdownReply(reply)
                        if reply.status == ReplyStatus::Ok && !reply.restart =>
                    {
                        Ok(())
                    }
                    _ => Err(()),
                };
            }
        }
    }
}

fn validate_info(
    info: KernelInfoReply,
    language: &str,
    source: &FailureSource,
) -> Result<KernelRuntime, ExecutionFailure> {
    let version_ok = info
        .protocol_version
        .split_once('.')
        .is_some_and(|(major, minor)| major == "5" && minor.parse::<u32>().is_ok());
    if info.status != ReplyStatus::Ok
        || !version_ok
        || [
            &info.implementation,
            &info.implementation_version,
            &info.language_info.name,
            &info.language_info.version,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        || normalize_language(&info.language_info.name) != language
    {
        return Err(source.failure(
            ExecutionFailureKind::Protocol,
            "The kernel reported incompatible or incomplete protocol and language information.",
        ));
    }
    Ok(KernelRuntime {
        implementation: info.implementation,
        implementation_version: info.implementation_version,
        language: normalize_language(&info.language_info.name),
        language_version: info.language_info.version,
        protocol_version: info.protocol_version,
    })
}

pub(super) fn random_token(source: &FailureSource) -> Result<String, ExecutionFailure> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| {
        source.failure(
            ExecutionFailureKind::Startup,
            "Secure randomness for the kernel session is unavailable.",
        )
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn protocol_failure(source: &FailureSource) -> ExecutionFailure {
    source.failure(
        ExecutionFailureKind::Protocol,
        "Kernel channel communication or authentication failed.",
    )
}

async fn retry<T, F, Fut>(mut connect: F) -> Result<T, RuntimeError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RuntimeError>>,
{
    loop {
        match connect().await {
            Err(error) if transient(&error) => tokio::time::sleep(Duration::from_millis(10)).await,
            result => return result,
        }
    }
}

fn transient(error: &RuntimeError) -> bool {
    let mut cause: &(dyn Error + 'static) = error;
    loop {
        if let Some(error) = cause.downcast_ref::<io::Error>() {
            return matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::TimedOut
            );
        }
        match cause.source() {
            Some(source) => cause = source,
            None => return false,
        }
    }
}
