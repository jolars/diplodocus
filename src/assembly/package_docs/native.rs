use super::*;
use std::process::Stdio;
use std::time::Duration;

use crate::execution::{ExecutionCancellation, ExecutionDeadlines};

const HELPER: &str = include_str!("native.R");

pub(super) struct NativeResult {
    pub runtime: RPackageRuntime,
    pub news: Option<String>,
}

pub(super) fn diagnostic(
    id: &str,
    location: &SourceLocation,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::new(DiagnosticCode::RMetadata, Severity::Error, message)
        .with_source(source(location))
        .with_entity(DiagnosticEntity::Package { id: id.into() })
}

struct Process {
    child: tokio::process::Child,
    #[cfg(target_os = "linux")]
    group: Option<rustix::process::Pid>,
}

impl Process {
    fn kill(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(group) = self.group {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.child.start_kill();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(super) async fn complete(
    id: &str,
    package: &ResolvedPackagePaths,
    citation: Option<&Path>,
    news: Option<&Path>,
    location: &SourceLocation,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<NativeResult, AssemblyError> {
    let staging = tempfile::tempdir()?;
    let output = staging.path().join("metadata");
    let errors = staging.path().join("errors");
    let mut command = tokio::process::Command::new("R");
    command
        .args(["--vanilla", "--slave", "-e", HELPER, "--args"])
        .arg(&package.metadata_path)
        .arg(citation.unwrap_or(Path::new("")))
        .arg(news.unwrap_or(Path::new("")))
        .arg(&output)
        .current_dir(&package.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&errors)?)
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let child = command.spawn().map_err(|_| {
        AssemblyError::Diagnostics(vec![diagnostic(
            id,
            location,
            "R package metadata completion requires R on PATH.",
        )])
    })?;
    #[cfg(target_os = "linux")]
    let group = child
        .id()
        .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
    let mut process = Process {
        child,
        #[cfg(target_os = "linux")]
        group,
    };
    let status = tokio::select! {
        result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => {
            match result {
                Ok(result) => result?,
                Err(_) => {
                    process.kill();
                    reap(&mut process, deadlines).await?;
                    return Err(AssemblyError::Diagnostics(vec![diagnostic(id, location, "R package metadata completion timed out.")]));
                }
            }
        }
        () = cancellation.as_mut() => {
            process.kill();
            reap(&mut process, deadlines).await?;
            return Err(AssemblyError::Cancelled);
        }
    };
    if !status.success() {
        // Native messages can contain checkout paths; portable diagnostics name sources separately.
        let message = fs::read_to_string(errors)
            .unwrap_or_default()
            .replace(package.path.to_string_lossy().as_ref(), "<package>")
            .replace(staging.path().to_string_lossy().as_ref(), "<metadata>");
        return Err(AssemblyError::Diagnostics(vec![diagnostic(
            id,
            location,
            format!("R package metadata completion failed: {}", message.trim()),
        )]));
    }
    decode(&fs::read_to_string(output)?, id, location)
}

async fn reap(process: &mut Process, deadlines: ExecutionDeadlines) -> Result<(), AssemblyError> {
    tokio::time::timeout(
        Duration::from_millis(deadlines.forced_exit.max(1)),
        process.child.wait(),
    )
    .await
    .map_err(|_| std::io::Error::other("R metadata helper did not exit after termination"))??;
    Ok(())
}

fn decode(text: &str, id: &str, location: &SourceLocation) -> Result<NativeResult, AssemblyError> {
    let invalid = || {
        AssemblyError::Diagnostics(vec![diagnostic(
            id,
            location,
            "R package metadata helper returned malformed results.",
        )])
    };
    let mut authors = vec![];
    let mut maintainers = vec![];
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('\t').ok_or_else(invalid)?;
        if value.len() % 2 != 0 {
            return Err(invalid());
        }
        let bytes = value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let value = std::str::from_utf8(pair).map_err(|_| invalid())?;
                u8::from_str_radix(value, 16).map_err(|_| invalid())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let value = String::from_utf8(bytes).map_err(|_| invalid())?;
        if key == "author" {
            authors.push(value);
        } else if key == "maintainer" {
            maintainers.push(value);
        } else if fields.insert(key.to_owned(), value).is_some() {
            return Err(invalid());
        }
    }
    if fields.remove("complete").as_deref() != Some("r-package-metadata-v1") {
        return Err(invalid());
    }
    let r_version = fields
        .remove("version")
        .filter(|version| !version.is_empty())
        .ok_or_else(invalid)?;
    let citation = match (fields.remove("citation"), fields.remove("bibtex")) {
        (Some(text), Some(bibtex)) => Some(RCitation { text, bibtex }),
        (None, None) => None,
        _ => return Err(invalid()),
    };
    let news = fields.remove("news");
    if !fields.is_empty() {
        return Err(invalid());
    }
    Ok(NativeResult {
        runtime: RPackageRuntime {
            authors,
            maintainers,
            citation,
            r_version,
        },
        news,
    })
}
