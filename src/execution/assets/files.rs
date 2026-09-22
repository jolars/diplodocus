//! Boundary checks apply before reading bytes or creating staging children.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;

use super::AssetError;
use crate::diagnostics::DiagnosticPath;
use crate::paths::{PathResolutionErrorKind, resolve_input_file};

/// Unix directory identity prevents a replacement tree from becoming owned
/// staging merely because it occupies the same checked pathname.
#[derive(PartialEq, Eq)]
pub(super) struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl DirectoryIdentity {
    pub fn capture(path: &Path) -> Result<Self, AssetError> {
        let metadata = fs::symlink_metadata(path).map_err(input_error)?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err(AssetError::OutsideBoundary);
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        })
    }

    pub fn matches(&self, path: &Path) -> bool {
        Self::capture(path).is_ok_and(|identity| &identity == self)
    }
}

pub(super) fn input_error(error: io::Error) -> AssetError {
    match error.kind() {
        io::ErrorKind::NotFound => AssetError::Missing,
        io::ErrorKind::NotADirectory => AssetError::OutsideBoundary,
        _ => AssetError::Storage,
    }
}

pub(super) fn absolute_normalized(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
}

/// Walk every component so a symlink cannot redirect creation outside staging.
pub(super) fn directory(path: &Path, create: bool) -> Result<(), AssetError> {
    if !absolute_normalized(path) {
        return Err(AssetError::OutsideBoundary);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
            Ok(_) => return Err(AssetError::OutsideBoundary),
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| AssetError::Storage)?;
            }
            Err(error) => return Err(input_error(error)),
        }
    }
    Ok(())
}

pub(super) fn read_local(
    repository: &Path,
    page: &DiagnosticPath,
    target: &str,
) -> Result<Vec<u8>, AssetError> {
    let decoded = percent_decode_str(target)
        .decode_utf8()
        .map_err(|_| AssetError::OutsideBoundary)?;
    if decoded.is_empty()
        || decoded.contains(['\\', ':', '\0', '?', '#'])
        || decoded.chars().any(char::is_control)
        || Path::new(decoded.as_ref()).is_absolute()
    {
        return Err(AssetError::OutsideBoundary);
    }
    let parent = Path::new(page.as_str()).parent().unwrap_or(Path::new(""));
    let declared = parent.join(decoded.as_ref());
    let resolved = resolve_input_file(repository, "execution asset", &declared, repository)
        .map_err(|error| match error.kind {
            PathResolutionErrorKind::FileSystem { source, .. } => input_error(source),
            _ => AssetError::OutsideBoundary,
        })?;
    read_regular(&resolved, repository)
}

pub(super) fn read_staged(path: &Path, directory_path: &Path) -> Result<Vec<u8>, AssetError> {
    directory(directory_path, false)?;
    let metadata = fs::symlink_metadata(path).map_err(input_error)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(AssetError::OutsideBoundary);
    }
    read_regular(path, directory_path)
}

fn read_regular(path: &Path, boundary: &Path) -> Result<Vec<u8>, AssetError> {
    // Nonblocking open prevents a replacement FIFO from hanging validation.
    // Inspect the opened referent before reading, not just the pathname checked
    // earlier. Authored code still runs with the user's filesystem privileges.
    #[cfg(target_os = "linux")]
    let mut file = {
        use rustix::fs::{Mode, OFlags, open};
        use std::os::fd::AsRawFd;
        let fd = open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP {
                AssetError::OutsideBoundary
            } else {
                input_error(error.into())
            }
        })?;
        let actual =
            fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).map_err(input_error)?;
        if actual != path || !actual.starts_with(boundary) {
            return Err(AssetError::OutsideBoundary);
        }
        File::from(fd)
    };
    #[cfg(not(target_os = "linux"))]
    let mut file = {
        if fs::canonicalize(path).map_err(input_error)? != path || !path.starts_with(boundary) {
            return Err(AssetError::OutsideBoundary);
        }
        File::open(path).map_err(input_error)?
    };
    if !file.metadata().map_err(input_error)?.is_file() {
        return Err(AssetError::OutsideBoundary);
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(input_error)?;
    Ok(bytes)
}
