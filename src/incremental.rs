//! Private, disposable result caches and current-command reuse policy.
//!
//! Cache records are candidates. Consumers reconstruct local observations and
//! apply their normal semantic and output validation before accepting them.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::configuration::{PackageConfiguration, WorkspaceConfiguration};
use crate::ir::Fingerprint;
use crate::paths::ResolvedWorkspacePaths;
use crate::provenance::fingerprint_bytes;

/// Result-cache behavior for one command, independent of compiler caches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachePolicy {
    /// Reuse verified results and retain newly completed results.
    #[default]
    Auto,
    /// Bypass both result-cache reads and writes.
    Bypass,
}

thread_local! { static POLICY: Cell<Option<CachePolicy>> = const { Cell::new(None) }; }
thread_local! { static OBSERVATIONS: RefCell<Option<VerificationState>> = const { RefCell::new(None) }; }
tokio::task_local! { static ASYNC_POLICY: CachePolicy; }
tokio::task_local! { static OUTPUTS: Vec<PathBuf>; }

#[derive(Default)]
struct VerificationState {
    resolutions: HashMap<PathBuf, Option<PathBuf>>,
    metadata: HashMap<PathBuf, fs::Metadata>,
    generated_files: HashMap<PathBuf, bool>,
    generated_sites: HashMap<PathBuf, bool>,
    directories: HashMap<PathBuf, Arc<Vec<OsString>>>,
    digests: HashMap<PathBuf, Fingerprint>,
    inventories: BTreeMap<InventorySpec, Arc<Manifest>>,
    environment: HashMap<String, Option<Fingerprint>>,
}

pub(crate) async fn scope_outputs<T>(paths: Vec<PathBuf>, operation: impl Future<Output = T>) -> T {
    OUTPUTS.scope(paths, operation).await
}

pub(crate) fn verification_scope<T>(operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<VerificationState>);
    impl Drop for Reset {
        fn drop(&mut self) {
            OBSERVATIONS.with(|cache| *cache.borrow_mut() = self.0.take());
        }
    }
    let _reset =
        Reset(OBSERVATIONS.with(|cache| cache.replace(Some(VerificationState::default()))));
    operation()
}

fn observing<T>(operation: impl FnOnce() -> T) -> T {
    if OBSERVATIONS.with(|cache| cache.borrow().is_some()) {
        operation()
    } else {
        verification_scope(operation)
    }
}

fn observed<T>(read: impl FnOnce(&VerificationState) -> Option<T>) -> Option<T> {
    OBSERVATIONS.with(|cache| cache.borrow().as_ref().and_then(read))
}

fn remember(write: impl FnOnce(&mut VerificationState)) {
    OBSERVATIONS.with(|cache| {
        if let Some(state) = cache.borrow_mut().as_mut() {
            write(state);
        }
    });
}

fn resolve_observed(path: &Path) -> io::Result<Option<PathBuf>> {
    if let Some(target) = observed(|state| state.resolutions.get(path).cloned()) {
        return Ok(target);
    }
    let target = if cfg!(unix)
        && OBSERVATIONS.with(|state| state.borrow().is_some())
        && let Some((parent, name)) = ordinary_child(path)
    {
        match resolve_observed(parent)? {
            Some(parent) if metadata_observed(&parent)?.is_dir() => {
                let child = parent.join(name);
                if let Some(target) = observed(|state| state.resolutions.get(&child).cloned()) {
                    target
                } else {
                    // The parent was resolved in this frame, so nonlinks need no prefix walk.
                    let target = match fs::symlink_metadata(&child) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            canonicalize_observed(&child)?
                        }
                        Ok(metadata) => {
                            remember(|state| {
                                state.metadata.insert(child.clone(), metadata);
                            });
                            Some(child.clone())
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                            ) =>
                        {
                            None
                        }
                        Err(error) => return Err(error),
                    };
                    remember(|state| {
                        state.resolutions.insert(child, target.clone());
                    });
                    target
                }
            }
            _ => None,
        }
    } else {
        canonicalize_observed(path)?
    };
    remember(|state| {
        state.resolutions.insert(path.to_owned(), target.clone());
        if let Some(target) = &target {
            state
                .resolutions
                .entry(target.clone())
                .or_insert_with(|| Some(target.clone()));
        }
    });
    Ok(target)
}

fn canonicalize_observed(path: &Path) -> io::Result<Option<PathBuf>> {
    Ok(match fs::canonicalize(path) {
        Ok(path) => Some(path),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            None
        }
        Err(error) => return Err(error),
    })
}

fn ordinary_child(path: &Path) -> Option<(&Path, &std::ffi::OsStr)> {
    let last = path
        .as_os_str()
        .as_encoded_bytes()
        .rsplit(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'))
        .next()?;
    // Trailing directory syntax must retain canonicalize's not-a-directory checks.
    if matches!(last, b"" | b"." | b"..") {
        return None;
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())?;
    Some((parent, path.file_name()?))
}

fn metadata_observed(canonical: &Path) -> io::Result<fs::Metadata> {
    if let Some(metadata) = observed(|state| state.metadata.get(canonical).cloned()) {
        return Ok(metadata);
    }
    let metadata = fs::metadata(canonical)?;
    remember(|state| {
        state
            .metadata
            .insert(canonical.to_owned(), metadata.clone());
    });
    Ok(metadata)
}

fn generated_file_observed(canonical: &Path) -> bool {
    if let Some(generated) = observed(|state| state.generated_files.get(canonical).copied()) {
        return generated;
    }
    let immutable = immutable_signature(canonical).ok().flatten();
    let classifications = immutable_classifications();
    if let Some(signature) = &immutable
        && let Some((previous, generated)) = classifications
            .lock()
            .expect("immutable file classifications")
            .get(canonical)
        && previous == signature
    {
        let generated = *generated;
        remember(|state| {
            state
                .generated_files
                .insert(canonical.to_owned(), generated);
        });
        return generated;
    }
    let generated = crate::generated_storage::is_generated_file(canonical);
    if let Some(signature) = immutable {
        classifications
            .lock()
            .expect("immutable file classifications")
            .insert(canonical.to_owned(), (signature, generated));
    }
    remember(|state| {
        state
            .generated_files
            .insert(canonical.to_owned(), generated);
    });
    generated
}

fn generated_site_observed(canonical: &Path) -> bool {
    if let Some(generated) = observed(|state| state.generated_sites.get(canonical).copied()) {
        return generated;
    }
    let generated = fs::read(canonical.join(".diplodocus-site"))
        .is_ok_and(|bytes| bytes == b"diplodocus-site-v1\n");
    remember(|state| {
        state
            .generated_sites
            .insert(canonical.to_owned(), generated);
    });
    generated
}

fn directory_observed(canonical: &Path) -> io::Result<Arc<Vec<OsString>>> {
    if let Some(entries) = observed(|state| state.directories.get(canonical).cloned()) {
        return Ok(entries);
    }
    let mut entries = fs::read_dir(canonical)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort();
    let entries = Arc::new(entries);
    remember(|state| {
        state
            .directories
            .insert(canonical.to_owned(), entries.clone());
    });
    Ok(entries)
}

pub(crate) fn file_digest(path: &Path) -> io::Result<Fingerprint> {
    if let Some(digest) = observed(|state| state.digests.get(path).cloned()) {
        return Ok(digest);
    }
    let canonical =
        resolve_observed(path)?.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    if let Some(digest) = observed(|state| state.digests.get(&canonical).cloned()) {
        remember(|state| {
            state.digests.insert(path.to_owned(), digest.clone());
        });
        return Ok(digest);
    }
    let immutable = immutable_signature(&canonical)?;
    // Nix store files are immutable; metadata checks also detect replacement or repair.
    static IMMUTABLE: OnceLock<std::sync::Mutex<ImmutableDigests>> = OnceLock::new();
    let remembered = IMMUTABLE.get_or_init(Default::default);
    if let Some(signature) = &immutable
        && let Some((previous, digest)) = remembered
            .lock()
            .expect("immutable input digests")
            .get(&canonical)
        && previous == signature
    {
        let digest = digest.clone();
        remember(|state| {
            state.digests.insert(path.to_owned(), digest.clone());
            state.digests.insert(canonical.clone(), digest.clone());
        });
        return Ok(digest);
    }
    let digest = fingerprint_bytes(&fs::read(&canonical)?);
    if let Some(signature) = immutable {
        remembered
            .lock()
            .expect("immutable input digests")
            .insert(canonical.clone(), (signature, digest.clone()));
    }
    remember(|state| {
        state.digests.insert(path.to_owned(), digest.clone());
        state.digests.insert(canonical, digest.clone());
    });
    Ok(digest)
}

#[cfg(unix)]
type ImmutableSignature = (u64, u64, u64, i64, i64, i64, i64, u32);
#[cfg(not(unix))]
type ImmutableSignature = ();
type ImmutableDigests = HashMap<PathBuf, (ImmutableSignature, Fingerprint)>;
type ImmutableClassifications = HashMap<PathBuf, (ImmutableSignature, bool)>;

fn immutable_classifications() -> &'static std::sync::Mutex<ImmutableClassifications> {
    // Repeated guards still inspect metadata, but immutable files need one header read.
    static CLASSIFICATIONS: OnceLock<std::sync::Mutex<ImmutableClassifications>> = OnceLock::new();
    CLASSIFICATIONS.get_or_init(Default::default)
}

fn immutable_signature(canonical: &Path) -> io::Result<Option<ImmutableSignature>> {
    #[cfg(unix)]
    {
        if canonical.starts_with("/nix/store") {
            let metadata = metadata_observed(canonical)?;
            return Ok(metadata.is_file().then(|| metadata_signature(&metadata)));
        }
    }
    #[cfg(not(unix))]
    let _ = canonical;
    Ok(None)
}

#[cfg(unix)]
fn metadata_signature(metadata: &fs::Metadata) -> ImmutableSignature {
    use std::os::unix::fs::MetadataExt;
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
        metadata.mode(),
    )
}

pub(crate) fn enabled() -> bool {
    ASYNC_POLICY.try_with(|p| *p).ok().or_else(|| POLICY.get()) == Some(CachePolicy::Auto)
}

pub(crate) fn bypassed() -> bool {
    ASYNC_POLICY.try_with(|p| *p).ok().or_else(|| POLICY.get()) == Some(CachePolicy::Bypass)
}

pub(crate) fn scope<T>(policy: CachePolicy, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<CachePolicy>);
    impl Drop for Reset {
        fn drop(&mut self) {
            POLICY.set(self.0);
        }
    }
    let _reset = Reset(POLICY.replace(Some(policy)));
    operation()
}

pub(crate) async fn scope_async<T>(policy: CachePolicy, operation: impl Future<Output = T>) -> T {
    ASYNC_POLICY.scope(policy, operation).await
}

pub(crate) fn key(value: &impl Serialize) -> Fingerprint {
    fingerprint_bytes(
        &serde_json::to_vec(&("incremental-v1", value)).expect("cache identity serializes"),
    )
}

pub(crate) struct BuildIdentity;
impl BuildIdentity {
    pub(crate) fn observe() -> io::Result<String> {
        static IDENTITY: OnceLock<String> = OnceLock::new();
        if let Some(value) = IDENTITY.get() {
            return Ok(value.clone());
        }
        #[cfg(target_os = "linux")]
        let path = PathBuf::from("/proc/self/exe");
        #[cfg(not(target_os = "linux"))]
        let path = std::env::current_exe()?;
        let value = fingerprint_bytes(&fs::read(path)?).value;
        let _ = IDENTITY.set(value.clone());
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    target: Option<PathBuf>,
    digest: Option<Fingerprint>,
    directory: bool,
    permissions: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Selection {
    #[default]
    All,
    NativeLibraries,
    RustSources {
        roots: Vec<PathBuf>,
    },
    PythonSources {
        roots: Vec<PathBuf>,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InventorySpec {
    selection: Selection,
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
    environment: Vec<String>,
}

impl InventorySpec {
    fn new(
        paths: &[PathBuf],
        environment: &[String],
        excludes: &[PathBuf],
        selection: Selection,
    ) -> Self {
        let mut roots = paths.to_vec();
        roots.sort();
        roots.dedup();
        let mut excludes = excludes.to_vec();
        excludes.extend(OUTPUTS.try_with(Clone::clone).unwrap_or_default());
        excludes.sort();
        excludes.dedup();
        excludes.retain(|exclude| {
            roots
                .iter()
                .any(|root| exclude.starts_with(root) || root.starts_with(exclude))
        });
        let mut environment = environment.to_vec();
        environment.sort();
        environment.dedup();
        Self {
            selection,
            roots,
            excludes,
            environment,
        }
    }
}

fn environment_observed(name: &str) -> Option<Fingerprint> {
    if let Some(value) = observed(|state| state.environment.get(name).cloned()) {
        return value;
    }
    let value =
        std::env::var_os(name).map(|value| fingerprint_bytes(value.as_os_str().as_encoded_bytes()));
    remember(|state| {
        state.environment.insert(name.to_owned(), value.clone());
    });
    value
}

pub(crate) fn native_library_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name.ends_with(".so")
                || name.contains(".so.")
                || name.ends_with(".dylib")
                || name.ends_with(".dll")
        })
}

/// Exact local observations, including missing paths and directory membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    #[serde(default)]
    selection: Selection,
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
    inputs: BTreeMap<PathBuf, Input>,
    environment: BTreeMap<String, Option<Fingerprint>>,
}

impl Manifest {
    pub(crate) fn capture(paths: &[PathBuf], environment: &[String]) -> io::Result<Self> {
        Self::capture_excluding(paths, environment, &[])
    }

    pub(crate) fn capture_excluding(
        paths: &[PathBuf],
        environment: &[String],
        excludes: &[PathBuf],
    ) -> io::Result<Self> {
        Self::capture_selected(paths, environment, excludes, Selection::All)
    }

    pub(crate) fn capture_rust_sources(
        paths: &[PathBuf],
        environment: &[String],
        excludes: &[PathBuf],
        mut roots: Vec<PathBuf>,
    ) -> io::Result<Self> {
        roots.sort();
        roots.dedup();
        Self::capture_selected(
            paths,
            environment,
            excludes,
            Selection::RustSources { roots },
        )
    }

    pub(crate) fn capture_native_libraries(
        paths: &[PathBuf],
        environment: &[String],
    ) -> io::Result<Self> {
        Self::capture_selected(paths, environment, &[], Selection::NativeLibraries)
    }

    fn capture_selected(
        paths: &[PathBuf],
        environment: &[String],
        excludes: &[PathBuf],
        selection: Selection,
    ) -> io::Result<Self> {
        let spec = InventorySpec::new(paths, environment, excludes, selection);
        observing(|| Self::capture_inventory(&spec))
    }

    fn capture_inventory(spec: &InventorySpec) -> io::Result<Self> {
        let mut inputs = BTreeMap::new();
        for path in &spec.roots {
            // A completed earlier root already observed this subtree under the same selection.
            if inputs.contains_key(path) {
                continue;
            }
            visit(
                path,
                &spec.excludes,
                &mut inputs,
                &mut BTreeSet::new(),
                &spec.selection,
            )?;
        }
        let environment = spec
            .environment
            .iter()
            .map(|name| (name.clone(), environment_observed(name)))
            .collect();
        Ok(Self {
            selection: spec.selection.clone(),
            roots: spec.roots.clone(),
            excludes: spec.excludes.clone(),
            inputs,
            environment,
        })
    }

    pub(crate) fn verify(&self) -> bool {
        self.current().is_ok_and(|current| *current == *self)
    }

    pub(crate) fn verify_input(
        &self,
        check: &'static str,
    ) -> Result<(), crate::assembly::AssemblyError> {
        use crate::assembly::{AssemblyError, fingerprint_difference};

        let current = self.current().map_err(|error| {
            AssemblyError::inputs_changed(check, format!("could not recapture inputs: {error}"))
        })?;
        if *current == *self {
            return Ok(());
        }
        for (path, expected) in &self.inputs {
            let observed = current.inputs.get(path);
            if observed != Some(expected) {
                let digest = fingerprint_difference(
                    path.display(),
                    expected.digest.as_ref(),
                    observed.and_then(|input| input.digest.as_ref()),
                );
                let details = if let Some(observed) = observed {
                    if expected.target != observed.target
                        || expected.directory != observed.directory
                        || expected.permissions != observed.permissions
                    {
                        format!(
                            "{digest}; input metadata changed (expected target {:?}, directory {}, permissions {:?}; observed target {:?}, directory {}, permissions {:?})",
                            expected.target,
                            expected.directory,
                            expected.permissions,
                            observed.target,
                            observed.directory,
                            observed.permissions,
                        )
                    } else {
                        digest
                    }
                } else {
                    format!("{digest}; input left the inventory")
                };
                return Err(AssemblyError::inputs_changed(check, details));
            }
        }
        if let Some((path, observed)) = current
            .inputs
            .iter()
            .find(|(path, _)| !self.inputs.contains_key(*path))
        {
            return Err(AssemblyError::inputs_changed(
                check,
                format!(
                    "{}; input entered the inventory",
                    fingerprint_difference(path.display(), None, observed.digest.as_ref())
                ),
            ));
        }
        for (name, expected) in &self.environment {
            let observed = current.environment.get(name).and_then(Option::as_ref);
            if expected.as_ref() != observed {
                return Err(AssemblyError::fingerprint_changed(
                    check,
                    format_args!("environment variable {name}"),
                    expected.as_ref(),
                    observed,
                ));
            }
        }
        Err(AssemblyError::inputs_changed(
            check,
            format!(
                "inventory exclusions changed (expected {:?}, observed {:?})",
                self.excludes, current.excludes
            ),
        ))
    }

    fn current(&self) -> io::Result<Arc<Self>> {
        let spec = InventorySpec::new(
            &self.roots,
            &self.environment.keys().cloned().collect::<Vec<_>>(),
            &self.excludes,
            self.selection.clone(),
        );
        observing(|| {
            if let Some(current) = observed(|state| state.inventories.get(&spec).cloned()) {
                return Ok(current);
            }
            let current = Arc::new(Self::capture_inventory(&spec)?);
            remember(|state| {
                state.inventories.insert(spec, current.clone());
            });
            Ok(current)
        })
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.inputs.keys()
    }
    pub(crate) fn verify_file(
        &self,
        check: &'static str,
        path: &Path,
        target: &Path,
        expected: Option<&Fingerprint>,
    ) -> Result<(), crate::assembly::AssemblyError> {
        use crate::assembly::AssemblyError;

        let input = self.inputs.get(path);
        let observed_target = input.and_then(|input| input.target.as_deref());
        let observed = input.and_then(|input| input.digest.as_ref());
        if observed_target == Some(target) && expected.is_some() && expected == observed {
            return Ok(());
        }
        Err(AssemblyError::fingerprint_changed(
            check,
            format_args!(
                "{} (expected target {}, observed target {observed_target:?})",
                path.display(),
                target.display()
            ),
            expected,
            observed,
        ))
    }
    pub(crate) fn resolved_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.inputs
            .values()
            .filter_map(|input| input.target.as_ref())
    }
    pub(crate) fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.roots
            .iter()
            .filter(|_| self.selection != Selection::NativeLibraries)
    }
    pub(crate) fn native_library_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.roots
            .iter()
            .filter(|_| self.selection == Selection::NativeLibraries)
    }
}

fn visit(
    path: &Path,
    excludes: &[PathBuf],
    inputs: &mut BTreeMap<PathBuf, Input>,
    seen: &mut BTreeSet<PathBuf>,
    selection: &Selection,
) -> io::Result<()> {
    if crate::generated_storage::is_reserved(path)
        || excludes.iter().any(|root| path.starts_with(root))
    {
        return Ok(());
    }
    let at_path =
        |error: io::Error| io::Error::new(error.kind(), format!("{}: {error}", path.display()));
    let target = match resolve_observed(path).map_err(at_path)? {
        Some(path) => path,
        None => {
            inputs.insert(
                path.to_owned(),
                Input {
                    target: None,
                    digest: None,
                    directory: false,
                    permissions: None,
                },
            );
            return Ok(());
        }
    };
    let metadata = metadata_observed(&target).map_err(at_path)?;
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    };
    #[cfg(not(unix))]
    let permissions = u32::from(metadata.permissions().readonly());
    if metadata.is_dir() && generated_site_observed(&target) {
        return Ok(());
    }
    if metadata.is_file() {
        if !generated_file_observed(&target) {
            inputs.insert(
                path.to_owned(),
                Input {
                    target: Some(target.clone()),
                    digest: Some(file_digest(&target).map_err(at_path)?),
                    directory: false,
                    permissions: Some(permissions),
                },
            );
        }
    } else if metadata.is_dir() {
        inputs.insert(
            path.to_owned(),
            Input {
                target: Some(target.clone()),
                digest: None,
                directory: true,
                permissions: Some(permissions),
            },
        );
        if !seen.insert(target.clone()) {
            return Err(at_path(io::Error::other("cyclic cache input directory")));
        }
        // Python discovery traverses `target` and `node_modules`. Retain the
        // other observations, including hidden files used as extra inputs.
        let python_source = matches!(selection, Selection::PythonSources { roots }
            if roots.iter().any(|root| path.starts_with(root)));
        let rust_source = matches!(selection, Selection::RustSources { roots }
            if roots.iter().any(|root| path.starts_with(root)));
        for name in directory_observed(&target).map_err(at_path)?.iter() {
            if rust_source && crate::generated_storage::rust_environment_entry(name) {
                continue;
            }
            if *selection == Selection::NativeLibraries
                && (!native_library_file(Path::new(name))
                    || resolve_observed(&target.join(name))
                        .ok()
                        .flatten()
                        .and_then(|target| metadata_observed(&target).ok())
                        .is_some_and(|metadata| metadata.is_dir()))
            {
                continue;
            }
            let excluded = matches!(
                name.to_str(),
                Some(".git" | ".devenv" | ".direnv" | "__pycache__" | ".pytest_cache")
            ) || (!python_source
                && matches!(name.to_str(), Some("target" | "node_modules")));
            if excluded {
                continue;
            }
            visit(&path.join(name), excludes, inputs, seen, selection)?;
        }
        seen.remove(&target);
    } else {
        return Err(at_path(io::Error::other(
            "cache input is not a regular file or directory",
        )));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct Store {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    key: Fingerprint,
    digest: Fingerprint,
    payload: serde_json::Value,
}

impl Store {
    pub(crate) fn new(configuration_directory: &Path) -> Option<Self> {
        enabled().then(|| Self {
            root: configuration_directory.join(".diplodocus/cache/incremental"),
        })
    }

    fn path(&self, stage: &str, key: &Fingerprint) -> PathBuf {
        assert!(
            stage
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        );
        self.root.join(stage).join(format!("{}.json", key.value))
    }

    pub(crate) fn load<T: DeserializeOwned>(&self, stage: &str, key: &Fingerprint) -> Option<T> {
        let path = self.path(stage, key);
        if !regular_directory(path.parent()?) || !fs::symlink_metadata(&path).ok()?.is_file() {
            return None;
        }
        let envelope: Envelope = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        if envelope.schema != 1
            || envelope.key != *key
            || self::key(&envelope.payload) != envelope.digest
        {
            return None;
        }
        match serde_json::from_value(envelope.payload) {
            Ok(value) => Some(value),
            Err(_) => {
                let _ = self.discard(stage, key);
                None
            }
        }
    }

    pub(crate) fn save<T: Serialize>(
        &self,
        stage: &str,
        key: &Fingerprint,
        value: &T,
    ) -> io::Result<()> {
        let payload = serde_json::to_value(value)?;
        if let Some(existing) = self.load::<serde_json::Value>(stage, key) {
            return if existing == payload {
                Ok(())
            } else {
                Err(io::Error::other(
                    "the same cache inputs produced a different result; preserving the existing entry",
                ))
            };
        }
        let envelope = Envelope {
            schema: 1,
            key: key.clone(),
            digest: self::key(&payload),
            payload,
        };
        let path = self.path(stage, key);
        let parent = path.parent().expect("cache entry parent");
        ensure_directory(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temporary, &envelope)?;
        temporary.flush()?;
        if let Err(error) = temporary.persist_noclobber(&path) {
            if error.error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.error);
            }
            if let Some(existing) = self.load::<serde_json::Value>(stage, key) {
                if existing != envelope.payload {
                    return Err(io::Error::other(
                        "the same cache inputs produced a different result; preserving the existing entry",
                    ));
                }
            } else {
                error.file.persist(&path).map_err(|error| error.error)?;
            }
        }
        Ok(())
    }

    pub(crate) fn discard(&self, stage: &str, key: &Fingerprint) -> io::Result<()> {
        let path = self.path(stage, key);
        if !regular_directory(path.parent().expect("cache entry parent")) {
            return Err(io::Error::other(
                "cache directory must not be a symbolic link",
            ));
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        && path
            .parent()
            .filter(|parent| *parent != path)
            .is_none_or(regular_directory)
}

fn ensure_directory(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| *parent != path) {
        ensure_directory(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(io::Error::other(
            "cache directory must not be a symbolic link",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).or_else(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    ensure_directory(path)
                } else {
                    Err(error)
                }
            })
        }
        Err(error) => Err(error),
    }
}

fn supplemental_paths(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    package: &PackageConfiguration,
    visited: &mut BTreeSet<String>,
    inputs: &mut Vec<PathBuf>,
    env: &mut Vec<String>,
) -> io::Result<()> {
    if !visited.insert(package.id.clone()) {
        return Ok(());
    }
    let repository = paths
        .repositories
        .iter()
        .find(|r| r.id == package.repository)
        .ok_or_else(|| io::Error::other("unknown cache repository"))?;
    for path in &package.incremental.extra_inputs {
        let path = checked_incremental_path(&repository.path, path)?;
        validate_incremental_tree(&repository.path, &path, &mut BTreeSet::new())?;
        inputs.push(path);
    }
    env.extend(package.incremental.environment_variables.iter().cloned());
    let mut dependencies: BTreeSet<String> =
        package.incremental.depends_on.iter().cloned().collect();
    if let Some(index) = configuration
        .packages
        .iter()
        .position(|p| p.id == package.id)
    {
        dependencies.extend(inferred_dependencies(configuration, paths, index));
    }
    for id in dependencies {
        let index = configuration
            .packages
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| io::Error::other("unknown cache dependency"))?;
        inputs.extend(package_roots(
            &configuration.packages[index],
            &paths.packages[index],
        ));
        supplemental_paths(
            configuration,
            paths,
            &configuration.packages[index],
            visited,
            inputs,
            env,
        )?;
    }
    Ok(())
}

fn language_metadata(ecosystem: &str, path: &Path) -> Option<(String, BTreeSet<String>)> {
    let text = fs::read_to_string(path).ok()?;
    if ecosystem == "r" {
        let source = crate::ir::SourceLocation {
            repository: "cache".into(),
            path: "DESCRIPTION".try_into().ok()?,
            span: None,
        };
        let metadata = crate::extractors::r::validate_metadata(&text, &source, &mut Vec::new())?;
        return Some((
            metadata.name,
            metadata
                .dependencies
                .values()
                .flatten()
                .map(|dependency| dependency.name.clone())
                .collect(),
        ));
    }
    let value: toml::Value = toml::from_str(&text).ok()?;
    let normalize = |name: &str| {
        if ecosystem == "python" {
            name.to_ascii_lowercase().replace(['_', '.'], "-")
        } else {
            name.into()
        }
    };
    let mut dependencies = BTreeSet::new();
    let name = match ecosystem {
        "python" => {
            let project = value.get("project")?;
            if let Some(declared) = project.get("dependencies").and_then(toml::Value::as_array) {
                for dependency in declared.iter().filter_map(toml::Value::as_str) {
                    let name = dependency
                        .split(|c: char| !c.is_ascii_alphanumeric() && !"-_.".contains(c))
                        .next()?;
                    dependencies.insert(normalize(name));
                }
            }
            project.get("name")?.as_str()?
        }
        "julia" => {
            for field in ["deps", "weakdeps", "extras"] {
                if let Some(table) = value.get(field).and_then(toml::Value::as_table) {
                    dependencies.extend(table.keys().cloned());
                }
            }
            value.get("name")?.as_str()?
        }
        "rust" => {
            for field in ["dependencies", "build-dependencies", "dev-dependencies"] {
                if let Some(table) = value.get(field).and_then(toml::Value::as_table) {
                    dependencies.extend(table.iter().map(|(name, settings)| {
                        settings
                            .get("package")
                            .and_then(toml::Value::as_str)
                            .unwrap_or(name)
                            .to_owned()
                    }));
                }
            }
            value.get("package")?.get("name")?.as_str()?
        }
        _ => return None,
    };
    Some((normalize(name), dependencies))
}

fn inferred_dependencies(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    index: usize,
) -> Vec<String> {
    let package = &configuration.packages[index];
    let Some((_, dependencies)) =
        language_metadata(&package.ecosystem, &paths.packages[index].metadata_path)
    else {
        return Vec::new();
    };
    configuration
        .packages
        .iter()
        .enumerate()
        .filter(|(other, p)| *other != index && p.ecosystem == package.ecosystem)
        .filter_map(|(index, p)| {
            let (name, _) = language_metadata(&p.ecosystem, &paths.packages[index].metadata_path)?;
            dependencies.contains(&name).then(|| p.id.clone())
        })
        .collect()
}

pub(crate) fn package_order(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
) -> Vec<usize> {
    fn visit(
        index: usize,
        configuration: &WorkspaceConfiguration,
        paths: &ResolvedWorkspacePaths,
        visited: &mut BTreeSet<usize>,
        order: &mut Vec<usize>,
    ) {
        if !visited.insert(index) {
            return;
        }
        let package = &configuration.packages[index];
        let dependencies = package
            .incremental
            .depends_on
            .iter()
            .cloned()
            .chain(inferred_dependencies(configuration, paths, index));
        for id in dependencies {
            if let Some(dependency) = configuration.packages.iter().position(|p| p.id == id) {
                visit(dependency, configuration, paths, visited, order);
            }
        }
        order.push(index);
    }
    let mut order = Vec::new();
    let mut visited = BTreeSet::new();
    for index in 0..configuration.packages.len() {
        visit(index, configuration, paths, &mut visited, &mut order);
    }
    order
}

pub(crate) fn supplemental_manifest(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    package: &PackageConfiguration,
) -> io::Result<Manifest> {
    let mut inputs = Vec::new();
    let mut env = Vec::new();
    supplemental_paths(
        configuration,
        paths,
        package,
        &mut BTreeSet::new(),
        &mut inputs,
        &mut env,
    )?;
    workspace_manifest(configuration, paths, &inputs, &env, &[])
}

fn workspace_manifest(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    inputs: &[PathBuf],
    environment: &[String],
    excludes: &[PathBuf],
) -> io::Result<Manifest> {
    // Keep discovery rules per target, including dependencies of another
    // ecosystem, without traversing unrelated compiler output directories.
    let mut roots: Vec<_> = configuration
        .packages
        .iter()
        .zip(&paths.packages)
        .filter(|(package, _)| package.ecosystem == "python")
        .flat_map(|(_, package)| &package.targets)
        .map(|target| target.path.clone())
        .filter(|root| {
            inputs
                .iter()
                .any(|input| root.starts_with(input) || input.starts_with(root))
        })
        .collect();
    roots.sort();
    roots.dedup();
    let selection = if roots.is_empty() {
        Selection::All
    } else {
        Selection::PythonSources { roots }
    };
    Manifest::capture_selected(inputs, environment, excludes, selection)
}

fn package_roots(
    declaration: &PackageConfiguration,
    package: &crate::paths::ResolvedPackagePaths,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = if declaration.ecosystem == "r" {
        [
            "DESCRIPTION",
            "NAMESPACE",
            ".Rbuildignore",
            "renv.lock",
            "R",
            "src",
            "inst",
            "data",
            "man",
            "NEWS",
            "NEWS.md",
            "NEWS.Rd",
        ]
        .iter()
        .map(|name| package.path.join(name))
        .chain([package.metadata_path.clone()])
        .collect()
    } else if declaration.ecosystem == "julia" {
        package
            .targets
            .iter()
            .map(|target| target.path.parent().unwrap_or(&package.path).to_owned())
            .chain(
                declaration
                    .targets
                    .iter()
                    .filter_map(|target| target.julia.as_ref())
                    .flat_map(|julia| &julia.extensions)
                    .map(|path| package.path.join(path)),
            )
            .chain(
                [
                    "Project.toml",
                    "JuliaProject.toml",
                    "Manifest.toml",
                    "JuliaManifest.toml",
                    "LocalPreferences.toml",
                    "src",
                    "ext",
                    "deps",
                    "data",
                    "Artifacts.toml",
                ]
                .into_iter()
                .map(|name| package.path.join(name)),
            )
            .chain([package.metadata_path.clone()])
            .collect()
    } else if declaration.ecosystem == "python" {
        package
            .targets
            .iter()
            .map(|target| {
                if target.path.is_dir() {
                    target.path.clone()
                } else {
                    target.path.parent().unwrap_or(&package.path).to_owned()
                }
            })
            .chain(
                [
                    "pyproject.toml",
                    "setup.cfg",
                    "setup.py",
                    "requirements.txt",
                    "uv.lock",
                    "poetry.lock",
                    "pdm.lock",
                    "Pipfile.lock",
                    ".python-version",
                ]
                .into_iter()
                .map(|name| package.path.join(name)),
            )
            .chain([package.metadata_path.clone()])
            .collect()
    } else {
        vec![package.path.clone()]
    };
    if let Some(r) = &declaration.r {
        roots.extend(r.extra_inputs.iter().map(|path| package.path.join(path)));
    }
    roots
}

pub(crate) fn rust_package_context(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    index: usize,
) -> Option<(Store, String, Manifest)> {
    let package = &configuration.packages[index];
    if package.incremental.cache == crate::configuration::IncrementalCache::Off {
        return None;
    }
    Some((
        Store::new(&paths.configuration_directory)?,
        BuildIdentity::observe().ok()?,
        supplemental_manifest(configuration, paths, package).ok()?,
    ))
}

pub(crate) fn package_context(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    index: usize,
) -> Option<(Store, String, Manifest)> {
    let declaration = &configuration.packages[index];
    if declaration.incremental.cache == crate::configuration::IncrementalCache::Off {
        return None;
    }
    let store = Store::new(&paths.configuration_directory)?;
    let build = BuildIdentity::observe().ok()?;
    let package = &paths.packages[index];
    let mut inputs = package_roots(declaration, package);
    let mut env = vec!["PATH".into(), "LANG".into(), "LC_ALL".into(), "TZ".into()];
    supplemental_paths(
        configuration,
        paths,
        declaration,
        &mut BTreeSet::new(),
        &mut inputs,
        &mut env,
    )
    .ok()?;
    if let Some(r) = &declaration.r {
        inputs.extend(r.extra_inputs.iter().map(|path| package.path.join(path)));
    }
    let excludes: Vec<_> = paths
        .content
        .iter()
        .filter(|c| {
            c.path != paths.packages[index].path
                && c.path.starts_with(&paths.packages[index].path)
                && !package.targets.iter().any(|target| {
                    target.path.starts_with(&c.path) || c.path.starts_with(&target.path)
                })
        })
        .map(|c| c.path.clone())
        .collect();
    let manifest = workspace_manifest(configuration, paths, &inputs, &env, &excludes).ok()?;
    Some((store, build, manifest))
}

pub(crate) fn content_preparation_context(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    index: usize,
) -> Option<(Store, String, Manifest)> {
    let declaration = &configuration.content[index];
    if declaration.incremental.cache == crate::configuration::IncrementalCache::Off {
        return None;
    }
    let repository = &paths.repositories[paths.content[index].repository_index];
    let mut inputs = Vec::new();
    for path in &declaration.incremental.extra_inputs {
        let path = checked_incremental_path(&repository.path, path).ok()?;
        validate_incremental_tree(&repository.path, &path, &mut BTreeSet::new()).ok()?;
        inputs.push(path);
    }
    inputs.extend(
        declaration
            .execution
            .declared_environment_inputs
            .iter()
            .map(|p| repository.path.join(p)),
    );
    let mut env = declaration.incremental.environment_variables.clone();
    for id in &declaration.incremental.depends_on {
        let package_index = configuration.packages.iter().position(|p| &p.id == id)?;
        inputs.extend(package_roots(
            &configuration.packages[package_index],
            &paths.packages[package_index],
        ));
        supplemental_paths(
            configuration,
            paths,
            &configuration.packages[package_index],
            &mut BTreeSet::new(),
            &mut inputs,
            &mut env,
        )
        .ok()?;
    }
    Some((
        Store::new(&paths.configuration_directory)?,
        BuildIdentity::observe().ok()?,
        workspace_manifest(configuration, paths, &inputs, &env, &[]).ok()?,
    ))
}

fn validate_incremental_tree(
    boundary: &Path,
    path: &Path,
    seen: &mut BTreeSet<PathBuf>,
) -> io::Result<()> {
    if crate::generated_storage::is_reserved(path) {
        return Ok(());
    }
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !canonical.starts_with(boundary) {
        return Err(io::Error::other("incremental input escapes its repository"));
    }
    if canonical.is_dir() {
        if !seen.insert(canonical.clone()) {
            return Err(io::Error::other("cyclic incremental input directory"));
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if !matches!(
                entry.file_name().to_str(),
                Some(
                    ".git"
                        | ".devenv"
                        | ".direnv"
                        | "target"
                        | "node_modules"
                        | "__pycache__"
                        | ".pytest_cache"
                )
            ) {
                validate_incremental_tree(boundary, &entry.path(), seen)?;
            }
        }
        seen.remove(&canonical);
    }
    Ok(())
}

pub(crate) fn content_context(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    index: usize,
) -> Option<(Store, String, Manifest)> {
    let (store, build, supplemental) = content_preparation_context(configuration, paths, index)?;
    let collection = &configuration.content[index];
    let mut inputs = supplemental.roots;
    let mut env: Vec<_> = supplemental.environment.keys().cloned().collect();
    if let Some(package_index) = configuration
        .packages
        .iter()
        .position(|p| p.id == collection.owner)
    {
        let (_, _, package) = package_context(configuration, paths, package_index)?;
        inputs.extend(package.roots);
        env.extend(package.environment.keys().cloned());
    }
    Some((
        store,
        build,
        workspace_manifest(configuration, paths, &inputs, &env, &[]).ok()?,
    ))
}

/// Preserve declared aliases for later verification while checking containment.
pub(crate) fn checked_incremental_path(repository: &Path, declared: &Path) -> io::Result<PathBuf> {
    use std::path::Component;

    if declared.as_os_str().is_empty()
        || declared
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(io::Error::other(
            "incremental input must be repository-relative without parent components",
        ));
    }
    let boundary = fs::canonicalize(repository)?;
    let mut current = boundary.clone();
    let mut missing = false;
    for component in declared.components() {
        if let Component::Normal(name) = component {
            current.push(name);
            if !missing {
                match fs::symlink_metadata(&current) {
                    Ok(_) => {
                        current = fs::canonicalize(&current)?;
                        if !current.starts_with(&boundary) {
                            return Err(io::Error::other(
                                "incremental input escapes its repository",
                            ));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => missing = true,
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(boundary.join(declared))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_discovery_tracks_membership_without_including_unrelated_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("pkg");
        let source = python.join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        fs::create_dir(python.join("__pycache__")).unwrap();
        fs::create_dir(python.join(".pytest_cache")).unwrap();
        let hidden_input = python.join(".settings.json");
        fs::write(&hidden_input, "settings").unwrap();
        let roots = [root.path().to_owned()];
        let ordinary = Manifest::capture(&roots, &[]).unwrap();
        let sources = Manifest::capture_selected(
            &roots,
            &[],
            &[],
            Selection::PythonSources {
                roots: vec![python.clone()],
            },
        )
        .unwrap();
        assert!(sources.inputs.contains_key(&source));
        assert!(sources.inputs.contains_key(&hidden_input));
        assert!(!ordinary.inputs.contains_key(&source));
        assert_eq!(sources.roots().count(), 1);

        fs::write(root.path().join("target/compiler-output"), "output").unwrap();
        fs::write(python.join("__pycache__/cached.pyc"), "bytecode").unwrap();
        fs::write(python.join(".pytest_cache/ignored.py"), "ignored").unwrap();
        assert!(sources.verify());
        let ordinary = Manifest::capture(&roots, &[]).unwrap();
        fs::write(source.join("added.py"), "new source").unwrap();
        verification_scope(|| {
            assert!(ordinary.verify());
            assert!(!sources.verify());
        });
    }

    #[test]
    fn overlapping_roots_preserve_recursive_and_shallow_selections() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let source = nested.join("source.txt");
        let library = nested.join("libnested.so");
        fs::write(&source, "source").unwrap();
        fs::write(&library, "native").unwrap();
        let roots = [root.path().to_owned(), nested.clone(), source.clone()];
        let recursive = Manifest::capture(&roots, &[]).unwrap();
        let parent = Manifest::capture(&roots[..1], &[]).unwrap();
        assert_eq!(recursive.inputs, parent.inputs);
        assert_eq!(recursive.roots, roots);
        assert!(recursive.verify());

        let shallow = Manifest::capture_native_libraries(&roots[..2], &[]).unwrap();
        let parent = Manifest::capture_native_libraries(&roots[..1], &[]).unwrap();
        assert!(!parent.inputs.contains_key(&nested));
        assert!(shallow.inputs.contains_key(&nested));
        assert!(shallow.inputs.contains_key(&library));
        assert!(!shallow.inputs.contains_key(&source));
        fs::write(&source, "other").unwrap();
        assert!(!recursive.verify());
        assert!(shallow.verify());
        fs::write(&library, "changed native").unwrap();
        assert!(!shallow.verify());
    }

    #[test]
    fn verification_keeps_expectations_independent_and_refreshes_each_boundary() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source");
        fs::write(&path, "first").unwrap();
        let first = Manifest::capture(std::slice::from_ref(&path), &[]).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, "other").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let other = Manifest::capture(std::slice::from_ref(&path), &[]).unwrap();
        verification_scope(|| {
            assert!(!first.verify());
            assert!(other.verify());
            assert!(!first.verify());
        });
        verification_scope(|| {
            assert!(other.verify());
            assert!(!first.verify());
        });
        fs::write(&path, "third").unwrap();
        assert!(!other.verify());
        assert!(OBSERVATIONS.with(|state| state.borrow().is_none()));
    }

    #[test]
    fn shared_directory_entries_preserve_selections_and_exclusions() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        fs::write(root.path().join("libfirst.so"), "native").unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("nested/libnested.so"), "nested").unwrap();
        let roots = [root.path().to_owned()];
        let all = Manifest::capture(&roots, &[]).unwrap();
        let native = Manifest::capture_native_libraries(&roots, &[]).unwrap();
        let ignore_first =
            Manifest::capture_excluding(&roots, &[], std::slice::from_ref(&first)).unwrap();
        let ignore_second =
            Manifest::capture_excluding(&roots, &[], std::slice::from_ref(&second)).unwrap();
        fs::write(&first, "changed").unwrap();
        verification_scope(|| {
            assert!(native.verify());
            assert!(!all.verify());
            assert!(ignore_first.verify());
            assert!(!ignore_second.verify());
        });
        fs::write(root.path().join("nested/libnew.so"), "nested addition").unwrap();
        assert!(native.verify());
        fs::write(root.path().join("libnew.so"), "direct addition").unwrap();
        assert!(!native.verify());
    }

    #[test]
    fn effective_output_exclusions_do_not_share_an_inventory() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let roots = [root.path().to_owned()];
        let first_output = OUTPUTS.sync_scope(vec![first.clone()], || {
            Manifest::capture(&roots, &[]).unwrap()
        });
        let second_output = OUTPUTS.sync_scope(vec![second.clone()], || {
            Manifest::capture(&roots, &[]).unwrap()
        });
        verification_scope(|| {
            OUTPUTS.sync_scope(vec![first], || {
                assert!(first_output.verify());
                assert!(!second_output.verify());
            });
            OUTPUTS.sync_scope(vec![second], || {
                assert!(second_output.verify());
                assert!(!first_output.verify());
            });
        });
    }

    #[test]
    fn missing_paths_and_generated_storage_are_reobserved() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        fs::write(&parent, "not a directory").unwrap();
        let child = parent.join("child");
        let missing = Manifest::capture(std::slice::from_ref(&child), &[]).unwrap();
        assert!(missing.verify());
        fs::remove_file(&parent).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::write(&child, "present").unwrap();
        assert!(!missing.verify());
        let mut header = vec![0; 72];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[68..72]
            .copy_from_slice(&crate::generated_storage::SNAPSHOT_APPLICATION_ID.to_be_bytes());
        fs::write(&child, header).unwrap();
        let generated = Manifest::capture(std::slice::from_ref(&child), &[]).unwrap();
        assert!(generated.inputs.is_empty());
        assert!(generated.verify());
        fs::write(&child, "ordinary maintained input").unwrap();
        assert!(!generated.verify());
        let directory = Manifest::capture(std::slice::from_ref(&parent), &[]).unwrap();
        fs::write(parent.join(".diplodocus-site"), "diplodocus-site-v1\n").unwrap();
        assert!(!directory.verify());
    }

    #[test]
    fn nested_verification_restores_the_enclosing_frame_without_leaking_it() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source");
        fs::write(&path, "first").unwrap();
        let first = Manifest::capture(std::slice::from_ref(&path), &[]).unwrap();
        verification_scope(|| {
            assert!(first.verify());
            fs::write(&path, "other").unwrap();
            verification_scope(|| assert!(!first.verify()));
            assert!(first.verify());
        });
        assert!(!first.verify());
        assert!(OBSERVATIONS.with(|state| state.borrow().is_none()));
    }

    #[cfg(unix)]
    #[test]
    fn directory_aliases_remain_distinct_and_cycles_are_not_hidden() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("source"), "first").unwrap();
        fs::write(second.join("source"), "other").unwrap();
        let left = root.path().join("left");
        let right = root.path().join("right");
        symlink(&first, &left).unwrap();
        symlink(&first, &right).unwrap();
        let aliases = Manifest::capture(&[left.clone(), right.clone()], &[]).unwrap();
        assert!(aliases.inputs.contains_key(&left.join("source")));
        assert!(aliases.inputs.contains_key(&right.join("source")));
        verification_scope(|| {
            let direct = Manifest::capture(std::slice::from_ref(&first), &[]).unwrap();
            assert!(direct.verify());
            assert!(aliases.verify());
        });
        fs::remove_file(&right).unwrap();
        symlink(&second, &right).unwrap();
        assert!(!aliases.verify());
        symlink(&second, second.join("cycle")).unwrap();
        assert!(Manifest::capture(std::slice::from_ref(&right), &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn immutable_digest_hits_join_the_current_verification_frame() {
        let shell = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .find_map(|directory| {
                fs::canonicalize(directory.join("sh"))
                    .ok()
                    .filter(|path| path.starts_with("/nix/store") && path.is_file())
            });
        let Some(shell) = shell else {
            return;
        };
        let digest = file_digest(&shell).unwrap();
        verification_scope(|| {
            assert_eq!(file_digest(&shell).unwrap(), digest);
            assert_eq!(
                observed(|state| state.digests.get(&shell).cloned()),
                Some(digest)
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn immutable_classification_hits_join_the_current_verification_frame() {
        let shell = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .find_map(|directory| {
                fs::canonicalize(directory.join("sh"))
                    .ok()
                    .filter(|path| path.starts_with("/nix/store") && path.is_file())
            });
        let Some(shell) = shell else {
            return;
        };
        let first = verification_scope(|| generated_file_observed(&shell));
        let signature = immutable_signature(&shell).unwrap().unwrap();
        assert_eq!(
            immutable_classifications().lock().unwrap().get(&shell),
            Some(&(signature, first)),
        );
        verification_scope(|| {
            assert_eq!(generated_file_observed(&shell), first);
            assert_eq!(
                observed(|state| state.generated_files.get(&shell).copied()),
                Some(first)
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn immutable_signatures_detect_same_size_replacements_with_restored_mtime() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("input");
        let replacement = root.path().join("replacement");
        fs::write(&path, "first").unwrap();
        let before = fs::metadata(&path).unwrap();
        fs::write(&replacement, "other").unwrap();
        fs::set_permissions(&replacement, before.permissions()).unwrap();
        fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
            .unwrap();
        fs::rename(replacement, &path).unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_ne!(metadata_signature(&before), metadata_signature(&after));
        assert_eq!(immutable_signature(&path).unwrap(), None);
    }

    #[test]
    fn component_resolution_preserves_relative_and_directory_syntax() {
        let relative = fs::read_dir(".")
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| PathBuf::from(entry.file_name()))
            .find(|path| fs::canonicalize(path).is_ok())
            .unwrap();
        let expected = fs::canonicalize(&relative).unwrap();
        verification_scope(|| {
            assert!(ordinary_child(&relative).is_none());
            assert_eq!(resolve_observed(&relative).unwrap(), Some(expected));
            for special in [Path::new("."), Path::new("..")] {
                assert_eq!(
                    resolve_observed(special).unwrap(),
                    Some(fs::canonicalize(special).unwrap())
                );
            }
        });
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        fs::write(&file, "source").unwrap();
        let mut slash = file.as_os_str().to_os_string();
        slash.push(std::path::MAIN_SEPARATOR_STR);
        for path in [PathBuf::from(slash), file.join("."), file.join("..")] {
            verification_scope(|| {
                assert!(ordinary_child(&path).is_none());
                let expected = fs::canonicalize(&path).ok();
                #[cfg(unix)]
                assert!(expected.is_none());
                assert_eq!(resolve_observed(&path).unwrap(), expected);
            });
        }
    }

    #[cfg(unix)]
    #[test]
    fn component_resolution_keeps_symlink_parent_semantics_and_refreshes_replacements() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("real/nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("source"), "first").unwrap();
        let alias = root.path().join("alias");
        symlink(&nested, &alias).unwrap();
        let parent_path = alias.join("../nested/source");
        verification_scope(|| {
            assert_eq!(
                resolve_observed(&parent_path).unwrap(),
                Some(fs::canonicalize(&parent_path).unwrap())
            );
            let roots = [alias.join("missing/first"), alias.join("missing/second")];
            let missing = Manifest::capture(&roots, &[]).unwrap();
            assert!(missing.inputs.values().all(|input| input.target.is_none()));
        });
        fs::create_dir(nested.join("missing")).unwrap();
        fs::write(nested.join("missing/first"), "added").unwrap();
        verification_scope(|| {
            assert_eq!(
                resolve_observed(&alias.join("missing/first")).unwrap(),
                Some(fs::canonicalize(alias.join("missing/first")).unwrap())
            );
        });
        let file = root.path().join("file");
        fs::write(&file, "first").unwrap();
        let original = Manifest::capture(std::slice::from_ref(&file), &[]).unwrap();
        fs::remove_file(&file).unwrap();
        symlink(nested.join("source"), &file).unwrap();
        let linked = Manifest::capture(std::slice::from_ref(&file), &[]).unwrap();
        assert!(!original.verify());
        assert!(linked.verify());
        fs::remove_file(&file).unwrap();
        fs::write(&file, "first").unwrap();
        assert!(original.verify());
        assert!(!linked.verify());
    }

    #[test]
    fn native_loader_directories_ignore_build_artifacts_and_track_libraries() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("libexample.so.1"), "first").unwrap();
        fs::write(root.path().join("library.rlib"), "compiler artifact").unwrap();
        let manifest = Manifest::capture_native_libraries(&[root.path().to_owned()], &[]).unwrap();
        fs::write(
            root.path().join("library.rlib"),
            "changed compiler artifact",
        )
        .unwrap();
        fs::write(root.path().join("test-binary"), "new executable").unwrap();
        assert!(manifest.verify());
        fs::write(root.path().join("libexample.so.1"), "second").unwrap();
        assert!(!manifest.verify());
        let manifest = Manifest::capture_native_libraries(&[root.path().to_owned()], &[]).unwrap();
        fs::write(root.path().join("libnew.so"), "new library").unwrap();
        assert!(!manifest.verify());
    }

    #[test]
    fn supplemental_paths_allow_missing_inputs_and_reject_escaping_links() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        fs::create_dir(&repository).unwrap();
        assert_eq!(
            checked_incremental_path(&repository, Path::new("missing/nested")).unwrap(),
            repository.join("missing/nested")
        );
        for path in ["", "../outside", "/outside"] {
            assert!(checked_incremental_path(&repository, Path::new(path)).is_err());
        }
        #[cfg(unix)]
        {
            fs::create_dir(root.path().join("outside")).unwrap();
            std::os::unix::fs::symlink(root.path().join("outside"), repository.join("escape"))
                .unwrap();
            assert!(checked_incremental_path(&repository, Path::new("escape/missing")).is_err());
            fs::create_dir(repository.join("inside")).unwrap();
            std::os::unix::fs::symlink(repository.join("inside"), repository.join("alias"))
                .unwrap();
            assert_eq!(
                checked_incremental_path(&repository, Path::new("alias/missing")).unwrap(),
                repository.join("alias/missing")
            );
            std::os::unix::fs::symlink(
                root.path().join("outside"),
                repository.join("inside/escape"),
            )
            .unwrap();
            assert!(
                validate_incremental_tree(
                    &repository,
                    &repository.join("inside"),
                    &mut BTreeSet::new()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn manifest_checks_bytes_membership_and_missing_paths() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        let manifest = Manifest::capture(&[root.path().to_owned(), missing.clone()], &[]).unwrap();
        assert!(manifest.verify());
        fs::write(&missing, "first").unwrap();
        assert!(!manifest.verify());
        let manifest = Manifest::capture(&[root.path().to_owned()], &[]).unwrap();
        fs::write(&missing, "other").unwrap();
        assert!(!manifest.verify());
        fs::remove_file(missing).unwrap();
        assert!(!manifest.verify());
    }

    #[test]
    fn manifest_diagnostic_names_the_check_path_and_observed_digest_for_a_new_file() {
        let root = tempfile::tempdir().unwrap();
        let inputs = Manifest::capture(&[root.path().to_owned()], &[]).unwrap();
        let file = root.path().join("palette.png");
        let bytes = b"generated output";
        fs::write(&file, bytes).unwrap();

        let error = inputs
            .verify_input("package cache inventory verification")
            .unwrap_err();
        let message = error.to_string();
        let digest = fingerprint_bytes(bytes);
        assert!(
            message.contains("package cache inventory verification"),
            "{message}"
        );
        assert!(message.contains(&file.display().to_string()), "{message}");
        assert!(message.contains("input entered the inventory"), "{message}");
        assert!(
            message.contains("expected fingerprint unavailable"),
            "{message}"
        );
        assert!(
            message.contains(&format!("observed fingerprint sha256:{}", digest.value)),
            "{message}"
        );
    }

    #[test]
    fn store_is_scoped_and_rejects_corruption() {
        let root = tempfile::tempdir().unwrap();
        assert!(Store::new(root.path()).is_none());
        scope(CachePolicy::Auto, || {
            let store = Store::new(root.path()).unwrap();
            let key = key(&"input");
            store.save("test", &key, &vec!["result"]).unwrap();
            assert_eq!(store.load::<Vec<String>>("test", &key).unwrap(), ["result"]);
            assert!(store.save("test", &key, &vec!["different"]).is_err());
            assert_eq!(store.load::<Vec<String>>("test", &key).unwrap(), ["result"]);
            fs::write(store.path("test", &key), "{}").unwrap();
            assert!(store.load::<Vec<String>>("test", &key).is_none());
            store.save("test", &key, &vec!["recovered"]).unwrap();
            assert_eq!(
                store.load::<Vec<String>>("test", &key).unwrap(),
                ["recovered"]
            );
            store.discard("test", &key).unwrap();
            assert!(store.load::<Vec<String>>("test", &key).is_none());
            scope(CachePolicy::Bypass, || {
                assert!(Store::new(root.path()).is_none())
            });
        });
        assert!(Store::new(root.path()).is_none());
    }
}
