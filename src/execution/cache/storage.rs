use super::*;
use rustix::fs::{FlockOperation, Mode, OFlags};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

// Bound untrusted allocation before parsing or decoding images. Oversized
// artifacts are rejected as complete candidates, never partially restored.
const MAX_MANIFEST: u64 = 64 * 1024 * 1024;
const MAX_ASSETS: u64 = 512 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}
fn ensure(ok: bool) -> io::Result<()> {
    if ok { Ok(()) } else { Err(invalid()) }
}

// Resolve every component with a directory descriptor and O_NOFOLLOW. Keeping
// the descriptors alive anchors subsequent operations even if paths are renamed.
struct Directory(File);
impl Directory {
    fn open(path: &Path, create: bool) -> io::Result<Self> {
        let mut directory = Self(File::open(if path.is_absolute() { "/" } else { "." })?);
        for component in path.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::CurDir => {}
                std::path::Component::Normal(name) => directory = directory.child(name, create)?,
                _ => return Err(invalid()),
            }
        }
        Ok(directory)
    }
    fn child(&self, name: impl AsRef<std::ffi::OsStr>, create: bool) -> io::Result<Self> {
        let name = name.as_ref();
        if create {
            match rustix::fs::mkdirat(&self.0, name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Self(
            rustix::fs::openat(
                &self.0,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?
            .into(),
        ))
    }
    fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.0.as_raw_fd()))
    }
}

fn read(path: &Path, maximum: u64) -> io::Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure(metadata.is_file() && metadata.len() <= maximum)?;
    let mut bytes = Vec::new();
    (&mut file).take(maximum + 1).read_to_end(&mut bytes)?;
    ensure(bytes.len() as u64 == metadata.len())?;
    Ok(bytes)
}

fn names(path: &Path, expected: BTreeSet<String>) -> io::Result<()> {
    let actual = fs::read_dir(path)?
        .map(|e| e?.file_name().into_string().map_err(|_| invalid()))
        .collect::<io::Result<BTreeSet<_>>>()?;
    ensure(actual == expected)
}

fn entry_path(root: &Path, key: &CanonicalValue) -> io::Result<PathBuf> {
    let digest =
        identity::domain_digest("diplodocus/page-execution-key-v1", key).map_err(|_| invalid())?;
    Ok(root.join("v1/sha256").join(&digest[7..]))
}

fn load(
    root: &Path,
    key: &CanonicalValue,
    prepared: &PreparedExecution,
) -> io::Result<Option<Candidate>> {
    let entry = entry_path(root, key)?;
    let entry_directory = Directory::open(&entry, false)?;
    load_entry(entry_directory, key, prepared)
}

fn load_entry(
    entry_directory: Directory,
    key: &CanonicalValue,
    prepared: &PreparedExecution,
) -> io::Result<Option<Candidate>> {
    let entry = entry_directory.path();
    let manifest = read(&entry.join("manifest.json"), MAX_MANIFEST)?;
    let canonical = CanonicalValue::decode(&manifest).map_err(|_| invalid())?;
    let value = codec::json_value(&canonical).map_err(|_| invalid())?;
    if value["schema"] != codec::SCHEMA
        || value["result"]["ir_schema"] != "execution-result-v1"
        || value["key_input"]["schema"] != "page-execution-key-v1"
        || value["key_input"]["schemas"]["encoding"] != "execution-json-v1"
        || value["key_input"]["schemas"]["artifact"] != codec::SCHEMA
        || value["key_input"]["schemas"]["ir"] != "execution-result-v1"
    {
        return Ok(None);
    }
    codec::envelope(&manifest, key).map_err(|_| invalid())?;
    let entries = value["result"]["assets"].as_array().ok_or_else(invalid)?;
    let mut assets = BTreeMap::new();
    let assets_directory = entry_directory.child("assets", false)?;
    let asset_directory = assets_directory.child("sha256", false)?;
    let asset_dir = asset_directory.path();
    names(
        &entry,
        BTreeSet::from(["manifest.json".into(), "assets".into()]),
    )?;
    names(&assets_directory.path(), BTreeSet::from(["sha256".into()]))?;
    let mut remaining = MAX_ASSETS;
    for asset in entries {
        let fingerprint = codec::fingerprint(&asset["digest"]).map_err(|_| invalid())?;
        ensure(!assets.contains_key(&fingerprint.value))?;
        let bytes = read(&asset_dir.join(&fingerprint.value), remaining)?;
        remaining = remaining
            .checked_sub(bytes.len() as u64)
            .ok_or_else(invalid)?;
        assets.insert(fingerprint.value, bytes);
    }
    names(&asset_dir, assets.keys().cloned().collect())?;
    let page = codec::restore(&manifest, key, prepared, &assets).map_err(|_| invalid())?;
    // Recheck entry bytes after validation so ordinary concurrent edits reject
    // the candidate. Publication only installs immutable complete directories.
    ensure(read(&entry.join("manifest.json"), MAX_MANIFEST)? == manifest)?;
    Ok(Some(Candidate { page, assets }))
}

pub(super) fn lookup(root: &Path, key: &CanonicalValue, prepared: &PreparedExecution) -> Lookup {
    let entry = match entry_path(root, key) {
        Ok(path) => path,
        Err(_) => return Lookup::Rejected,
    };
    // Missing cache ancestors are normal, but missing files inside an existing
    // entry are corruption and must produce the same warning as invalid bytes.
    match Directory::open(entry.parent().expect("key parent"), false) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Lookup::Miss,
        Err(_) => return Lookup::Rejected,
        Ok(_) => {}
    }
    match fs::symlink_metadata(&entry) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Lookup::Miss,
        Err(_) => return Lookup::Rejected,
        Ok(_) => {}
    }
    match load(root, key, prepared) {
        Ok(Some(candidate)) => Lookup::Hit(Box::new(candidate)),
        Ok(None) => Lookup::Miss,
        Err(_) => Lookup::Rejected,
    }
}

pub(super) fn publish(
    root: &Path,
    key: &CanonicalValue,
    prepared: &PreparedExecution,
    encoded: &[u8],
    assets: BTreeMap<String, PathBuf>,
) -> Option<DiagnosticCode> {
    match publish_inner(root, key, prepared, encoded, assets) {
        Ok(code) => code,
        Err(_) => Some(DiagnosticCode::ExecutionCacheUnavailable),
    }
}

fn publish_inner(
    root: &Path,
    key: &CanonicalValue,
    prepared: &PreparedExecution,
    encoded: &[u8],
    assets: BTreeMap<String, PathBuf>,
) -> io::Result<Option<DiagnosticCode>> {
    let root_directory = Directory::open(root, true)?;
    let destination_directory = root_directory.child("v1", true)?.child("sha256", true)?;
    let locks_directory = root_directory.child("locks", true)?.child("sha256", true)?;
    let staging_directory = root_directory.child("staging", true)?;
    let original_destination = entry_path(root, key)?;
    let destination = destination_directory
        .path()
        .join(original_destination.file_name().unwrap());
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(
            locks_directory
                .path()
                .join(destination.file_name().unwrap()),
        )?;
    ensure(lock.metadata()?.is_file())?;
    match rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive) {
        Ok(_) => {}
        Err(rustix::io::Errno::WOULDBLOCK) => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    if let Ok(Some(candidate)) = destination_directory
        .child(destination.file_name().unwrap(), false)
        .and_then(|entry| load_entry(entry, key, prepared))
    {
        let previous = codec::encode(prepared, key, &candidate.page).map_err(|_| invalid())?;
        return Ok((previous != encoded).then_some(DiagnosticCode::NonDeterministicExecution));
    }
    let stage = tempfile::Builder::new()
        .prefix("page-")
        .tempdir_in(staging_directory.path())?;
    let mut renamed = false;
    let writing = (|| {
        let stage_directory = staging_directory.child(stage.path().file_name().unwrap(), false)?;
        let assets_directory = stage_directory.child("assets", true)?;
        let asset_directory = assets_directory.child("sha256", true)?;
        let asset_dir = asset_directory.path();
        let mut copied = BTreeMap::new();
        let mut remaining = MAX_ASSETS;
        for (digest, path) in assets {
            let bytes = read(&path, remaining)?;
            remaining = remaining
                .checked_sub(bytes.len() as u64)
                .ok_or_else(invalid)?;
            // A caller-owned staging path may have changed after retention.
            ensure(identity::content_digest(&bytes) == format!("sha256:{digest}"))?;
            write(&asset_dir.join(&digest), &bytes)?;
            copied.insert(digest, bytes);
        }
        ensure(encoded.len() as u64 <= MAX_MANIFEST)?;
        codec::restore(encoded, key, prepared, &copied).map_err(|_| invalid())?;
        write(&stage.path().join("manifest.json"), encoded)?;
        for directory in [&asset_directory, &assets_directory, &stage_directory] {
            directory.0.sync_all()?;
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(&destination)?,
            Ok(_) => fs::remove_file(&destination)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(stage.path(), &destination)?;
        renamed = true;
        destination_directory.0.sync_all()?;
        Ok(())
    })();
    let cleanup = if renamed {
        let _ = stage.keep();
        Ok(())
    } else {
        stage.close()
    };
    writing?;
    cleanup?;
    Ok(None)
}
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
