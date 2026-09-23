//! Validated figures and page-scoped staging, independent of kernel transport.
//!
//! Portable names are `execution-assets/<page>/sha256/<content>`. The page hash
//! covers a domain separator and length-prefixed repository ID, collection ID,
//! and normalized source path. Neither hash includes a local filesystem path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use image::ImageFormat;
use tempfile::TempDir;
use thiserror::Error;

use super::{
    ExecutionAsset, ExecutionFailure, ExecutionFailureKind, ExecutionPage, StagedExecutionAsset,
};
use crate::diagnostics::{DiagnosticCode, DiagnosticPath};
use crate::ir::{AssetReference, Fingerprint};
use crate::provenance::fingerprint_bytes;

mod files;
mod raster;
mod svg;
#[cfg(test)]
mod tests;

/// Portable asset rejection or failure, without local paths or OS error strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AssetError {
    /// This media type is outside the image policy.
    #[error("The image media type is unsupported.")]
    UnsupportedMedia,
    /// The supported payload is malformed or contradicts its declared metadata.
    #[error("The image payload does not match its format or metadata.")]
    InvalidMedia,
    /// The XML or its contents fail the inert SVG allowlist.
    #[error("The SVG does not satisfy the inert image policy.")]
    UnsafeSvg,
    /// A path escapes its boundary, is absolute, or names a nonregular file.
    #[error("The execution asset is outside its declared boundary.")]
    OutsideBoundary,
    /// A generated source file is absent.
    #[error("The generated asset is missing.")]
    Missing,
    /// One digest identifies different bytes or media metadata.
    #[error("Execution assets conflict at the same content digest.")]
    Collision,
    /// A contained asset could not be read or staged.
    #[error("The execution asset could not be read or staged.")]
    Storage,
    /// Owned staging could not be removed safely.
    #[error("Execution asset staging could not be removed.")]
    Cleanup,
}

impl AssetError {
    /// Fatal failures stop execution even when another MIME alternative exists.
    /// Candidate rejections return `None` and permit a safe alternative.
    pub const fn failure_kind(self) -> Option<ExecutionFailureKind> {
        match self {
            Self::UnsupportedMedia | Self::InvalidMedia | Self::UnsafeSvg => None,
            Self::OutsideBoundary => Some(ExecutionFailureKind::AssetOutsideBoundary),
            Self::Missing => Some(ExecutionFailureKind::AssetMissing),
            Self::Collision => Some(ExecutionFailureKind::AssetCollision),
            Self::Storage => Some(ExecutionFailureKind::OutputValidation),
            Self::Cleanup => Some(ExecutionFailureKind::Cleanup),
        }
    }

    /// Stable diagnostic for the producing cell or page.
    pub const fn diagnostic_code(self) -> DiagnosticCode {
        match self {
            Self::UnsupportedMedia => DiagnosticCode::UnsupportedCellOutput,
            Self::InvalidMedia => DiagnosticCode::InvalidCellOutput,
            Self::UnsafeSvg => DiagnosticCode::UnsafeKernelSvg,
            other => match other.failure_kind() {
                Some(kind) => kind.diagnostic_code(),
                None => unreachable!(),
            },
        }
    }
}

/// Validate exact image bytes without filesystem access or staging.
///
/// PNG and JPEG must match their signatures and decode completely. SVG remains
/// XML stored as an image, never markup for insertion into an HTML document.
pub fn validate_image_bytes(media_type: &str, bytes: &[u8]) -> Result<(), AssetError> {
    let format = match media_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/svg+xml" => return svg::validate(bytes),
        _ => return Err(AssetError::UnsupportedMedia),
    };
    if image::guess_format(bytes).ok() != Some(format) {
        return Err(AssetError::InvalidMedia);
    }
    raster::validate(format, bytes)
}

/// Validate checked-in image bytes, retaining inert SVG accessibility metadata.
///
/// Authored SVG may label its title and description with IDs and ARIA attributes.
/// The generated-image policy remains unchanged; neither policy accepts scripts,
/// external references, style attributes, or embedded HTML.
pub fn validate_authored_image_bytes(media_type: &str, bytes: &[u8]) -> Result<(), AssetError> {
    if media_type == "image/svg+xml" {
        svg::validate_authored(bytes)
    } else {
        validate_image_bytes(media_type, bytes)
    }
}

/// A single page's uncommitted figures. Dropping the owner discards its files.
///
/// Construction creates no directories. Each first accepted image creates a
/// private child of the supplied staging boundary. Fatal errors poison the owner
/// so catching a failure cannot turn the attempt into a successful retention.
pub struct PageAssetStore {
    page: ExecutionPage,
    repository: PathBuf,
    boundary: PathBuf,
    namespace: String,
    directory: Option<TempDir>,
    directory_identity: Option<files::DirectoryIdentity>,
    assets: BTreeMap<String, ExecutionAsset>,
    failed: Option<AssetError>,
    digest: fn(&[u8]) -> Fingerprint,
}

/// Assets transferred after a successful page, in increasing digest order.
#[derive(Debug)]
pub struct RetainedExecutionAssets {
    /// Portable entries for the page record.
    pub assets: Vec<ExecutionAsset>,
    /// Local handles in the same order as `assets`.
    ///
    /// The caller owns these files and their private parent directory and must
    /// publish or discard them. The shared staging boundary remains caller-owned.
    pub staged_assets: Vec<StagedExecutionAsset>,
}

impl PageAssetStore {
    /// Check final metadata and bytes without transferring the staging owner.
    pub(crate) fn verify_assets(
        &mut self,
        page: &ExecutionPage,
        expected: &[ExecutionAsset],
    ) -> Result<(), AssetError> {
        let result = (|| {
            self.require_healthy()?;
            if page != &self.page {
                return Err(AssetError::Collision);
            }
            if expected.windows(2).any(|pair| {
                pair[0].reference.fingerprint.value >= pair[1].reference.fingerprint.value
            }) {
                return Err(AssetError::Collision);
            }
            for asset in expected {
                if self.assets.get(&asset.reference.fingerprint.value) != Some(asset) {
                    return Err(AssetError::Collision);
                }
            }
            if expected.is_empty() {
                return Ok(());
            }
            let directory = self.staging_directory()?;
            for asset in expected {
                let bytes = files::read_staged(
                    &directory.join(&asset.reference.fingerprint.value),
                    &directory,
                )?;
                if fingerprint_bytes(&bytes) != asset.reference.fingerprint
                    || bytes.len() as u64 != asset.byte_size
                {
                    return Err(AssetError::Collision);
                }
                validate_image_bytes(&asset.media_type, &bytes)?;
            }
            Ok(())
        })();
        self.remember(result)
    }

    /// Prepare an owner using a canonical repository and absolute output boundary.
    /// The page's normalized source path supplies its working directory and identity.
    pub fn new(
        page: ExecutionPage,
        repository: PathBuf,
        boundary: PathBuf,
    ) -> Result<Self, AssetError> {
        if !repository.is_absolute()
            || fs::canonicalize(&repository).map_err(files::input_error)? != repository
            || !repository.is_dir()
            || !files::absolute_normalized(&boundary)
        {
            return Err(AssetError::OutsideBoundary);
        }
        let mut identity = b"diplodocus/execution-assets-v1\0".to_vec();
        for field in [
            &page.source.repository,
            &page.collection,
            page.source.path.as_str(),
        ] {
            identity.extend_from_slice(&(field.len() as u64).to_be_bytes());
            identity.extend_from_slice(field.as_bytes());
        }
        let namespace = fingerprint_bytes(&identity).value;
        Ok(Self {
            page,
            repository,
            boundary,
            namespace,
            directory: None,
            directory_identity: None,
            assets: BTreeMap::new(),
            failed: None,
            digest: fingerprint_bytes,
        })
    }

    /// Validate and stage an inline or already-decoded image without a source path.
    pub fn stage_bytes(
        &mut self,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<ExecutionAsset, AssetError> {
        let result = self.stage(media_type, bytes);
        self.remember(result)
    }

    /// Read a generated image relative to the authored page, then validate and copy it.
    /// URL escapes are decoded before path validation; filenames never select media.
    pub fn stage_local(&mut self, target: &str) -> Result<ExecutionAsset, AssetError> {
        let result = (|| {
            self.require_healthy()?;
            let bytes = files::read_local(&self.repository, &self.page.source.path, target)?;
            let media_type = match image::guess_format(&bytes) {
                Ok(ImageFormat::Png) => "image/png",
                Ok(ImageFormat::Jpeg) => "image/jpeg",
                _ if std::str::from_utf8(&bytes).is_ok_and(|s| {
                    s.trim_start_matches('\u{feff}')
                        .trim_start()
                        .starts_with('<')
                }) =>
                {
                    "image/svg+xml"
                }
                _ => return Err(AssetError::UnsupportedMedia),
            };
            self.stage(media_type, &bytes)
        })();
        self.remember(result)
    }

    /// Restore verified bytes without looking up a kernel-generated source path.
    /// The complete digest, media, size, and page namespace must match the record.
    pub fn stage_cached(
        &mut self,
        expected: &ExecutionAsset,
        bytes: &[u8],
    ) -> Result<ExecutionAsset, AssetError> {
        let result = (|| {
            self.require_healthy()?;
            let digest = fingerprint_bytes(bytes);
            if expected.reference != self.reference(digest)
                || expected.byte_size != bytes.len() as u64
            {
                return Err(AssetError::InvalidMedia);
            }
            self.stage(&expected.media_type, bytes)
        })();
        self.remember(result)
    }

    /// Discard all private staging and report cleanup failures explicitly.
    /// Engines attach this failure to the primary execution failure, if any.
    pub fn rollback(mut self) -> Result<(), AssetError> {
        self.discard()
    }

    /// Retain exactly the final references and transfer ownership to the caller.
    ///
    /// Call only after kernel cleanup, input revalidation, and final output
    /// validation. Repeated references are deduplicated. Missing, altered, or
    /// foreign references fail the operation and discard owned staging.
    pub fn retain(
        mut self,
        references: &[AssetReference],
    ) -> Result<RetainedExecutionAssets, ExecutionFailure> {
        let result = self.retain_inner(references);
        result.map_err(|error| {
            let kind = error
                .failure_kind()
                .unwrap_or(ExecutionFailureKind::OutputValidation);
            let mut failure = ExecutionFailure {
                kind,
                diagnostics: vec![
                    kind.to_diagnostic(&self.page.collection, self.page.source.clone()),
                ],
                cleanup_diagnostics: Vec::new(),
            };
            if self.discard().is_err() {
                failure.cleanup_diagnostics.push(
                    ExecutionFailureKind::Cleanup
                        .to_diagnostic(&self.page.collection, self.page.source.clone()),
                );
            }
            failure
        })
    }

    fn require_healthy(&self) -> Result<(), AssetError> {
        self.failed.map_or(Ok(()), Err)
    }

    fn remember<T>(&mut self, result: Result<T, AssetError>) -> Result<T, AssetError> {
        if let Err(error) = result.as_ref()
            && error.failure_kind().is_some()
        {
            self.failed.get_or_insert(*error);
        }
        result
    }

    fn reference(&self, fingerprint: Fingerprint) -> AssetReference {
        AssetReference {
            path: DiagnosticPath::try_from(format!(
                "execution-assets/{}/sha256/{}",
                self.namespace, fingerprint.value
            ))
            .expect("digest-derived portable path"),
            fingerprint,
        }
    }

    fn stage(&mut self, media_type: &str, bytes: &[u8]) -> Result<ExecutionAsset, AssetError> {
        self.require_healthy()?;
        validate_image_bytes(media_type, bytes)?;
        let fingerprint = (self.digest)(bytes);
        let asset = ExecutionAsset {
            reference: self.reference(fingerprint.clone()),
            media_type: media_type.into(),
            byte_size: bytes.len() as u64,
        };
        let directory = self.staging_directory()?;
        let path = directory.join(&fingerprint.value);
        if let Some(previous) = self.assets.get(&fingerprint.value) {
            if previous != &asset || files::read_staged(&path, &directory)? != bytes {
                return Err(AssetError::Collision);
            }
            return Ok(previous.clone());
        }
        // A private directory has no legitimate unregistered files. Never
        // overwrite an existing name, including a symlink or injected file.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AssetError::Collision
                } else {
                    AssetError::Storage
                }
            })?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| AssetError::Storage)?;
        self.assets.insert(fingerprint.value, asset.clone());
        Ok(asset)
    }

    fn staging_directory(&mut self) -> Result<PathBuf, AssetError> {
        files::directory(&self.boundary, self.directory.is_none())?;
        if self.directory.is_none() {
            self.directory = Some(
                tempfile::Builder::new()
                    .prefix(".diplodocus-execution-")
                    .tempdir_in(&self.boundary)
                    .map_err(|_| AssetError::Storage)?,
            );
            self.directory_identity = Some(files::DirectoryIdentity::capture(
                self.directory.as_ref().expect("created staging").path(),
            )?);
        }
        let path = self.directory.as_ref().expect("created staging").path();
        files::directory(path, false)?;
        if !self
            .directory_identity
            .as_ref()
            .is_some_and(|identity| identity.matches(path))
        {
            return Err(AssetError::OutsideBoundary);
        }
        Ok(path.to_owned())
    }

    fn retain_inner(
        &mut self,
        references: &[AssetReference],
    ) -> Result<RetainedExecutionAssets, AssetError> {
        self.require_healthy()?;
        let mut retained = BTreeSet::new();
        for reference in references {
            let digest = &reference.fingerprint.value;
            if self
                .assets
                .get(digest)
                .is_none_or(|asset| &asset.reference != reference)
            {
                return Err(AssetError::Collision);
            }
            retained.insert(digest.clone());
        }
        let mut result = RetainedExecutionAssets {
            assets: Vec::new(),
            staged_assets: Vec::new(),
        };
        if self.directory.is_none() {
            return Ok(result);
        }
        let directory = self.staging_directory()?;
        for (digest, asset) in &self.assets {
            let path = directory.join(digest);
            if retained.contains(digest) {
                let bytes = files::read_staged(&path, &directory)?;
                if (self.digest)(&bytes) != asset.reference.fingerprint
                    || bytes.len() as u64 != asset.byte_size
                {
                    return Err(AssetError::Collision);
                }
                validate_image_bytes(&asset.media_type, &bytes)?;
                result.assets.push(asset.clone());
                result.staged_assets.push(StagedExecutionAsset {
                    reference: asset.reference.clone(),
                    path,
                });
            } else {
                fs::remove_file(path).map_err(|_| AssetError::Cleanup)?;
            }
        }
        // Files that were never accepted must not survive a successful transfer.
        if fs::read_dir(&directory)
            .map_err(|_| AssetError::Storage)?
            .count()
            != retained.len()
        {
            return Err(AssetError::Collision);
        }
        if retained.is_empty() {
            self.discard()?;
        } else {
            let _ = self.directory.take().expect("owned staging").keep();
        }
        Ok(result)
    }

    fn discard(&mut self) -> Result<(), AssetError> {
        if let Some(directory) = self.directory.take() {
            if files::directory(directory.path(), false).is_err()
                || !self
                    .directory_identity
                    .as_ref()
                    .is_some_and(|identity| identity.matches(directory.path()))
            {
                // An ancestor may have been replaced. Do not let TempDir's
                // destructor follow it into a different directory tree.
                let _ = directory.keep();
                return Err(AssetError::Cleanup);
            }
            directory.close().map_err(|_| AssetError::Cleanup)?;
        }
        Ok(())
    }
}

impl Drop for PageAssetStore {
    fn drop(&mut self) {
        let _ = self.discard();
    }
}
