//! Active validation of generated content, independent of transport and cache storage.
//!
//! Portable decoded content carries no trust. Only these validators construct the
//! immutable wrappers, whose image bindings refer to actively validated assets.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::DiagnosticSource;
use crate::ir::{Fingerprint, SourceLocation, SourceSpan};

use super::assets::{AssetError, PageAssetStore};
use super::{ExecutionAsset, ExecutionFailure, ExecutionFailureKind};

mod diagnostics;
mod fragment;
mod html;
mod markdown;
mod urls;
mod warnings;

pub use diagnostics::*;
pub use fragment::*;
pub use html::{
    DecodedHtml, HtmlAttribute, HtmlNode, ValidatedHtml, restore_html, validate_html_live,
};
pub use markdown::{
    ImageBinding, NodeAddress, NodeEdge, ValidatedMarkdown, restore_markdown,
    validate_markdown_live,
};

/// Authored page identity and anchors captured by successful preparation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredOutputContext {
    source: SourceLocation,
    collection: String,
    anchors: BTreeSet<String>,
}

impl AuthoredOutputContext {
    /// Capture prepared authored anchors. Generated output must never extend them.
    pub fn new(mut source: SourceLocation, collection: String, anchors: BTreeSet<String>) -> Self {
        source.span = None;
        Self {
            source,
            collection,
            anchors,
        }
    }

    /// The normalized repository-relative authored page location.
    pub fn source(&self) -> &SourceLocation {
        &self.source
    }
    /// Owning content collection.
    pub fn collection(&self) -> &str {
        &self.collection
    }
    /// Complete prepared authored anchor set.
    pub fn anchors(&self) -> &BTreeSet<String> {
        &self.anchors
    }

    fn diagnostic_source(&self) -> DiagnosticSource {
        DiagnosticSource::Repository {
            repository: self.source.repository.clone(),
            path: self.source.path.clone(),
        }
    }

    fn owns_asset(&self, asset: &ExecutionAsset) -> bool {
        let mut identity = b"diplodocus/execution-assets-v1\0".to_vec();
        for field in [
            self.source.repository.as_str(),
            self.collection.as_str(),
            self.source.path.as_str(),
        ] {
            identity.extend_from_slice(&(field.len() as u64).to_be_bytes());
            identity.extend_from_slice(field.as_bytes());
        }
        let namespace = crate::provenance::fingerprint_bytes(&identity).value;
        valid_digest(&asset.reference.fingerprint)
            && asset.reference.path.as_str()
                == format!(
                    "execution-assets/{namespace}/sha256/{}",
                    asset.reference.fingerprint.value
                )
    }
}

/// Identity allocated while the producing fragment's exact bytes are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentIdentity {
    /// Monotonic per-producing-cell ordinal, including replaced outputs.
    pub ordinal: usize,
    /// Original UTF-8 byte length, independent of the surviving tree.
    pub byte_length: usize,
}

/// Producing cell and output slot; never a final owning-slot authority key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputOrigin {
    /// Producing cell ordinal.
    pub cell: usize,
    /// Producer slot ordinal.
    pub slot: usize,
    /// Authored cell range, distinct from fragment-relative ranges.
    pub cell_span: SourceSpan,
    /// Present only for generated Markdown.
    pub fragment: Option<FragmentIdentity>,
}

/// One accepted alternative or a safe rejection that permits MIME fallback.
#[derive(Debug)]
pub enum Validation<T> {
    /// Accepted content and its ordered producing warnings.
    Accepted {
        /// Immutable validated value.
        value: T,
        /// Warnings retained even if this alternative is hidden or unselected.
        diagnostics: Vec<ExecutionDiagnostic>,
    },
    /// No trusted content was created.
    Rejected {
        /// Specific warnings; a reducer must not add a redundant generic warning.
        diagnostics: Vec<ExecutionDiagnostic>,
    },
}

/// Untrusted, path-independent image metadata used in canonical content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetUse {
    /// SHA-256 of the exact image bytes.
    pub digest: Fingerprint,
    /// Validated image MIME type.
    pub media_type: String,
    /// Exact byte length.
    pub byte_size: u64,
}

impl From<&ExecutionAsset> for AssetUse {
    fn from(asset: &ExecutionAsset) -> Self {
        Self {
            digest: asset.reference.fingerprint.clone(),
            media_type: asset.media_type.clone(),
            byte_size: asset.byte_size,
        }
    }
}

/// Active cache-restore rejection; a rejected alternative rejects the whole candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RestoreRejection {
    /// Invalid fragment shape, ranges, or attribution.
    #[error("Invalid generated-content structure.")]
    Structure,
    /// A URL fails the active output policy.
    #[error("Unsafe generated-content URL.")]
    Url,
    /// An image has no actively verified staged bytes.
    #[error("Generated content references an unverified asset.")]
    UnboundAsset,
    /// Image digest, media, or size contradicts the verified table.
    #[error("Generated-content asset metadata does not match.")]
    AssetMismatch,
    /// Accepted markup differs from the canonical spelling on disk.
    #[error("Generated content is not canonical.")]
    NonCanonical,
}

/// Image records obtainable only by validating and staging exact bytes.
///
/// ```compile_fail
/// use diplodocus::execution::output_safety::VerifiedAssets;
/// let _: VerifiedAssets = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Default)]
pub struct VerifiedAssets {
    assets: BTreeMap<String, ExecutionAsset>,
}

impl VerifiedAssets {
    /// Start an empty table; this grants no trust to any image.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate digest, namespace, media, size, and bytes through the staging owner.
    pub fn stage(
        &mut self,
        store: &mut PageAssetStore,
        expected: &ExecutionAsset,
        bytes: &[u8],
    ) -> Result<(), AssetError> {
        let asset = store.stage_cached(expected, bytes)?;
        if self
            .assets
            .get(&asset.reference.fingerprint.value)
            .is_some_and(|old| old != &asset)
        {
            return Err(AssetError::Collision);
        }
        self.assets
            .insert(asset.reference.fingerprint.value.clone(), asset);
        Ok(())
    }

    /// Verified immutable records in digest order.
    pub fn assets(&self) -> impl Iterator<Item = &ExecutionAsset> {
        self.assets.values()
    }

    fn resolve(
        &self,
        usage: &AssetUse,
        context: &AuthoredOutputContext,
    ) -> Result<ExecutionAsset, RestoreRejection> {
        if !valid_digest(&usage.digest) {
            return Err(RestoreRejection::AssetMismatch);
        }
        let asset = self
            .assets
            .get(&usage.digest.value)
            .ok_or(RestoreRejection::UnboundAsset)?;
        if AssetUse::from(asset) != *usage || !context.owns_asset(asset) {
            return Err(RestoreRejection::AssetMismatch);
        }
        Ok(asset.clone())
    }
}

fn valid_digest(digest: &Fingerprint) -> bool {
    digest.algorithm == "sha256"
        && digest.value.len() == 64
        && digest
            .value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn failure(
    kind: ExecutionFailureKind,
    context: &AuthoredOutputContext,
    origin: &OutputOrigin,
) -> ExecutionFailure {
    ExecutionFailure {
        kind,
        diagnostics: vec![kind.to_diagnostic(
            context.collection.clone(),
            SourceLocation {
                span: Some(origin.cell_span),
                ..context.source.clone()
            },
        )],
        cleanup_diagnostics: Vec::new(),
    }
}
