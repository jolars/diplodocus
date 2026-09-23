use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::diagnostics::DiagnosticPath;
use crate::execution::{ExecutionComponent, ExecutionPlatform};
use crate::ir::Fingerprint;
use thiserror::Error;

use super::{CanonicalValue, content_digest, domain_digest};

/// A local observation failed. The error deliberately excludes paths and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Execution input observation is invalid or unavailable.")]
pub struct IdentityError;

impl From<super::CanonicalError> for IdentityError {
    fn from(_: super::CanonicalError) -> Self {
        Self
    }
}

/// An explicit repository-relative file declaration, independent of its digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepositoryFile {
    /// Stable configured repository identifier.
    pub repository: String,
    /// Normalized declared spelling, never a canonical absolute path.
    pub path: DiagnosticPath,
}

impl RepositoryFile {
    /// Normalize a configured path lexically without permitting an escape.
    pub fn new(repository: impl Into<String>, path: &Path) -> Result<Self, IdentityError> {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => components.push(part.to_str().ok_or(IdentityError)?),
                Component::ParentDir => {
                    components.pop().ok_or(IdentityError)?;
                }
                _ => return Err(IdentityError),
            }
        }
        let path = components.join("/");
        if path.contains(['*', '?', '[', ']']) {
            return Err(IdentityError);
        }
        let repository = repository.into();
        if repository.is_empty() {
            return Err(IdentityError);
        }
        Ok(Self {
            repository,
            path: DiagnosticPath::try_from(path).map_err(|_| IdentityError)?,
        })
    }
}

/// Exact observations of the running engine and selected build components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildObservation {
    /// Diplodocus crate version, with no requirement operators.
    pub engine_version: String,
    /// Digest of the running executable's exact bytes.
    pub executable_digest: Fingerprint,
    /// One selected implementation per unique role.
    pub components: BTreeMap<String, ExecutionComponent>,
    /// Cargo's target platform, which may differ from the build host.
    pub platform: ExecutionPlatform,
}

mod generated {
    include!(concat!(env!("OUT_DIR"), "/execution_build.rs"));
}

impl BuildObservation {
    /// Read the running executable and combine it with build-time locked metadata.
    pub async fn observe() -> Result<Self, IdentityError> {
        let executable = std::env::current_exe().map_err(|_| IdentityError)?;
        let (_, bytes) = read_regular(&executable).await?;
        Ok(Self {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            executable_digest: fingerprint(&content_digest(&bytes))?,
            components: generated::COMPONENTS
                .iter()
                .map(|(role, name, version)| {
                    (
                        (*role).into(),
                        ExecutionComponent {
                            name: (*name).into(),
                            version: (*version).into(),
                        },
                    )
                })
                .collect(),
            platform: ExecutionPlatform {
                os: generated::OS.into(),
                architecture: generated::ARCH.into(),
                target: generated::TARGET.into(),
            },
        })
    }
}

pub(super) fn fingerprint(value: &str) -> Result<Fingerprint, IdentityError> {
    let hex = value.strip_prefix("sha256:").ok_or(IdentityError)?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(IdentityError);
    }
    Ok(Fingerprint {
        algorithm: "sha256".into(),
        value: hex.into(),
    })
}

pub(super) fn digest(value: &Fingerprint) -> Result<String, IdentityError> {
    let text = format!("{}:{}", value.algorithm, value.value);
    fingerprint(&text)?;
    Ok(text)
}

pub(super) fn canonical(value: serde_json::Value) -> Result<CanonicalValue, IdentityError> {
    Ok(CanonicalValue::from_json(value)?)
}
pub(super) fn hash(domain: &str, value: serde_json::Value) -> Result<String, IdentityError> {
    Ok(domain_digest(domain, &canonical(value)?)?)
}

pub(super) async fn roots(repositories: &BTreeMap<String, PathBuf>) -> Result<(), IdentityError> {
    if repositories.is_empty() {
        return Err(IdentityError);
    }
    for (id, path) in repositories {
        if id.is_empty()
            || !path.is_absolute()
            || path.to_str().is_none()
            || tokio::fs::canonicalize(path)
                .await
                .map_err(|_| IdentityError)?
                != *path
            || !tokio::fs::metadata(path)
                .await
                .map_err(|_| IdentityError)?
                .is_dir()
        {
            return Err(IdentityError);
        }
    }
    Ok(())
}

pub(super) async fn read_regular(path: &Path) -> Result<(PathBuf, Vec<u8>), IdentityError> {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| IdentityError)?;
    if !tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| IdentityError)?
        .is_file()
    {
        return Err(IdentityError);
    }
    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|_| IdentityError)?;
    if tokio::fs::canonicalize(path)
        .await
        .map_err(|_| IdentityError)?
        != canonical
    {
        return Err(IdentityError);
    }
    Ok((canonical, bytes))
}

pub(super) async fn read_declared(
    repositories: &BTreeMap<String, PathBuf>,
    file: &RepositoryFile,
) -> Result<Vec<u8>, IdentityError> {
    let root = repositories.get(&file.repository).ok_or(IdentityError)?;
    let declared = root.join(file.path.as_str());
    let resolved = tokio::fs::canonicalize(&declared)
        .await
        .map_err(|_| IdentityError)?;
    if !resolved.starts_with(root) {
        return Err(IdentityError);
    }
    let (after, bytes) = read_regular(&declared).await?;
    if after != resolved || !after.starts_with(root) {
        return Err(IdentityError);
    }
    Ok(bytes)
}

pub(super) fn normalize_language(language: &str) -> String {
    match language.to_ascii_lowercase().as_str() {
        "python3" => "python".into(),
        other => other.into(),
    }
}

pub(super) fn exact_version(version: &str) -> bool {
    !version.is_empty()
        && version.as_bytes()[0].is_ascii_digit()
        && !version.contains([' ', '*', '^', '~', '<', '>', '=', '\n', '\r', '\0', ','])
}
