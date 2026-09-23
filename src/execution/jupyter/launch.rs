//! Bind discovery to the immutable launch observation consumed by the supervisor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::FailureSource;
use super::discovery::SelectedKernel;
use crate::diagnostics::Diagnostic;
use crate::execution::identity::{LaunchIdentityInput, LaunchResolverInput};
use crate::execution::{ExecutionFailure, ExecutionFailureKind};
use crate::provenance::fingerprint_bytes;

pub(super) struct ResolvedKernel {
    kernel: SelectedKernel,
    launch: LaunchIdentityInput,
}

impl ResolvedKernel {
    pub async fn resolve(
        kernel: SelectedKernel,
        repositories: BTreeMap<String, PathBuf>,
        repository_root: &Path,
        page_path: &Path,
        source: &FailureSource,
    ) -> Result<Self, ExecutionFailure> {
        let diagnostics = kernel.diagnostics.clone();
        Self::resolve_inner(kernel, repositories, repository_root, page_path, source)
            .await
            .map_err(|failure| with_discovery(failure, &diagnostics))
    }

    async fn resolve_inner(
        kernel: SelectedKernel,
        repositories: BTreeMap<String, PathBuf>,
        repository_root: &Path,
        page_path: &Path,
        source: &FailureSource,
    ) -> Result<Self, ExecutionFailure> {
        let startup = || {
            source.failure(
                ExecutionFailureKind::Startup,
                "The authored page must be a canonical file within its declared repository.",
            )
        };
        let root = repositories
            .get(&source.source.repository)
            .ok_or_else(startup)?;
        let page = tokio::fs::canonicalize(root.join(source.source.path.as_str()))
            .await
            .map_err(|_| startup())?;
        if root != repository_root
            || page != page_path
            || !page.starts_with(root)
            || !tokio::fs::metadata(&page)
                .await
                .map_err(|_| startup())?
                .is_file()
        {
            return Err(startup());
        }
        let unchanged = || async {
            let path = kernel.directory.join("kernel.json");
            if !tokio::fs::metadata(&path)
                .await
                .map_err(|_| changed(source))?
                .is_file()
            {
                return Err(changed(source));
            }
            let bytes = tokio::fs::read(path).await.map_err(|_| changed(source))?;
            if fingerprint_bytes(&bytes) != kernel.spec_observation {
                return Err(changed(source));
            }
            Ok(())
        };
        unchanged().await?;
        let working_directory = Path::new(source.source.path.as_str())
            .parent()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(".");
        let launch = LaunchIdentityInput::resolve(LaunchResolverInput {
            repositories,
            spec_path: kernel.directory.join("kernel.json"),
            selector: kernel.name.clone(),
            search: kernel.search.clone(),
            executable_search_path: kernel.executable_path.clone().unwrap_or_default(),
            repository: source.source.repository.clone(),
            working_directory: working_directory.into(),
        })
        .await
        .map_err(|_| {
            source.failure(
                ExecutionFailureKind::Startup,
                "The selected kernel launch could not be resolved and validated.",
            )
        })?;
        unchanged().await?;
        if Some(launch.working_directory()) != page.parent()
            || launch.language() != kernel.language
            || launch.interrupt_mode() != kernel.interrupt_mode
            || launch.environment() != &kernel.env
        {
            return Err(changed(source));
        }
        Ok(Self { kernel, launch })
    }

    pub fn identity(&self) -> &LaunchIdentityInput {
        &self.launch
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.kernel.diagnostics
    }

    pub fn into_parts(self) -> (SelectedKernel, LaunchIdentityInput) {
        (self.kernel, self.launch)
    }
}

pub(super) fn with_discovery(
    mut failure: ExecutionFailure,
    diagnostics: &[Diagnostic],
) -> ExecutionFailure {
    failure
        .diagnostics
        .splice(0..0, diagnostics.iter().cloned());
    failure
}

fn changed(source: &FailureSource) -> ExecutionFailure {
    source.failure(
        ExecutionFailureKind::InputChanged,
        "The selected kernel changed after discovery.",
    )
}
