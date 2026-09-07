//! Testable operations invoked by the command-line interface.

use std::net::IpAddr;
use std::path::PathBuf;

use thiserror::Error;

/// Options for checking a documentation workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
}

/// Options for building a static documentation site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
    /// Directory in which to write the generated site.
    pub output: PathBuf,
}

/// Options for building and serving a documentation site locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// Path to the workspace configuration file.
    pub config: PathBuf,
    /// Directory in which to write the generated site.
    pub output: PathBuf,
    /// Address on which to serve the generated site.
    pub host: IpAddr,
    /// Port on which to serve the generated site.
    pub port: u16,
}

/// An error returned by a Diplodocus command operation.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandError {
    /// Building is not available in the infrastructure milestone.
    #[error("`diplodocus build` is not implemented yet")]
    BuildNotImplemented,
    /// Checking is not available in the infrastructure milestone.
    #[error("`diplodocus check` is not implemented yet")]
    CheckNotImplemented,
    /// Serving is not available in the infrastructure milestone.
    #[error("`diplodocus serve` is not implemented yet")]
    ServeNotImplemented,
}

/// Build a static documentation site.
pub fn build(_options: BuildOptions) -> Result<(), CommandError> {
    Err(CommandError::BuildNotImplemented)
}

/// Check a documentation workspace without writing a site.
pub fn check(_options: CheckOptions) -> Result<(), CommandError> {
    Err(CommandError::CheckNotImplemented)
}

/// Build and serve a documentation site locally.
pub fn serve(_options: ServeOptions) -> Result<(), CommandError> {
    Err(CommandError::ServeNotImplemented)
}
