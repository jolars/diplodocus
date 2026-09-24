//! Standalone SQLite snapshots of portable workspace records and asset bytes.
//!
//! Loading never consults source checkouts or executes authored code. Database
//! values remain untrusted until structural and content validation succeeds.

use std::collections::BTreeMap;
use std::path::Path;

use crate::assembly::{AssemblyError, WorkspaceSources};
use crate::ir::Workspace;
use crate::validation::{ContentAsset, ResolvedDocument, ResolvedWorkspace};

mod outputs;
mod storage;
mod validation;

/// Independent SQLite storage schema version.
pub const STORAGE_SCHEMA_VERSION: u32 = 1;
/// Canonical record encoding version, independent of database layout.
pub const RECORD_ENCODING_VERSION: u32 = 1;

/// A complete portable snapshot, with immutable validated records and bytes.
#[derive(Debug)]
pub struct Snapshot {
    workspace: Workspace,
    documents: Vec<ResolvedDocument>,
    assets: BTreeMap<String, ContentAsset>,
    producer: String,
    executions: BTreeMap<String, outputs::StoredPage>,
    validated_outputs: BTreeMap<String, crate::execution::ValidatedPage>,
}

/// Snapshot storage or validation failed without a publishable replacement.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Input observations changed before the portable copy was made.
    #[error(transparent)]
    Inputs(#[from] AssemblyError),
    /// Snapshot filesystem access failed.
    #[error("snapshot I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// The database could not be read or written.
    #[error("snapshot database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    /// A record failed typed decoding or encoding.
    #[error("snapshot record encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    /// The reader cannot interpret this schema or encoding.
    #[error("unsupported snapshot storage, IR, or record encoding version")]
    Version,
    /// A required record, identity, reference, or asset is invalid.
    #[error("invalid snapshot: {0}")]
    Invalid(&'static str),
}

impl Snapshot {
    /// Copy statically assembled and resolved records while rechecking inputs.
    ///
    /// This operation does not execute eligible cells. Commands that authorize
    /// execution must first run the workspace execution operation.
    pub fn from_sources(
        sources: &WorkspaceSources,
        resolved: &ResolvedWorkspace,
    ) -> Result<Self, SnapshotError> {
        sources.revalidate()?;
        resolved.revalidate()?;
        let mut workspace = sources.workspace().clone();
        workspace.diagnostics = resolved.diagnostics().clone();
        let mut snapshot = Self {
            workspace,
            documents: resolved.records().to_vec(),
            assets: resolved.assets().clone(),
            producer: env!("CARGO_PKG_VERSION").into(),
            executions: BTreeMap::new(),
            validated_outputs: BTreeMap::new(),
        };
        snapshot.validated_outputs = snapshot.validate()?;
        Ok(snapshot)
    }

    /// Portable semantic records; no runtime checkout paths are stored.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    /// Resolved references and authored anchor sets in semantic document order.
    pub fn documents(&self) -> &[ResolvedDocument] {
        &self.documents
    }
    /// Complete content bytes, indexed by their SHA-256 digest.
    pub fn assets(&self) -> &BTreeMap<String, ContentAsset> {
        &self.assets
    }
    /// Load and validate a standalone database using a read-only connection.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        storage::load(path.as_ref())
    }
    /// Atomically replace a snapshot with a completed, closed database.
    ///
    /// The caller must protect declared input paths before selecting the target.
    /// A failure before replacement leaves an existing destination untouched.
    pub fn publish(&self, path: impl AsRef<Path>) -> Result<(), SnapshotError> {
        self.validate()?;
        storage::publish(self, path.as_ref())
    }
    /// Canonical readable records and asset bytes, independent of SQLite layout.
    pub fn canonical_export(&self) -> Result<String, SnapshotError> {
        storage::canonical_export(self)
    }
}
