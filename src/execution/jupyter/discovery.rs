//! Static selection of only the requested kernel.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use jupyter_protocol::JupyterKernelspec;
use tokio::fs;

use super::FailureSource;
use crate::configuration::valid_kernel_selector;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::execution::{
    ExecutionFailure, ExecutionFailureKind, KernelInterruptMode, KernelSearchClass,
    KernelSearchLocation,
};
use crate::ir::Fingerprint;
use crate::provenance::fingerprint_bytes;

/// A snapshot makes discovery independent of concurrent environment changes.
pub(super) struct SearchEnvironment {
    pub jupyter_path: Option<OsString>,
    pub jupyter_data_dir: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub path: Option<OsString>,
    pub current_directory: PathBuf,
    pub system_local: PathBuf,
    pub system: PathBuf,
}

impl SearchEnvironment {
    pub fn capture(source: &FailureSource) -> Result<Self, ExecutionFailure> {
        let directory = |name| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Ok(Self {
            jupyter_path: std::env::var_os("JUPYTER_PATH"),
            jupyter_data_dir: directory("JUPYTER_DATA_DIR"),
            xdg_data_home: directory("XDG_DATA_HOME"),
            home: directory("HOME"),
            path: std::env::var_os("PATH"),
            current_directory: std::env::current_dir().map_err(|_| {
                source.failure(
                    ExecutionFailureKind::Startup,
                    "The build working directory is unavailable.",
                )
            })?,
            system_local: PathBuf::from("/usr/local/share/jupyter"),
            system: PathBuf::from("/usr/share/jupyter"),
        })
    }

    fn roots(&self) -> Vec<(KernelSearchLocation, PathBuf)> {
        let mut roots = Vec::new();
        if let Some(paths) = &self.jupyter_path {
            for (ordinal, path) in std::env::split_paths(paths)
                .filter(|path| !path.as_os_str().is_empty())
                .enumerate()
            {
                roots.push((location(KernelSearchClass::JupyterPath, ordinal), path));
            }
        }
        let user = self
            .jupyter_data_dir
            .clone()
            .or_else(|| self.xdg_data_home.as_ref().map(|path| path.join("jupyter")))
            .or_else(|| {
                self.home
                    .as_ref()
                    .map(|path| path.join(".local/share/jupyter"))
            });
        if let Some(user) = user {
            roots.push((location(KernelSearchClass::UserData, 0), user));
        }
        roots.push((
            location(KernelSearchClass::SystemLocal, 0),
            self.system_local.clone(),
        ));
        roots.push((location(KernelSearchClass::System, 0), self.system.clone()));
        roots
    }
}

fn location(class: KernelSearchClass, ordinal: usize) -> KernelSearchLocation {
    KernelSearchLocation {
        class,
        ordinal,
        selected: false,
    }
}

/// Launch data stays local; it must never be serialized or formatted in diagnostics.
#[derive(Clone)]
pub(super) struct SelectedKernel {
    pub name: String,
    pub directory: PathBuf,
    pub language: String,
    pub interrupt_mode: KernelInterruptMode,
    pub env: BTreeMap<String, String>,
    pub search: Vec<KernelSearchLocation>,
    pub diagnostics: Vec<Diagnostic>,
    pub executable_path: Option<OsString>,
    pub spec_observation: Fingerprint,
}

pub(super) async fn discover_kernel(
    name: &str,
    environment: &SearchEnvironment,
    source: &FailureSource,
) -> Result<SelectedKernel, ExecutionFailure> {
    let fail = |message| source.failure(ExecutionFailureKind::Startup, message);
    if !valid_kernel_selector(name) {
        return Err(fail("The configured kernel selector has invalid syntax."));
    }
    let mut seen = HashSet::new();
    let mut search = Vec::new();
    let mut selected = None;
    let mut diagnostics = Vec::new();
    for (mut location, path) in environment.roots() {
        let path = if path.is_absolute() {
            path
        } else {
            environment.current_directory.join(path)
        };
        let path = match fs::canonicalize(&path).await {
            Ok(path) => path,
            Err(error) if error.kind() == ErrorKind::NotFound => normalize_absolute(&path),
            Err(_) => {
                return Err(fail(
                    "A kernelspec search directory could not be inspected.",
                ));
            }
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let mut names = Vec::new();
        match fs::read_dir(path.join("kernels")).await {
            Ok(mut entries) => {
                while let Some(entry) = entries
                    .next_entry()
                    .await
                    .map_err(|_| fail("A kernelspec search directory could not be read."))?
                {
                    if entry
                        .file_name()
                        .to_str()
                        .is_some_and(|entry| entry.eq_ignore_ascii_case(name))
                    {
                        names.push(entry.file_name());
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(fail("A kernelspec search directory could not be read.")),
        }
        if names.len() > 1 {
            return Err(fail(
                "The configured kernel name is ambiguous within a search directory.",
            ));
        }
        if let Some(name) = names.pop() {
            if selected.is_none() {
                let directory = path.join("kernels").join(name);
                let (spec, observation) = read_spec(&directory, source).await?;
                selected = Some((directory, spec, observation));
                location.selected = true;
            } else {
                let mut diagnostic = ExecutionFailureKind::Startup
                    .to_diagnostic(&source.collection, source.source.clone());
                diagnostic.code = DiagnosticCode::ShadowedKernelspec;
                diagnostic.severity = Severity::Warning;
                diagnostic.message = format!(
                    "A matching kernelspec at {:?} root {} is shadowed by the selected kernel.",
                    location.class, location.ordinal
                );
                diagnostics.push(diagnostic);
            }
        }
        search.push(location);
    }
    let (directory, spec, spec_observation) = selected
        .ok_or_else(|| fail("The configured kernel was not found in the search locations."))?;
    Ok(SelectedKernel {
        name: name.into(),
        directory,
        language: normalize_language(&spec.language),
        interrupt_mode: if spec.interrupt_mode.as_deref() == Some("message") {
            KernelInterruptMode::Message
        } else {
            KernelInterruptMode::Signal
        },
        env: spec.env.unwrap_or_default().into_iter().collect(),
        search,
        diagnostics,
        executable_path: environment.path.clone(),
        spec_observation,
    })
}

async fn read_spec(
    directory: &Path,
    source: &FailureSource,
) -> Result<(JupyterKernelspec, Fingerprint), ExecutionFailure> {
    let fail = |message| source.failure(ExecutionFailureKind::Startup, message);
    let bytes = fs::read(directory.join("kernel.json"))
        .await
        .map_err(|_| fail("The selected kernelspec could not be read."))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| fail("The selected kernelspec is not valid JSON."))?;
    let spec: JupyterKernelspec = serde_json::from_value(value.clone())
        .map_err(|_| fail("The selected kernelspec has invalid fields."))?;
    if value.as_object().is_none_or(|object| {
        object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "argv" | "language" | "display_name" | "interrupt_mode" | "env" | "metadata"
            )
        })
    }) || spec.metadata.as_ref().is_some_and(|metadata| {
        metadata.contains_key("kernel_provisioner") || metadata.contains_key("supported_encryption")
    }) {
        return Err(fail(
            "The selected kernelspec declares an unsupported launch extension.",
        ));
    }
    if spec
        .argv
        .first()
        .is_none_or(|argument| argument.is_empty() || argument.contains("{connection_file}"))
        || spec.argv.iter().any(|argument| argument.contains('\0'))
        || !spec
            .argv
            .iter()
            .skip(1)
            .any(|argument| argument.contains("{connection_file}"))
    {
        return Err(fail(
            "The kernelspec requires a valid executable and a connection-file argument.",
        ));
    }
    if spec.language.trim().is_empty() {
        return Err(fail("The kernelspec language must be nonempty."));
    }
    if !matches!(
        spec.interrupt_mode.as_deref(),
        None | Some("signal" | "message")
    ) {
        return Err(fail(
            "The kernelspec interrupt mode must be signal or message.",
        ));
    }
    if spec.env.as_ref().is_some_and(|env| {
        env.iter().any(|(name, value)| {
            name.is_empty()
                || name.contains(['=', '\0'])
                || value.contains('\0')
                || value.contains("${")
        })
    }) {
        return Err(fail(
            "The kernelspec environment requires valid names and literal values without variable expansion.",
        ));
    }
    Ok((spec, fingerprint_bytes(&bytes)))
}

pub(super) fn normalize_language(language: &str) -> String {
    match language.to_ascii_lowercase().as_str() {
        "python" | "python3" => "python".into(),
        other => other.into(),
    }
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}
