//! Fresh source expansion shared by request verification and cache identity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{IdentityError, RepositoryFile, local::read_declared};
use crate::diagnostics::DiagnosticPath;
use crate::documents::{AuthoredFormat, includes};
use crate::ir::IncludeSourceRange;
use crate::paths::ResolvedRepositoryPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceSnapshot {
    pub bytes: Vec<u8>,
    collection_root: Option<PathBuf>,
    pub inputs: BTreeMap<RepositoryFile, Vec<u8>>,
    pub ranges: Vec<IncludeSourceRange>,
}

impl SourceSnapshot {
    pub async fn read(
        repositories: &BTreeMap<String, PathBuf>,
        page: &RepositoryFile,
        collection_root: Option<&Path>,
    ) -> Result<Self, IdentityError> {
        let bytes = read_declared(repositories, page).await?;
        let Some(collection_root) = collection_root else {
            return Ok(Self {
                bytes,
                collection_root: None,
                inputs: BTreeMap::new(),
                ranges: vec![],
            });
        };
        let repository = ResolvedRepositoryPaths {
            id: page.repository.clone(),
            path: repositories
                .get(&page.repository)
                .ok_or(IdentityError)?
                .clone(),
        };
        let page = page.clone();
        let collection_root = collection_root.to_owned();
        // The include resolver parses and reads files synchronously. Keep that
        // work off the async executor while retaining the same boundary rules.
        tokio::task::spawn_blocking(move || {
            let root = std::fs::canonicalize(&collection_root).map_err(|_| IdentityError)?;
            if root != collection_root || !root.starts_with(&repository.path) || !root.is_dir() {
                return Err(IdentityError);
            }
            let path = repository.path.join(page.path.as_str());
            let location = repository
                .source_location(&path, None)
                .map_err(|_| IdentityError)?;
            if location.path != page.path {
                return Err(IdentityError);
            }
            let expanded = includes::expand(
                std::str::from_utf8(&bytes).map_err(|_| IdentityError)?,
                &path,
                &root,
                &repository,
                AuthoredFormat::Qmd,
            )
            .map_err(|_| IdentityError)?;
            let mut inputs = BTreeMap::new();
            for (location, content) in expanded.inputs {
                let file = RepositoryFile {
                    repository: location.repository,
                    path: location.path,
                };
                let bytes = content.into_bytes();
                if inputs
                    .insert(file, bytes.clone())
                    .is_some_and(|old| old != bytes)
                {
                    return Err(IdentityError);
                }
            }
            Ok(Self {
                bytes: expanded.text.into_bytes(),
                collection_root: Some(root),
                inputs,
                ranges: expanded.ranges,
            })
        })
        .await
        .map_err(|_| IdentityError)?
    }

    pub async fn revalidate(
        &self,
        repositories: &BTreeMap<String, PathBuf>,
        page: &RepositoryFile,
    ) -> Result<(), IdentityError> {
        let current = Self::read(repositories, page, self.collection_root.as_deref()).await?;
        if current != *self {
            return Err(IdentityError);
        }
        Ok(())
    }

    pub fn has_includes(&self) -> bool {
        self.ranges
            .iter()
            .any(|range| !range.include_chain.is_empty())
    }

    pub fn collection_root(
        &self,
        repository: &Path,
    ) -> Result<Option<DiagnosticPath>, IdentityError> {
        let root = self.collection_root.as_ref().ok_or(IdentityError)?;
        let relative = root.strip_prefix(repository).map_err(|_| IdentityError)?;
        if relative.as_os_str().is_empty() {
            Ok(None)
        } else {
            DiagnosticPath::try_from(relative.to_str().ok_or(IdentityError)?)
                .map(Some)
                .map_err(|_| IdentityError)
        }
    }
}
