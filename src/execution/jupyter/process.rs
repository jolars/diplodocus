//! Own the process group and private connection file through bounded cleanup.

use std::io::{ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use jupyter_protocol::{ConnectionInfo, Transport};
use jupyter_zmq_client::peek_ports_with_listeners;
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tempfile::TempDir;
use tokio::fs;
use tokio::process::{Child, Command};
use tokio::time::timeout;

use super::FailureSource;
use super::discovery::SelectedKernel;
use super::session::SessionInputs;
use super::transport::{Channels, random_token};
use crate::diagnostics::Diagnostic;
use crate::execution::{
    ExecutionFailure, ExecutionFailureKind, ExecutionPhase, KernelInterruptMode,
};

#[derive(Default)]
pub(super) struct KernelProcess {
    pub child: Option<Child>,
    group: Option<Pid>,
    directory: Option<TempDir>,
}

impl KernelProcess {
    pub async fn launch(
        &mut self,
        kernel: &SelectedKernel,
        inputs: &SessionInputs,
    ) -> Result<ConnectionInfo, ExecutionFailure> {
        let fail = |message| {
            inputs
                .source
                .failure(ExecutionFailureKind::Startup, message)
        };
        let root = fs::canonicalize(&inputs.repository_root)
            .await
            .map_err(|_| fail("The execution repository is unavailable."))?;
        let page = fs::canonicalize(&inputs.page_path)
            .await
            .map_err(|_| fail("The authored page is unavailable."))?;
        if root != inputs.repository_root
            || page != inputs.page_path
            || !page.starts_with(&root)
            || !fs::metadata(&page)
                .await
                .map_err(|_| fail("The authored page could not be inspected."))?
                .is_file()
        {
            return Err(fail(
                "The authored page must be a canonical file within its repository.",
            ));
        }
        let working_directory = page
            .parent()
            .ok_or_else(|| fail("The authored page has no working directory."))?;
        let executable = resolve_executable(kernel, working_directory, &inputs.source).await?;
        self.directory = Some(
            tempfile::Builder::new()
                .prefix("diplodocus-kernel-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .map_err(|_| fail("A private kernel connection directory could not be created."))?,
        );
        let connection_path = self
            .directory
            .as_ref()
            .expect("owned directory")
            .path()
            .join("connection.json");
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let (ports, listeners) = peek_ports_with_listeners(ip, 5)
            .await
            .map_err(|_| fail("Kernel loopback ports could not be reserved."))?;
        let connection = ConnectionInfo {
            transport: Transport::TCP,
            ip: ip.to_string(),
            shell_port: ports[0],
            iopub_port: ports[1],
            stdin_port: ports[2],
            control_port: ports[3],
            hb_port: ports[4],
            signature_scheme: "hmac-sha256".into(),
            key: random_token(&inputs.source)?,
            kernel_name: Some(kernel.name.clone()),
        };
        let bytes = serde_json::to_vec(&connection)
            .map_err(|_| fail("Kernel connection information could not be encoded."))?;
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&connection_path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| fail("The private kernel connection file could not be written."))?;
        let connection_path = connection_path
            .to_str()
            .ok_or_else(|| fail("The connection file path must be UTF-8."))?;
        let mut command = Command::new(executable);
        command
            .args(
                kernel
                    .argv
                    .iter()
                    .skip(1)
                    .map(|argument| argument.replace("{connection_file}", connection_path)),
            )
            .envs(&kernel.env)
            .current_dir(working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        // The kernel binds these ports itself; release reservations immediately before spawn.
        drop(listeners);
        let child = command
            .spawn()
            .map_err(|_| fail("The configured kernel executable could not be started."))?;
        self.group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(Pid::from_raw);
        self.child = Some(child);
        Ok(connection)
    }

    pub async fn cleanup(
        &mut self,
        channels: &mut Option<Channels>,
        kernel: &SelectedKernel,
        inputs: &SessionInputs,
        interrupt: bool,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if interrupt && self.running().unwrap_or(true) {
            let _ = timeout(Duration::from_millis(inputs.deadlines.interrupt), async {
                match kernel.interrupt_mode {
                    KernelInterruptMode::Message => {
                        if let Some(channels) = channels.as_mut() {
                            tokio::select! {
                                result = channels.interrupt() => result,
                                _ = self.child.as_mut().expect("running child").wait() => Ok(()),
                            }
                        } else { Ok(()) }
                    }
                    KernelInterruptMode::Signal => {
                        self.signal(Signal::INT)?;
                        if let Some(channels) = channels.as_mut() {
                            tokio::select! {
                                _ = self.child.as_mut().expect("running child").wait() => Ok(()),
                                result = async {
                                    loop {
                                        let message = channels.iopub.read().await.map_err(|_| ())?;
                                        if matches!(message.content, jupyter_protocol::JupyterMessageContent::Status(status)
                                            if status.execution_state == jupyter_protocol::ExecutionState::Idle) {
                                            return Ok(());
                                        }
                                    }
                                } => result,
                            }
                        } else { self.wait_for_exit().await }
                    }
                }
            }).await;
        }
        if self.running().unwrap_or(true)
            && let Some(channels) = channels.as_mut()
        {
            let _ = timeout(Duration::from_millis(inputs.deadlines.shutdown), async {
                channels.shutdown().await?;
                self.child
                    .as_mut()
                    .expect("running child")
                    .wait()
                    .await
                    .map_err(|_| ())?;
                Ok::<_, ()>(())
            })
            .await;
        }
        if !self.exited().unwrap_or(false) {
            let terminated = self.signal(Signal::TERM).is_ok()
                && matches!(
                    timeout(
                        Duration::from_millis(inputs.deadlines.termination),
                        self.wait_for_exit()
                    )
                    .await,
                    Ok(Ok(()))
                );
            if !terminated {
                let killed = self.signal(Signal::KILL).is_ok()
                    && matches!(
                        timeout(
                            Duration::from_millis(inputs.deadlines.forced_exit),
                            self.wait_for_exit()
                        )
                        .await,
                        Ok(Ok(()))
                    );
                if !killed {
                    diagnostics.extend(inputs.source.failure(
                        ExecutionFailureKind::Timeout { phase: ExecutionPhase::ForcedExit },
                        "The kernel process group could not be terminated and reaped within the cleanup deadline.").diagnostics);
                }
            }
        }
        if self.exited().unwrap_or(false) {
            self.group = None;
            self.child = None;
        }
        *channels = None;
        if let Some(directory) = self.directory.take()
            && directory.close().is_err()
        {
            diagnostics.extend(
                inputs
                    .source
                    .failure(
                        ExecutionFailureKind::Cleanup,
                        "The private kernel connection directory could not be removed.",
                    )
                    .diagnostics,
            );
        }
        diagnostics
    }

    fn running(&mut self) -> Result<bool, ()> {
        self.child
            .as_mut()
            .map(|child| {
                child
                    .try_wait()
                    .map(|status| status.is_none())
                    .map_err(|_| ())
            })
            .unwrap_or(Ok(false))
    }

    fn exited(&mut self) -> Result<bool, ()> {
        if self.running()? {
            return Ok(false);
        }
        match self.group.map(test_kill_process_group) {
            None | Some(Err(rustix::io::Errno::SRCH)) => Ok(true),
            Some(Ok(())) => Ok(false),
            Some(Err(_)) => Err(()),
        }
    }

    fn signal(&self, signal: Signal) -> Result<(), ()> {
        match self.group.map(|group| kill_process_group(group, signal)) {
            None | Some(Ok(())) | Some(Err(rustix::io::Errno::SRCH)) => Ok(()),
            Some(Err(_)) => Err(()),
        }
    }

    async fn wait_for_exit(&mut self) -> Result<(), ()> {
        loop {
            if self.exited()? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for KernelProcess {
    fn drop(&mut self) {
        // Normal cleanup reaps explicitly. This also covers a supervisor panic or runtime exit.
        let _ = self.signal(Signal::KILL);
    }
}

async fn resolve_executable(
    kernel: &SelectedKernel,
    working_directory: &Path,
    source: &FailureSource,
) -> Result<PathBuf, ExecutionFailure> {
    let declared = Path::new(&kernel.argv[0]);
    let candidates: Vec<_> = if declared.components().count() == 1 && !declared.is_absolute() {
        kernel
            .executable_path
            .as_ref()
            .map(|paths| {
                std::env::split_paths(paths)
                    .map(|path| working_directory.join(path).join(declared))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![working_directory.join(declared)]
    };
    for candidate in candidates {
        match fs::metadata(&candidate).await {
            Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
                return fs::canonicalize(candidate).await.map_err(|_| {
                    source.failure(
                        ExecutionFailureKind::Startup,
                        "The kernel executable could not be resolved.",
                    )
                });
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::NotADirectory
                ) => {}
            Err(_) => break,
        }
    }
    Err(source.failure(
        ExecutionFailureKind::Startup,
        "The configured kernel executable is unavailable or not executable.",
    ))
}
