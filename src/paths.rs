//! Resolve explicit filesystem inputs without changing portable declarations.
//!
//! Call [`resolve_workspace_paths`] after parsing or loading a configuration.
//! The resulting absolute paths describe this machine's filesystem at the time
//! of the call. They are runtime data, not serializable documentation IR.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::configuration::WorkspaceConfiguration;

/// Canonical local inputs, retaining the configuration's declaration order.
///
/// These records deliberately do not implement serialization. The corresponding
/// [`WorkspaceConfiguration`] retains the original paths for portable storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspacePaths {
    /// Canonical parent of the caller-supplied configuration path.
    pub configuration_directory: PathBuf,
    /// Repository roots, in repository declaration order.
    pub repositories: Vec<ResolvedRepositoryPaths>,
    /// Package roots and inputs, in package declaration order.
    pub packages: Vec<ResolvedPackagePaths>,
    /// Authored roots and environment inputs, in content declaration order.
    pub content: Vec<ResolvedContentPaths>,
}

/// An explicitly supplied repository's local boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRepositoryPaths {
    /// Repository identifier, retained as declared.
    pub id: String,
    /// Canonical absolute repository directory.
    pub path: PathBuf,
}

/// A package's local boundary and explicit extractor inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackagePaths {
    /// Package identifier, retained as declared.
    pub id: String,
    /// Index of the containing repository in [`ResolvedWorkspacePaths::repositories`].
    pub repository_index: usize,
    /// Canonical absolute package directory, contained in its repository.
    pub path: PathBuf,
    /// Canonical regular metadata file, contained in the package directory.
    pub metadata_path: PathBuf,
    /// Explicit targets, in target declaration order; never inferred.
    pub targets: Vec<ResolvedTargetPath>,
}

/// One explicitly declared source file or directory for extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTargetPath {
    /// Target identifier, retained as declared.
    pub id: String,
    /// Canonical absolute file or directory, contained in its package.
    pub path: PathBuf,
}

/// An authored collection's root and explicitly declared environment files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContentPaths {
    /// Collection identifier, retained as declared.
    pub id: String,
    /// Index of the containing repository in [`ResolvedWorkspacePaths::repositories`].
    pub repository_index: usize,
    /// Canonical absolute content directory, contained in its repository.
    pub path: PathBuf,
    /// Canonical regular files relative to the repository, in declaration order.
    pub declared_environment_inputs: Vec<PathBuf>,
}

/// A filesystem resolution failure tied to a specific configuration field.
#[derive(Debug, Error)]
#[error("could not resolve configuration `{}` field `{field}`: {kind}", configuration_path.display())]
pub struct PathResolutionError {
    /// Configuration path supplied to [`resolve_workspace_paths`].
    pub configuration_path: PathBuf,
    /// Indexed configuration field and declaring ID, or `configuration_directory`.
    pub field: String,
    /// Underlying path or repository-reference failure.
    #[source]
    pub kind: PathResolutionErrorKind,
}

/// The reason a declared filesystem input could not be resolved.
#[derive(Debug, Error)]
pub enum PathResolutionErrorKind {
    /// A path is empty or cannot be interpreted relative to its boundary.
    #[error("invalid path `{}`: {reason}", path.display())]
    InvalidPath {
        /// Path as declared.
        path: PathBuf,
        /// Explanation of the rejected spelling.
        reason: &'static str,
    },
    /// A path could not be inspected or canonicalized.
    #[error("could not inspect `{}`: {source}", path.display())]
    FileSystem {
        /// Attempted path, preserving unresolved components when relevant.
        path: PathBuf,
        /// Underlying filesystem failure, including missing or inaccessible inputs.
        source: std::io::Error,
    },
    /// A path exists but has the wrong filesystem type.
    #[error("expected {expected} at `{}`", path.display())]
    WrongType {
        /// Canonical path with the wrong type.
        path: PathBuf,
        /// Type required by the declaring field.
        expected: PathType,
    },
    /// A resolved component leaves its declared repository or package.
    #[error("path `{}` escapes boundary `{}`", path.display(), boundary.display())]
    OutsideBoundary {
        /// First canonical path outside the boundary.
        path: PathBuf,
        /// Canonical repository or package root that contains this input.
        boundary: PathBuf,
    },
    /// A reference cannot select one unambiguous repository root.
    #[error("repository `{repository}` matches {matches} declarations; expected exactly one")]
    RepositoryReference {
        /// Repository ID referenced by a package or collection.
        repository: String,
        /// Number of matching repository declarations, either zero or more than one.
        matches: usize,
    },
}

/// Filesystem type required by a configuration field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    /// Repository, package, or content root.
    Directory,
    /// Package metadata or declared environment input.
    File,
    /// Explicit extraction target.
    FileOrDirectory,
}

impl fmt::Display for PathType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "a directory",
            Self::File => "a regular file",
            Self::FileOrDirectory => "a regular file or directory",
        })
    }
}

/// Resolve and validate only the filesystem inputs explicitly declared in a workspace.
///
/// Repository paths use the configuration directory as their base. Explicit
/// sibling roots (`../checkout`) and absolute repository roots are permitted.
/// Packages are relative to repositories; metadata and extraction targets are
/// relative to packages; content and environment inputs are relative to
/// repositories. Child declarations must be relative and cannot leave their
/// boundary, including through symlinks or intermediate `..` components that
/// later return. Contained `..` components and symlinks are permitted.
/// Each declared prefix is checked by its canonical referent; intermediate link
/// metadata in a symlink chain does not establish an additional input boundary.
///
/// The parent of `configuration_path` must exist as a directory, but the file
/// itself need not exist. A relative configuration path uses the current working
/// directory. A configuration-file symlink retains the caller-supplied parent
/// as the base, rather than using the target file's directory.
///
/// This does not change declarations, discover descendants, parse file contents,
/// validate owners or relationships, or run code. It checks paths at call time;
/// later discovery and reads must enforce containment again if paths can change.
/// Execution settings and document authority require their separate validators.
///
/// # Errors
///
/// Returns the first invalid path or unresolved/ambiguous repository reference.
/// Order is configuration directory, repository declarations, package declarations
/// (root, metadata, targets), then content declarations (root, environment inputs).
/// Roots must be directories, metadata and environment inputs regular files, and
/// targets regular files or directories. Errors retain the configuration path,
/// indexed field and declaration ID, and underlying filesystem cause when present.
pub fn resolve_workspace_paths(
    configuration_path: impl AsRef<Path>,
    configuration: &WorkspaceConfiguration,
) -> Result<ResolvedWorkspacePaths, PathResolutionError> {
    let resolver = Resolver {
        configuration_path: configuration_path.as_ref(),
    };
    let parent = resolver
        .configuration_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let configuration_directory =
        resolver.resolve_path("configuration_directory", parent, None, None)?;
    resolver.check_type(
        "configuration_directory",
        &configuration_directory,
        PathType::Directory,
    )?;

    let mut repositories = Vec::with_capacity(configuration.repositories.len());
    for (index, repository) in configuration.repositories.iter().enumerate() {
        let field = format!("repository[{index}] (`{}`).path", repository.id);
        let path = resolver.resolve_path(
            &field,
            &repository.path,
            Some(&configuration_directory),
            None,
        )?;
        resolver.check_type(&field, &path, PathType::Directory)?;
        repositories.push(ResolvedRepositoryPaths {
            id: repository.id.clone(),
            path,
        });
    }

    let mut packages = Vec::with_capacity(configuration.packages.len());
    for (index, package) in configuration.packages.iter().enumerate() {
        let field = format!("package[{index}] (`{}`)", package.id);
        let repository_index = resolver.repository_index(
            &format!("{field}.repository"),
            &package.repository,
            &repositories,
        )?;
        let path = resolver.child(
            &format!("{field}.path"),
            &package.path,
            &repositories[repository_index].path,
            PathType::Directory,
        )?;
        let metadata_path = resolver.child(
            &format!("{field}.metadata_path"),
            &package.metadata_path,
            &path,
            PathType::File,
        )?;
        let mut targets = Vec::with_capacity(package.targets.len());
        for (index, target) in package.targets.iter().enumerate() {
            targets.push(ResolvedTargetPath {
                id: target.id.clone(),
                path: resolver.child(
                    &format!("{field}.targets[{index}] (`{}`).path", target.id),
                    &target.path,
                    &path,
                    PathType::FileOrDirectory,
                )?,
            });
        }
        packages.push(ResolvedPackagePaths {
            id: package.id.clone(),
            repository_index,
            path,
            metadata_path,
            targets,
        });
    }

    let mut content = Vec::with_capacity(configuration.content.len());
    for (index, collection) in configuration.content.iter().enumerate() {
        let field = format!("content[{index}] (`{}`)", collection.id);
        let repository_index = resolver.repository_index(
            &format!("{field}.repository"),
            &collection.repository,
            &repositories,
        )?;
        let root = &repositories[repository_index].path;
        let path = resolver.child(
            &format!("{field}.path"),
            &collection.path,
            root,
            PathType::Directory,
        )?;
        let mut declared_environment_inputs = Vec::new();
        for (index, input) in collection
            .execution
            .declared_environment_inputs
            .iter()
            .enumerate()
        {
            declared_environment_inputs.push(resolver.child(
                &format!("{field}.execution.declared_environment_inputs[{index}]"),
                input,
                root,
                PathType::File,
            )?);
        }
        content.push(ResolvedContentPaths {
            id: collection.id.clone(),
            repository_index,
            path,
            declared_environment_inputs,
        });
    }

    Ok(ResolvedWorkspacePaths {
        configuration_directory,
        repositories,
        packages,
        content,
    })
}

struct Resolver<'a> {
    configuration_path: &'a Path,
}

impl Resolver<'_> {
    fn error(&self, field: &str, kind: PathResolutionErrorKind) -> PathResolutionError {
        PathResolutionError {
            configuration_path: self.configuration_path.to_owned(),
            field: field.to_owned(),
            kind,
        }
    }

    fn io_error(&self, field: &str, path: &Path, source: std::io::Error) -> PathResolutionError {
        self.error(
            field,
            PathResolutionErrorKind::FileSystem {
                path: path.to_owned(),
                source,
            },
        )
    }

    fn canonicalize(&self, field: &str, path: &Path) -> Result<PathBuf, PathResolutionError> {
        fs::canonicalize(path).map_err(|source| self.io_error(field, path, source))
    }

    fn check_spelling(
        &self,
        field: &str,
        path: &Path,
        child: bool,
    ) -> Result<(), PathResolutionError> {
        let reason = if path.as_os_str().is_empty() {
            Some("paths must not be empty; use `.` to select a root")
        } else if path
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        {
            if child {
                Some("child paths must be relative to their declared boundary")
            } else if !path.is_absolute() {
                Some("paths must be fully absolute or relative without a drive or root")
            } else {
                None
            }
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(self.error(
                field,
                PathResolutionErrorKind::InvalidPath {
                    path: path.to_owned(),
                    reason,
                },
            ));
        }
        Ok(())
    }

    fn check_type(
        &self,
        field: &str,
        path: &Path,
        expected: PathType,
    ) -> Result<(), PathResolutionError> {
        let metadata = fs::metadata(path).map_err(|source| self.io_error(field, path, source))?;
        let valid = match expected {
            PathType::Directory => metadata.is_dir(),
            PathType::File => metadata.is_file(),
            PathType::FileOrDirectory => metadata.is_file() || metadata.is_dir(),
        };
        if !valid {
            return Err(self.error(
                field,
                PathResolutionErrorKind::WrongType {
                    path: path.to_owned(),
                    expected,
                },
            ));
        }
        Ok(())
    }

    fn child(
        &self,
        field: &str,
        declared: &Path,
        boundary: &Path,
        expected: PathType,
    ) -> Result<PathBuf, PathResolutionError> {
        let resolved = self.resolve_path(field, declared, Some(boundary), Some(boundary))?;
        self.check_type(field, &resolved, expected)?;
        Ok(resolved)
    }

    fn resolve_path(
        &self,
        field: &str,
        declared: &Path,
        base: Option<&Path>,
        boundary: Option<&Path>,
    ) -> Result<PathBuf, PathResolutionError> {
        self.check_spelling(field, declared, boundary.is_some())?;
        let mut components = declared.components().peekable();
        let mut resolved = if declared.is_absolute() {
            let mut root = PathBuf::new();
            while let Some(component) = components
                .next_if(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
            {
                root.push(component);
            }
            self.canonicalize(field, &root)?
        } else if let Some(base) = base {
            base.to_owned()
        } else {
            self.canonicalize(field, Path::new("."))?
        };

        for component in components {
            self.require_directory(field, &resolved)?;
            match component {
                Component::Normal(name) => {
                    let mut next = resolved.clone();
                    if !next
                        .as_os_str()
                        .as_encoded_bytes()
                        .last()
                        .is_some_and(|byte| is_separator(*byte))
                    {
                        next.as_mut_os_string().push(std::path::MAIN_SEPARATOR_STR);
                    }
                    // A normal component can resemble a Windows drive prefix
                    // when parsed alone. Appending it must not change the base.
                    next.as_mut_os_string().push(name);
                    resolved = self.canonicalize(field, &next)?;
                }
                Component::ParentDir => {
                    // Canonical Windows bases use verbatim prefixes, whose joins
                    // erase dot components. Resolve links first, then take the
                    // actual parent without joining an unresolved `..`.
                    resolved.pop();
                }
                Component::CurDir => {}
                Component::Prefix(_) | Component::RootDir => {
                    unreachable!("root components are consumed before traversal");
                }
            }
            if let Some(boundary) = boundary {
                self.check_boundary(field, &resolved, boundary)?;
            }
        }

        // Components omits trailing separators and dots, but their directory
        // requirement must survive on both ordinary and verbatim paths.
        let last = declared
            .as_os_str()
            .as_encoded_bytes()
            .rsplit(|byte| is_separator(*byte))
            .next()
            .unwrap_or_default();
        if last.is_empty() || last == b"." {
            self.require_directory(field, &resolved)?;
        }
        Ok(resolved)
    }

    fn require_directory(&self, field: &str, path: &Path) -> Result<(), PathResolutionError> {
        let metadata = fs::metadata(path).map_err(|source| self.io_error(field, path, source))?;
        if !metadata.is_dir() {
            return Err(self.io_error(field, path, std::io::ErrorKind::NotADirectory.into()));
        }
        Ok(())
    }

    fn check_boundary(
        &self,
        field: &str,
        path: &Path,
        boundary: &Path,
    ) -> Result<(), PathResolutionError> {
        if !path.starts_with(boundary) {
            return Err(self.error(
                field,
                PathResolutionErrorKind::OutsideBoundary {
                    path: path.to_owned(),
                    boundary: boundary.to_owned(),
                },
            ));
        }
        Ok(())
    }

    fn repository_index(
        &self,
        field: &str,
        id: &str,
        repositories: &[ResolvedRepositoryPaths],
    ) -> Result<usize, PathResolutionError> {
        let mut matches = repositories
            .iter()
            .enumerate()
            .filter(|(_, repository)| repository.id == id);
        if let Some((index, _)) = matches.next() {
            let additional = matches.count();
            if additional == 0 {
                return Ok(index);
            }
            return Err(self.error(
                field,
                PathResolutionErrorKind::RepositoryReference {
                    repository: id.to_owned(),
                    matches: additional + 1,
                },
            ));
        }
        Err(self.error(
            field,
            PathResolutionErrorKind::RepositoryReference {
                repository: id.to_owned(),
                matches: 0,
            },
        ))
    }
}

fn is_separator(byte: u8) -> bool {
    byte == b'/' || (cfg!(windows) && byte == b'\\')
}
