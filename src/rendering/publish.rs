use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// A generated response, with a content type independent of the source filename.
#[derive(Debug, Clone)]
pub struct RenderedFile {
    /// Exact response and publication bytes.
    pub bytes: Vec<u8>,
    /// Fixed media type selected by generation.
    pub media_type: String,
}
/// One complete output tree. A server can retain it while the next build runs.
#[derive(Debug)]
pub struct RenderedSite {
    pub(super) files: BTreeMap<String, RenderedFile>,
}
impl RenderedSite {
    /// Files keyed by validated relative output path.
    pub fn files(&self) -> &BTreeMap<String, RenderedFile> {
        &self.files
    }
    /// Stage a sibling tree and publish it only after every file is written.
    /// Existing sites are replaced in one filesystem operation on Linux.
    pub fn publish(&self, output: &Path) -> Result<(), crate::site::SiteError> {
        use crate::site::SiteError;
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        if let Ok(metadata) = fs::symlink_metadata(output) {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(SiteError::UnownedOutput);
            }
            if fs::read_dir(output)?.next().is_some() && !output.join(".diplodocus-site").is_file()
            {
                return Err(SiteError::UnownedOutput);
            }
        }
        let stage = tempfile::Builder::new()
            .prefix(".diplodocus-site-")
            .tempdir_in(parent)?;
        for (path, file) in &self.files {
            let path = stage.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &file.bytes)?;
        }
        fs::write(
            stage.path().join(".diplodocus-site"),
            b"diplodocus-site-v1\n",
        )?;
        if output.exists() {
            #[cfg(target_os = "linux")]
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                stage.path(),
                rustix::fs::CWD,
                output,
                rustix::fs::RenameFlags::EXCHANGE,
            )
            .map_err(std::io::Error::from)?;
            #[cfg(not(target_os = "linux"))]
            {
                let backup = tempfile::Builder::new()
                    .prefix(".diplodocus-old-")
                    .tempdir_in(parent)?;
                let old = backup.path().join("site");
                fs::rename(output, &old)?;
                if let Err(error) = fs::rename(stage.path(), output) {
                    fs::rename(old, output)?;
                    return Err(error.into());
                }
            }
        } else {
            fs::rename(stage.path(), output)?;
        }
        Ok(())
    }
}
