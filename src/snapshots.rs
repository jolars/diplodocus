//! Standalone SQLite snapshots of portable workspace records and asset bytes.
//!
//! Loading never consults source checkouts or executes authored code. Database
//! values remain untrusted until structural and content validation succeeds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::assembly::{AssemblyError, WorkspaceSources};
use crate::configuration::PresentationDefaults;
use crate::ir::{AssetReference, Workspace};
use crate::validation::{ContentAsset, ResolvedDocument, ResolvedWorkspace};

mod canonical;
mod outputs;
mod storage;
mod validation;

pub(crate) fn sqlite_sidecar_paths(path: &Path) -> [PathBuf; 3] {
    ["-journal", "-wal", "-shm"].map(|suffix| {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        PathBuf::from(companion)
    })
}

/// Independent SQLite storage schema version.
pub const STORAGE_SCHEMA_VERSION: u32 = 2;
/// Canonical record encoding version, independent of database layout.
///
/// Version 1 sorts JSON object keys recursively, preserves semantic array
/// order, and hashes compact UTF-8 JSON with SHA-256. See the snapshot schema
/// contract for the record envelope and logical snapshot encoding.
pub const RECORD_ENCODING_VERSION: u32 = 1;

/// A complete portable snapshot, with immutable validated records and bytes.
#[derive(Debug)]
pub struct Snapshot {
    bundle: bool,
    workspace: Workspace,
    presentation: PresentationDefaults,
    pub(crate) package_mounts: BTreeMap<String, String>,
    pub(crate) public_assets: BTreeMap<String, String>,
    logo: Option<AssetReference>,
    logo_dark: Option<AssetReference>,
    header_logo: Option<AssetReference>,
    header_logo_dark: Option<AssetReference>,
    open_graph: Option<AssetReference>,
    components: BTreeMap<String, crate::components::ComponentBundle>,
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
        Self::from_resolved(sources, resolved, false)
    }

    fn from_resolved(
        sources: &WorkspaceSources,
        resolved: &ResolvedWorkspace,
        bundle: bool,
    ) -> Result<Self, SnapshotError> {
        sources.revalidate()?;
        resolved.revalidate()?;
        let mut workspace = sources.workspace().clone();
        workspace.diagnostics = resolved.diagnostics().clone();
        let mut snapshot = Self {
            bundle,
            workspace,
            presentation: resolved.presentation.clone(),
            package_mounts: sources.bundles.mounts.clone(),
            public_assets: resolved.public_assets.clone(),
            logo: resolved.logo().cloned(),
            logo_dark: resolved.logo_dark().cloned(),
            header_logo: resolved.header_logo().cloned(),
            header_logo_dark: resolved.header_logo_dark().cloned(),
            open_graph: resolved.open_graph().cloned(),
            components: resolved.components.clone(),
            documents: resolved.records().to_vec(),
            assets: resolved.assets().clone(),
            producer: env!("CARGO_PKG_VERSION").into(),
            executions: BTreeMap::new(),
            validated_outputs: BTreeMap::new(),
        };
        snapshot.validated_outputs = snapshot.validate()?;
        Ok(snapshot)
    }

    pub(crate) fn from_bundle(
        sources: &WorkspaceSources,
        resolved: &ResolvedWorkspace,
    ) -> Result<Self, SnapshotError> {
        Self::from_resolved(sources, resolved, true)
    }

    pub(crate) fn load_bundle(path: &Path) -> Result<Self, SnapshotError> {
        storage::load_kind(path, true)
    }

    /// Portable semantic records; no runtime checkout paths are stored.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    /// Recorded site defaults, available without the original configuration.
    pub fn presentation(&self) -> &PresentationDefaults {
        &self.presentation
    }
    /// Retained browser component bundles, inert until generation enables them.
    pub fn components(&self) -> &BTreeMap<String, crate::components::ComponentBundle> {
        &self.components
    }
    /// Public files indexed by publication path, with values naming asset digests.
    pub fn public_assets(&self) -> &BTreeMap<String, String> {
        &self.public_assets
    }
    /// Portable main logo, available without the original repository.
    pub fn logo(&self) -> Option<&AssetReference> {
        self.logo.as_ref()
    }
    /// Dark-mode artwork for the main logo.
    pub fn logo_dark(&self) -> Option<&AssetReference> {
        self.logo_dark.as_ref()
    }
    /// Dedicated navbar logo retained with the other portable assets.
    pub fn header_logo(&self) -> Option<&AssetReference> {
        self.header_logo.as_ref()
    }
    /// Dark-mode artwork for the dedicated navbar logo.
    pub fn header_logo_dark(&self) -> Option<&AssetReference> {
        self.header_logo_dark.as_ref()
    }
    /// Portable social preview image, available without the original repository.
    pub fn open_graph(&self) -> Option<&AssetReference> {
        self.open_graph.as_ref()
    }
    /// Diplodocus version that originally produced this snapshot.
    pub fn producer(&self) -> &str {
        &self.producer
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
    /// Errors leave an existing destination untouched. SQLite journal, WAL, or
    /// shared-memory sidecars at the destination prevent replacement.
    ///
    /// The caller must protect declared input paths before selecting the target
    /// and prevent external SQLite writers from modifying it during publication.
    pub fn publish(&self, path: impl AsRef<Path>) -> Result<(), SnapshotError> {
        self.publish_verified(path.as_ref()).map(drop)
    }

    pub(crate) fn publish_verified(&self, path: &Path) -> Result<Self, SnapshotError> {
        self.validate()?;
        storage::publish(self, path)
    }
    /// Canonical readable records and asset bytes, independent of SQLite layout.
    ///
    /// The versioned JSON export includes per-record fingerprints and base64
    /// asset bytes. It uses two-space indentation and one trailing newline.
    /// Comparing exports compares all portable records, including provenance
    /// and producer versions; it does not compare only rendered content.
    pub fn canonical_export(&self) -> Result<String, SnapshotError> {
        canonical::canonical_export(self)
    }
}
