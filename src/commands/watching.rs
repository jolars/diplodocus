use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::configuration::{ContentConfiguration, WorkspaceConfiguration};
use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::provenance::fingerprint_bytes;
use crate::reporting::{Event, Phase, Reporting};

/// Extract on startup and after debounced changes to declared inputs.
///
/// The callback receives the absolute path only after successful atomic
/// publication. The initial failure is returned; subsequent failures report
/// diagnostics and retain the last successful snapshot. Cancellation waits for
/// supervised execution cleanup before returning.
pub async fn watch_extract_with(
    options: ExtractOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    on_snapshot: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<(), CommandError> {
    CommandRunner::default()
        .watch_extract_with(options, deadlines, cancellation, on_snapshot)
        .await
}

pub(super) async fn watch_extract_reported(
    options: ExtractOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    on_snapshot: impl FnMut(&Path) -> std::io::Result<()>,
    reporting: &Reporting,
) -> Result<(), CommandError> {
    let (stop, receiver) = watch::channel(false);
    let worker = extract_loop(options, deadlines, receiver, on_snapshot, reporting);
    tokio::pin!(worker);
    tokio::select! {
        result = &mut worker => result,
        _ = cancellation => {
            let _ = stop.send(true);
            worker.await
        }
    }
}

async fn extract_loop(
    options: ExtractOptions,
    deadlines: ExecutionDeadlines,
    stop: watch::Receiver<bool>,
    mut on_snapshot: impl FnMut(&Path) -> std::io::Result<()>,
    reporting: &Reporting,
) -> Result<(), CommandError> {
    let output = options.output.clone().unwrap_or_else(|| {
        options
            .config
            .parent()
            .unwrap_or(Path::new("."))
            .join(".diplodocus/documentation.sqlite")
    });
    let mut watcher = Watcher::new(&options.config, &output, reporting)?;
    let mut initial = true;
    loop {
        if !initial {
            tokio::select! {
                _ = cancelled(stop.clone()) => return Ok(()),
                _ = watcher.next_change() => {},
            }
        }
        let command = reporting.command("extract");
        let mut attempt = pipeline::AttemptState::watching();
        let result = pipeline::extract_attempt_observed(
            options.clone(),
            None,
            deadlines,
            Box::pin(cancelled(stop.clone())),
            reporting,
            &mut attempt,
        )
        .await;
        watcher.acknowledge(attempt);
        let retained_previous = match result {
            Ok(path) => {
                on_snapshot(&pipeline::absolute(&path)?)?;
                command.finish(Some(&path), None);
                false
            }
            Err(error) => {
                drop(command);
                if *stop.borrow() && clean_cancellation(&error) {
                    return Ok(());
                }
                if initial || *stop.borrow() {
                    return Err(error);
                }
                reporting.error(&error);
                true
            }
        };
        reporting.emit(Event::Watching { retained_previous });
        initial = false;
    }
}

pub(super) struct Watcher {
    reporting: Reporting,
    observer: Observer,
    attempted: Observation,
    pending: Observation,
    changed: Instant,
    interval: tokio::time::Interval,
}

impl Watcher {
    pub(super) fn new(
        config: &Path,
        output: &Path,
        reporting: &Reporting,
    ) -> Result<Self, CommandError> {
        let timing = reporting.phase(Phase::WatchCapture, None, None);
        let mut observer = Observer::new(config, output)?;
        let attempted = observer.capture();
        timing.finish();
        Ok(Self {
            reporting: reporting.clone(),
            observer,
            pending: attempted.clone(),
            attempted,
            changed: Instant::now(),
            interval: tokio::time::interval(Duration::from_millis(100)),
        })
    }

    pub(super) fn acknowledge(&mut self, attempt: pipeline::AttemptState) {
        let timing = self.reporting.phase(Phase::WatchCapture, None, None);
        let previous_assets = self.observer.assets.clone();
        let previous_roots = self.observer.native_roots.clone();
        let previous_library_roots = self.observer.native_library_roots.clone();
        if let Some(inputs) = attempt.watch_inputs {
            // New subscriptions use the bytes observed during assembly, so an
            // edit after that observation still schedules another attempt.
            for (path, value) in &inputs.assets {
                self.attempted
                    .entry(path.clone())
                    .or_insert_with(|| value.clone());
            }
            self.observer.assets = inputs.assets.into_keys().collect();
            self.observer.native_roots = inputs.native_roots;
            self.observer.native_library_roots = inputs.native_library_roots;
            self.observer
                .external_fingerprints
                .extend(inputs.external_fingerprints);
        }
        let observed = self.observer.capture();
        for path in previous_assets.difference(&self.observer.assets) {
            if !observed.contains_key(path) {
                self.attempted.remove(path);
            }
        }
        self.attempted.retain(|path, _| {
            observed.contains_key(path)
                || !previous_roots
                    .iter()
                    .chain(&previous_library_roots)
                    .any(|root| path.starts_with(root))
                || self
                    .observer
                    .native_roots
                    .iter()
                    .chain(&self.observer.native_library_roots)
                    .any(|root| path.starts_with(root))
        });
        self.attempted.retain(|path, value| {
            !(value == "NotFound"
                && path.is_dir()
                && attempt.writes.keys().any(|output| output.starts_with(path)))
        });
        for (path, value) in &observed {
            if path.is_dir() && attempt.writes.keys().any(|output| output.starts_with(path)) {
                self.attempted
                    .entry(path.clone())
                    .or_insert_with(|| value.clone());
            }
            if value == "NotFound"
                && attempt
                    .writes
                    .iter()
                    .any(|(output, fingerprint)| fingerprint.is_none() && output.starts_with(path))
            {
                self.attempted
                    .entry(path.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        for (path, fingerprint) in attempt.writes {
            match fingerprint {
                Some(fingerprint) => {
                    self.attempted.insert(path, fingerprint.value);
                }
                None => {
                    if observed.contains_key(&path) {
                        self.attempted.insert(path, "NotFound".into());
                    } else {
                        self.attempted.remove(&path);
                    }
                }
            }
        }
        self.pending = self.attempted.clone();
        timing.finish();
    }

    pub(super) async fn next_change(&mut self) -> BTreeSet<PathBuf> {
        loop {
            self.interval.tick().await;
            let timing = self.reporting.phase(Phase::WatchPoll, None, None);
            let observed = self.observer.capture();
            timing.finish();
            // Wait a full period after scanning. A build or a slow scan can
            // leave ticks perpetually overdue and starve shutdown in select!.
            self.interval.reset();
            if observed != self.pending {
                self.pending = observed;
                self.changed = Instant::now();
            }
            if self.pending == self.attempted || self.changed.elapsed() < Duration::from_millis(200)
            {
                continue;
            }
            let paths = self
                .attempted
                .keys()
                .chain(self.pending.keys())
                .filter(|path| self.attempted.get(*path) != self.pending.get(*path))
                .cloned()
                .collect();
            self.attempted = self.pending.clone();
            return paths;
        }
    }
}

pub(super) async fn cancelled(mut receiver: watch::Receiver<bool>) {
    if !*receiver.borrow() {
        let _ = receiver.wait_for(|value| *value).await;
    }
}

pub(super) fn clean_cancellation(error: &CommandError) -> bool {
    match error {
        CommandError::Assembly(AssemblyError::Cancelled) => true,
        CommandError::Assembly(AssemblyError::Execution(failure)) => {
            failure.kind == crate::execution::ExecutionFailureKind::Cancelled
                && failure.cleanup_diagnostics.is_empty()
        }
        _ => false,
    }
}
pub(super) async fn termination() {
    #[cfg(unix)]
    {
        let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

type Observation = BTreeMap<PathBuf, String>;
type ExternalFingerprints = BTreeMap<PathBuf, (ExternalSignature, String)>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataSignature {
    length: u64,
    modified: Option<std::time::SystemTime>,
    directory: bool,
    #[cfg(unix)]
    unix: (u64, u64, i64, i64, i64, i64, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalSignature {
    target: MetadataSignature,
    link: Option<MetadataSignature>,
}

impl MetadataSignature {
    fn new(metadata: &fs::Metadata) -> Self {
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            directory: metadata.is_dir(),
            #[cfg(unix)]
            unix: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                    metadata.mode(),
                )
            },
        }
    }
}

struct Observer {
    config: PathBuf,
    output: PathBuf,
    configuration: Option<WorkspaceConfiguration>,
    assets: BTreeSet<PathBuf>,
    native_roots: Vec<PathBuf>,
    native_library_roots: Vec<PathBuf>,
    external_fingerprints: ExternalFingerprints,
}
impl Observer {
    fn new(config: &Path, output: &Path) -> Result<Self, CommandError> {
        let observer = Self {
            config: pipeline::absolute(
                config
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?
            .join(config.file_name().ok_or(CommandError::InputOverlap)?),
            output: pipeline::absolute(output)?,
            configuration: None,
            assets: BTreeSet::new(),
            native_roots: Vec::new(),
            native_library_roots: Vec::new(),
            external_fingerprints: BTreeMap::new(),
        };
        Ok(observer)
    }
    fn capture(&mut self) -> Observation {
        let mut observed = BTreeMap::new();
        observe(&self.config, &mut observed);
        if let Ok(text) = fs::read_to_string(&self.config)
            && let Ok(config) = crate::configuration::parse_configuration(&text)
        {
            self.configuration = Some(config);
        }
        let Some(config) = &self.configuration else {
            return observed;
        };
        let parent = self.config.parent().expect("absolute config");
        for bundle in &config.package_bundles {
            let Some(path) = &bundle.path else { continue };
            let path = parent.join(path);
            observe_external(&path, &mut observed, &mut self.external_fingerprints);
            for sidecar in crate::snapshots::sqlite_sidecar_paths(&path) {
                // Bundle loading checks directory entries, including dangling symlinks.
                let state = match fs::symlink_metadata(&sidecar) {
                    Ok(_) => "Present".to_owned(),
                    Err(error) => format!("{:?}", error.kind()),
                };
                observed.insert(sidecar, state);
            }
        }
        let roots: BTreeMap<_, _> = config
            .repositories
            .iter()
            .filter_map(|r| {
                pipeline::absolute(&parent.join(&r.path))
                    .ok()
                    .map(|path| (r.id.as_str(), path))
            })
            .collect();
        for root in &self.native_roots {
            if roots
                .values()
                .any(|repository| root.starts_with(repository))
            {
                discover_doxygen(root, &self.output, &mut observed);
            } else {
                discover_external(
                    root,
                    &self.output,
                    &mut observed,
                    &mut self.external_fingerprints,
                    &mut BTreeSet::new(),
                );
            }
        }
        for root in &self.native_library_roots {
            let maintained = roots
                .values()
                .any(|repository| root.starts_with(repository));
            discover_native_libraries(
                root,
                &self.output,
                maintained,
                &mut observed,
                &mut self.external_fingerprints,
            );
        }
        for asset in &self.assets {
            if roots.values().any(|root| asset.starts_with(root)) {
                observe(asset, &mut observed);
            } else {
                observe_external(asset, &mut observed, &mut self.external_fingerprints);
            }
        }
        if let Some(logo) = &config.presentation.logo
            && let Some(root) = roots.get(logo.repository.as_str())
        {
            observe(&root.join(logo.path.as_str()), &mut observed);
        }
        if let Some(image) = &config.presentation.open_graph
            && let Some(root) = roots.get(image.repository.as_str())
        {
            observe(&root.join(image.path.as_str()), &mut observed);
        }
        for asset in &config.presentation.assets {
            if let Some(root) = roots.get(asset.repository.as_str()) {
                observe(&root.join(asset.path.as_str()), &mut observed);
            }
        }
        for component in &config.components {
            if let Some(root) = roots.get(component.repository.as_str()) {
                fn bundle(path: &Path, observed: &mut Observation) {
                    observe(path, observed);
                    if path.is_symlink() {
                        return;
                    }
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            bundle(&entry.path(), observed);
                        }
                    }
                }
                bundle(&root.join(&component.path), &mut observed);
            }
        }
        for package in &config.packages {
            let Some(root) = roots.get(package.repository.as_str()) else {
                continue;
            };
            self.incremental_inputs(&package.incremental, root, &mut observed);
            let Ok(root) = pipeline::absolute(&root.join(&package.path)) else {
                continue;
            };
            observe(&root.join(&package.metadata_path), &mut observed);
            observe(&root.join("README.md"), &mut observed);
            if let Some(overview) = &package.overview {
                observe(&root.join(overview), &mut observed);
            }
            for name in crate::assembly::PACKAGE_LICENSE_FILENAMES {
                observe(&root.join(name), &mut observed);
            }
            if package.ecosystem == "r" {
                observe(&root.join("NAMESPACE"), &mut observed);
                for name in ["DESCRIPTION", ".Rbuildignore", "renv.lock"] {
                    observe(&root.join(name), &mut observed);
                }
                let mut native_inputs = Vec::new();
                for directory in ["man", "src", "inst", "data"] {
                    discover(
                        &root.join(directory),
                        &self.output,
                        &mut observed,
                        &mut native_inputs,
                        &|_| true,
                    );
                }
                if let Some(settings) = &package.r {
                    for input in settings
                        .extra_inputs
                        .iter()
                        .chain(&settings.generated_outputs)
                    {
                        let input = root.join(input);
                        let input = fs::canonicalize(&input).unwrap_or(input);
                        discover(
                            &input,
                            &self.output,
                            &mut observed,
                            &mut native_inputs,
                            &|_| true,
                        );
                    }
                }

                let mut vignettes = Vec::new();
                discover(
                    &root.join("vignettes"),
                    &self.output,
                    &mut observed,
                    &mut vignettes,
                    &|_| true,
                );
                for name in [
                    "NEWS.md",
                    "NEWS.Rd",
                    "NEWS",
                    "inst/CITATION",
                    "CONTRIBUTING.md",
                    "CODE_OF_CONDUCT.md",
                ] {
                    observe(&root.join(name), &mut observed);
                }
                for name in ["CONTRIBUTING.md", "CODE_OF_CONDUCT.md"] {
                    observe(
                        &roots[package.repository.as_str()].join(name),
                        &mut observed,
                    );
                }
                if let Ok(text) = fs::read_to_string(root.join(&package.metadata_path)) {
                    let metadata = arity_parser::dcf::parse(&text);
                    for field in metadata
                        .document()
                        .fields()
                        .filter(|field| field.name() == "License")
                    {
                        for name in crate::extractors::r::license_files(&field.folded_value()) {
                            observe(&root.join(name), &mut observed);
                        }
                    }
                }
            }
            for target in &package.targets {
                if target.extractor == "doxygen" {
                    discover_doxygen(&root, &self.output, &mut observed);
                    observe(&root.join(&target.path), &mut observed);
                    continue;
                }
                let mut selected = Vec::new();
                if let Some(settings) = &target.julia {
                    for extension in &settings.extensions {
                        observe(&root.join(extension), &mut observed);
                    }
                }
                if target.extractor == "rust" {
                    discover_rust(&root, &self.output, &mut observed);
                    let repository = &roots[package.repository.as_str()];
                    for ancestor in root
                        .ancestors()
                        .take_while(|path| path.starts_with(repository))
                    {
                        for name in [
                            "Cargo.toml",
                            "Cargo.lock",
                            "rust-toolchain.toml",
                            "rust-toolchain",
                            ".cargo/config",
                            ".cargo/config.toml",
                        ] {
                            observe(&ancestor.join(name), &mut observed);
                        }
                    }
                    continue;
                }
                discover(
                    &root.join(&target.path),
                    &self.output,
                    &mut observed,
                    &mut selected,
                    &|path| {
                        matches!(
                            path.extension().and_then(|s| s.to_str()),
                            Some("py" | "pyi" | "R" | "r" | "Rd" | "jl")
                        ) || path.file_name().is_some_and(|s| s == "py.typed")
                    },
                );
            }
        }
        for content in &config.content {
            let Some(root) = roots.get(content.repository.as_str()) else {
                continue;
            };
            self.incremental_inputs(&content.incremental, root, &mut observed);
            for input in &content.execution.declared_environment_inputs {
                observe(&root.join(input), &mut observed);
            }
            let mut pages = Vec::new();
            discover(
                &root.join(&content.path),
                &self.output,
                &mut observed,
                &mut pages,
                &|p| {
                    content.format == crate::documents::AuthoredFormat::Rmd
                        || content.format.matches_path(p)
                },
            );
            for page in pages {
                if content.format == crate::documents::AuthoredFormat::Rmd
                    && !content.format.matches_path(&page)
                {
                    continue;
                }
                self.authored_assets(&page, content, root, &mut observed);
            }
        }
        self.external_fingerprints
            .retain(|path, _| observed.contains_key(path));
        observed
    }
    fn incremental_inputs(
        &self,
        settings: &crate::configuration::IncrementalConfiguration,
        root: &Path,
        observed: &mut Observation,
    ) {
        for path in &settings.extra_inputs {
            discover_incremental(root, path, &self.output, observed, &mut BTreeSet::new());
        }
    }

    // Tests simulate the subscriptions supplied by a completed assembly.
    #[cfg(test)]
    fn refresh_assets(&mut self) {
        let Ok(sources) = crate::assembly::assemble_workspace(&self.config) else {
            return;
        };
        let inputs =
            SourceWatchInputs::capture(&sources, &self.output, &Reporting::default(), None);
        self.assets = inputs.assets.into_keys().collect();
        self.native_roots = inputs.native_roots;
    }
    fn authored_assets(
        &self,
        path: &Path,
        content: &ContentConfiguration,
        root: &Path,
        observed: &mut Observation,
    ) {
        if !fs::metadata(path).is_ok_and(|m| m.is_file()) {
            return;
        }
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let repository = crate::paths::ResolvedRepositoryPaths {
            id: content.repository.clone(),
            path: root.to_owned(),
        };
        if let Ok(collection) = pipeline::absolute(&root.join(&content.path)) {
            let source_root = if collection.is_file() {
                collection.parent().expect("absolute collection")
            } else {
                &collection
            };
            for input in crate::assembly::include_watch_inputs(
                &text,
                path,
                source_root,
                &repository,
                content.format,
            ) {
                if let Ok(input) = pipeline::absolute(&input)
                    && input.starts_with(root)
                    && !input.starts_with(&self.output)
                    && !crate::generated_storage::is_generated_file(&input)
                {
                    observe(&input, observed);
                }
            }
        }
        let Ok(prepared) = crate::documents::prepare_collection_document(&text, content) else {
            return;
        };
        self.document_assets(path, &prepared.parsed.document, root, observed);
    }
    fn document_assets(
        &self,
        path: &Path,
        document: &crate::ir::Document,
        root: &Path,
        observed: &mut Observation,
    ) {
        crate::validation::document_references(
            &document.blocks,
            None,
            &mut |kind, spelling, _, _| {
                if kind == crate::validation::ReferenceKind::Semantic
                    || url::Url::parse(spelling).is_ok()
                {
                    return;
                }
                let file = spelling.split('#').next().unwrap_or("");
                if file.is_empty() {
                    return;
                }
                let Ok(file) = percent_encoding::percent_decode_str(file).decode_utf8() else {
                    return;
                };
                if Path::new(file.as_ref()).is_absolute() {
                    return;
                }
                let Some(parent) = path.parent() else {
                    return;
                };
                let Ok(candidate) = pipeline::absolute(&parent.join(file.as_ref())) else {
                    return;
                };
                let Ok(root) = pipeline::absolute(root) else {
                    return;
                };
                if candidate.starts_with(root) && !candidate.starts_with(&self.output) {
                    observe(&candidate, observed);
                }
            },
        );
    }
}
#[derive(Default)]
pub(super) struct SourceWatchInputs {
    assets: Observation,
    native_roots: Vec<PathBuf>,
    native_library_roots: Vec<PathBuf>,
    external_fingerprints: ExternalFingerprints,
}

fn minimize_native_roots(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<_> = roots
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    roots.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    let mut selected = BTreeSet::<PathBuf>::new();
    let mut ordered = Vec::new();
    for root in roots {
        let covered = root.ancestors().any(|parent| {
            selected.contains(parent)
                && root.strip_prefix(parent).is_ok_and(|relative| {
                    !relative.components().any(|component| {
                        crate::extractors::doxygen::configuration::excluded(
                            &component.as_os_str().to_string_lossy(),
                        )
                    })
                })
        });
        if !covered {
            selected.insert(root.clone());
            ordered.push(root);
        }
    }
    ordered
}

impl SourceWatchInputs {
    pub(super) fn capture(
        sources: &crate::assembly::WorkspaceSources,
        output: &Path,
        reporting: &Reporting,
        previous: Option<Self>,
    ) -> Self {
        let timing = reporting.phase(Phase::WatchCapture, None, None);
        let observer = Observer {
            config: PathBuf::new(),
            output: output.to_owned(),
            configuration: None,
            assets: BTreeSet::new(),
            native_roots: Vec::new(),
            native_library_roots: Vec::new(),
            external_fingerprints: BTreeMap::new(),
        };
        let workspace = sources.workspace();
        let native_roots = minimize_native_roots(
            sources
                .doxygen_watch_roots()
                .chain(sources.dependency_watch_roots())
                .cloned(),
        );
        let native_library_roots = sources
            .dependency_library_roots()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut inputs = Self::from_previous(previous);
        inputs.native_roots = native_roots;
        inputs.native_library_roots = native_library_roots;
        inputs.observe_paths(sources.input_paths(), |path| {
            sources
                .paths()
                .repositories
                .iter()
                .any(|root| path.starts_with(&root.path))
        });
        let documents = workspace
            .pages
            .values()
            .map(|p| &p.document)
            .chain(
                workspace
                    .packages
                    .values()
                    .flat_map(|p| p.items.values().filter_map(|i| i.documentation.as_ref())),
            )
            .chain(
                workspace
                    .concepts
                    .values()
                    .filter_map(|c| c.documentation.as_ref()),
            );
        for document in documents {
            let Some(source) = &document.source_location else {
                continue;
            };
            let Some(root) = sources
                .paths()
                .repositories
                .iter()
                .find(|r| r.id == source.repository)
            else {
                continue;
            };
            observer.document_assets(
                &root.path.join(source.path.as_str()),
                &document.document,
                &root.path,
                &mut inputs.assets,
            );
        }
        inputs.retire_fingerprints();
        timing.finish();
        inputs
    }

    fn from_previous(previous: Option<Self>) -> Self {
        Self {
            external_fingerprints: previous
                .map(|inputs| inputs.external_fingerprints)
                .unwrap_or_default(),
            ..Self::default()
        }
    }

    fn observe_paths(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        maintained: impl Fn(&Path) -> bool,
    ) {
        for path in paths.into_iter().collect::<BTreeSet<_>>() {
            if maintained(&path) {
                observe(&path, &mut self.assets);
            } else {
                observe_external(&path, &mut self.assets, &mut self.external_fingerprints);
            }
        }
    }

    fn retire_fingerprints(&mut self) {
        self.external_fingerprints
            .retain(|path, _| self.assets.contains_key(path));
    }
}

fn observe(path: &Path, observed: &mut Observation) {
    if observed.contains_key(path) {
        return;
    }
    let value = match fs::metadata(path).and_then(|metadata| {
        if metadata.is_file() {
            fs::read(path)
        } else {
            Err(std::io::Error::other("not a regular input file"))
        }
    }) {
        Ok(bytes) => fingerprint_bytes(&bytes).value,
        Err(error) => format!("{:?}", error.kind()),
    };
    observed.insert(path.to_owned(), value);
}

fn observe_external(path: &Path, observed: &mut Observation, cache: &mut ExternalFingerprints) {
    if observed.contains_key(path) {
        return;
    }
    let value = (|| -> std::io::Result<String> {
        let link = fs::symlink_metadata(path)?;
        let metadata = if link.is_symlink() {
            fs::metadata(path)?
        } else {
            link.clone()
        };
        let signature = ExternalSignature {
            target: MetadataSignature::new(&metadata),
            link: link.is_symlink().then(|| MetadataSignature::new(&link)),
        };
        if metadata.is_dir() {
            // Recursive discovery tracks children without reacting to ignored cache writes.
            #[cfg(unix)]
            return Ok(format!(
                "Directory:{}:{}:{}",
                signature.target.unix.0, signature.target.unix.1, signature.target.unix.6
            ));
            #[cfg(not(unix))]
            return Ok(format!("Directory:{:?}", fs::canonicalize(path)?));
        }
        if metadata.is_file() {
            if let Some((previous, value)) = cache.get(path)
                && previous == &signature
            {
                return Ok(value.clone());
            }
            let fingerprint = crate::incremental::file_digest(path)?.value;
            let value = format!("File:{fingerprint}:{signature:?}");
            cache.insert(path.to_owned(), (signature, value.clone()));
            return Ok(value);
        }
        Err(std::io::Error::other("not a regular external dependency"))
    })()
    .unwrap_or_else(|error| {
        cache.remove(path);
        format!("{:?}", error.kind())
    });
    observed.insert(path.to_owned(), value);
}

fn discover_external(
    root: &Path,
    output: &Path,
    observed: &mut Observation,
    cache: &mut ExternalFingerprints,
    active: &mut BTreeSet<PathBuf>,
) {
    if root.starts_with(output) || crate::generated_storage::is_reserved(root) {
        return;
    }
    let directory = root.is_dir();
    if directory && generated_site(root) {
        return;
    }
    observe_external(root, observed, cache);
    if !directory {
        return;
    }
    let Ok(canonical) = fs::canonicalize(root) else {
        return;
    };
    if !active.insert(canonical.clone()) {
        return;
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
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
                discover_external(&entry.path(), output, observed, cache, active);
            }
        }
    }
    active.remove(&canonical);
}

fn discover_native_libraries(
    root: &Path,
    output: &Path,
    maintained: bool,
    observed: &mut Observation,
    cache: &mut ExternalFingerprints,
) {
    if root.starts_with(output) || crate::generated_storage::is_reserved(root) {
        return;
    }
    if maintained {
        observe(root, observed);
    } else {
        observe_external(root, observed, cache);
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if crate::incremental::native_library_file(&path) && !path.is_dir() {
                if maintained {
                    observe(&path, observed);
                } else {
                    observe_external(&path, observed, cache);
                }
            }
        }
    }
}

fn discover_incremental(
    repository: &Path,
    declared: &Path,
    output: &Path,
    observed: &mut Observation,
    active: &mut BTreeSet<PathBuf>,
) {
    let path = repository.join(declared);
    if crate::generated_storage::is_reserved(&path)
        || path.starts_with(output)
        || path.file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some(
                    ".git"
                        | ".devenv"
                        | ".direnv"
                        | "target"
                        | "node_modules"
                        | "__pycache__"
                        | ".pytest_cache"
                )
            )
        })
    {
        return;
    }
    if crate::incremental::checked_incremental_path(repository, declared).is_err() {
        observed.insert(path, "InvalidBoundary".into());
        return;
    }
    if !path.is_dir() {
        if !crate::generated_storage::is_generated_file(&path) {
            observe(&path, observed);
        }
        return;
    }
    if generated_site(&path) {
        return;
    }
    let Ok(canonical) = fs::canonicalize(&path) else {
        observe(&path, observed);
        return;
    };
    if canonical.starts_with(output) {
        return;
    }
    observed.insert(path.clone(), format!("Directory:{canonical:?}"));
    if !active.insert(canonical.clone()) {
        return;
    }
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            discover_incremental(
                repository,
                &declared.join(entry.file_name()),
                output,
                observed,
                active,
            );
        }
    }
    active.remove(&canonical);
}

fn discover_doxygen(root: &Path, output: &Path, observed: &mut Observation) {
    if root.starts_with(output) || crate::generated_storage::is_reserved(root) {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(root) else {
        observe(root, observed);
        return;
    };
    if metadata.is_file() || metadata.is_symlink() {
        if !crate::generated_storage::is_generated_file(root) {
            observe(root, observed);
        }
    } else if let Ok(entries) = fs::read_dir(root) {
        if generated_site(root) {
            return;
        }
        for entry in entries.flatten() {
            if !crate::extractors::doxygen::configuration::excluded(
                &entry.file_name().to_string_lossy(),
            ) {
                discover_doxygen(&entry.path(), output, observed);
            }
        }
    }
}

fn generated_site(path: &Path) -> bool {
    fs::read(path.join(".diplodocus-site")).is_ok_and(|bytes| bytes == b"diplodocus-site-v1\n")
}

fn discover_rust(root: &Path, output: &Path, observed: &mut Observation) {
    if root.starts_with(output)
        || crate::generated_storage::is_reserved(root)
        || root.file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some(".git" | "target" | "node_modules" | ".svelte-kit" | "dist" | "site")
            )
        })
    {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(root) else {
        return;
    };
    if metadata.is_symlink() {
        observe(root, observed);
    } else if metadata.is_file() {
        if !crate::generated_storage::is_generated_file(root) {
            observe(root, observed);
        }
    } else if metadata.is_dir() {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            discover_rust(&entry.path(), output, observed);
        }
    }
}
fn discover(
    root: &Path,
    output: &Path,
    observed: &mut Observation,
    selected: &mut Vec<PathBuf>,
    accept: &impl Fn(&Path) -> bool,
) {
    if root.starts_with(output)
        || root.file_name().is_some_and(|name| name == ".git")
        || crate::generated_storage::is_reserved(root)
    {
        return;
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(_) => {
            observe(root, observed);
            return;
        }
    };
    if metadata.is_dir() {
        let Ok(entries) = fs::read_dir(root) else {
            observe(root, observed);
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            discover(&entry.path(), output, observed, selected, accept);
        }
    } else if metadata.file_type().is_symlink() && fs::metadata(root).is_ok_and(|m| m.is_dir()) {
        observed.insert(
            root.into(),
            fs::read_link(root)
                .map(|p| format!("symlink:{p:?}"))
                .unwrap_or_default(),
        );
    } else if accept(root) {
        if let Ok(path) = fs::canonicalize(root)
            && crate::generated_storage::is_generated_file(&path)
        {
            return;
        }
        observe(root, observed);
        selected.push(root.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::assemble_workspace;

    #[test]
    fn removing_bundle_sidecars_changes_observation_without_repository_roots() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(
            &config,
            "project: {name: Test}\npackage-bundles:\n  - {package: example, path: example.sqlite}\n",
        )
        .unwrap();
        let bundle = root.path().join("example.sqlite");
        fs::write(&bundle, "unchanged database").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let clean = observer.capture();
        for sidecar in crate::snapshots::sqlite_sidecar_paths(&bundle) {
            fs::write(&sidecar, "").unwrap();
            assert_ne!(observer.capture(), clean);
            fs::remove_file(sidecar).unwrap();
            assert_eq!(observer.capture(), clean);
        }
    }

    fn journal_watcher(root: &Path, assets: &[&str]) -> Watcher {
        let config = root.join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: Initial\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: r\n    name: R\n    slug: r\n    ecosystem: r\n    repository: repo\n    path: .\n    metadata-path: DESCRIPTION\n    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n").unwrap();
        fs::create_dir_all(root.join("R")).unwrap();
        fs::write(root.join("R/source.R"), "f <- function() 1L\n").unwrap();
        fs::write(
            root.join("DESCRIPTION"),
            "Package: journaltest\nTitle: Journal test\nVersion: 1.0.0\n",
        )
        .unwrap();
        // Build the observer directly so these bookkeeping tests never load a package.
        let mut observer = Observer {
            config,
            output: root.join("site"),
            configuration: None,
            assets: assets.iter().map(|path| root.join(path)).collect(),
            native_roots: Vec::new(),
            native_library_roots: Vec::new(),
            external_fingerprints: BTreeMap::new(),
        };
        let attempted = observer.capture();
        Watcher {
            reporting: Reporting::default(),
            observer,
            pending: attempted.clone(),
            attempted,
            changed: Instant::now(),
            interval: tokio::time::interval(Duration::from_millis(100)),
        }
    }

    fn generated_attempt(root: &Path, paths: &[&str]) -> pipeline::AttemptState {
        pipeline::AttemptState {
            writes: paths
                .iter()
                .map(|path| {
                    let path = root.join(path);
                    let fingerprint = fs::read(&path).ok().map(|bytes| fingerprint_bytes(&bytes));
                    (path, fingerprint)
                })
                .collect(),
            ..pipeline::AttemptState::default()
        }
    }

    fn subscribed_inputs(root: &Path, paths: &[&str]) -> SourceWatchInputs {
        let mut assets = BTreeMap::new();
        for path in paths {
            observe(&root.join(path), &mut assets);
        }
        SourceWatchInputs {
            assets,
            ..SourceWatchInputs::default()
        }
    }

    #[test]
    fn native_root_pruning_preserves_explicit_roots_under_excluded_directories() {
        let roots = [
            "/deps/pkg/source",
            "/deps",
            "/deps/target/pkg",
            "/deps/target",
            "/deps/pkg",
            "/deps/target/pkg/.git/config",
            "/deps/node_modules/pkg",
            "/deps/node_modules/pkg/include",
            "/elsewhere/file",
            "/deps",
            "/deps/node_modules/pkg",
            "/deps/target/pkg/.git/config",
        ];
        assert_eq!(
            minimize_native_roots(roots.into_iter().map(PathBuf::from)),
            [
                "/deps",
                "/deps/target",
                "/elsewhere/file",
                "/deps/node_modules/pkg",
                "/deps/target/pkg/.git/config",
            ]
            .map(PathBuf::from)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn subscription_memos_recheck_mutations_between_captures_and_retire_removed_paths() {
        let root = tempfile::tempdir().unwrap();
        let dependencies = tempfile::tempdir().unwrap();
        let file = dependencies.path().join("retained");
        let removed = dependencies.path().join("removed");
        fs::write(&file, "first").unwrap();
        fs::write(&removed, "retired").unwrap();
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        let mut initial = SourceWatchInputs::from_previous(None);
        initial.observe_paths([file.clone(), removed.clone(), file.clone()], |_| false);
        initial.retire_fingerprints();
        let before = initial.assets[&file].clone();
        assert_eq!(initial.external_fingerprints.len(), 2);

        fs::write(&file, "other").unwrap();
        fs::File::open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let mut final_inputs = SourceWatchInputs::from_previous(Some(initial));
        final_inputs.observe_paths([file.clone()], |_| false);
        final_inputs.retire_fingerprints();
        assert_ne!(final_inputs.assets[&file], before);
        assert!(
            final_inputs.assets[&file]
                .starts_with(&format!("File:{}:", fingerprint_bytes(b"other").value))
        );
        assert_eq!(final_inputs.external_fingerprints.len(), 1);
        assert!(!final_inputs.external_fingerprints.contains_key(&removed));

        let mut watcher = journal_watcher(root.path(), &[]);
        watcher.acknowledge(pipeline::AttemptState {
            watch_inputs: Some(final_inputs),
            ..pipeline::AttemptState::default()
        });
        assert_eq!(watcher.observer.capture(), watcher.attempted);
        assert!(watcher.observer.external_fingerprints.contains_key(&file));
        watcher.acknowledge(pipeline::AttemptState {
            watch_inputs: Some(SourceWatchInputs::default()),
            ..pipeline::AttemptState::default()
        });
        assert!(watcher.observer.external_fingerprints.is_empty());
    }

    #[test]
    fn external_dependencies_detect_content_and_membership_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("dependency");
        fs::write(&file, "first").unwrap();
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        let mut cache = BTreeMap::new();
        let mut first = Observation::new();
        observe_external(&file, &mut first, &mut cache);
        let mut repeated = Observation::new();
        observe_external(&file, &mut repeated, &mut cache);
        assert_eq!(first, repeated);
        assert_eq!(cache.len(), 1);

        fs::write(&file, "other").unwrap();
        fs::File::open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let mut edited = Observation::new();
        observe_external(&file, &mut edited, &mut cache);
        assert_ne!(first, edited);

        let mut directory = Observation::new();
        discover_external(
            root.path(),
            &root.path().join("site"),
            &mut directory,
            &mut cache,
            &mut BTreeSet::new(),
        );
        fs::create_dir(root.path().join("new")).unwrap();
        let mut added = Observation::new();
        discover_external(
            root.path(),
            &root.path().join("site"),
            &mut added,
            &mut cache,
            &mut BTreeSet::new(),
        );
        assert_ne!(directory, added);

        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&file, &alias).unwrap();
            let mut linked = Observation::new();
            observe_external(&alias, &mut linked, &mut cache);
            let target = root.path().join("replacement");
            fs::write(&target, "other").unwrap();
            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&target, &alias).unwrap();
            let mut replaced = Observation::new();
            observe_external(&alias, &mut replaced, &mut cache);
            assert_ne!(linked, replaced);
        }
    }

    #[test]
    fn external_dependency_roots_discover_new_files_and_retire_memos() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("repository")).unwrap();
        let dependencies = root.path().join("dependencies");
        fs::create_dir(&dependencies).unwrap();
        let file = dependencies.join("first");
        fs::write(&file, "first").unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(
            &config,
            "project:\n  name: Dependencies\nrepositories:\n  - id: repo\n    path: repository\n",
        )
        .unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        observer.native_roots.push(dependencies.clone());
        observer.assets.insert(file.clone());
        let first = observer.capture();
        assert!(first.contains_key(&file));
        assert!(observer.external_fingerprints.contains_key(&file));
        fs::create_dir(dependencies.join("__pycache__")).unwrap();
        fs::write(dependencies.join("__pycache__/runtime.pyc"), "generated").unwrap();
        assert_eq!(observer.capture(), first);
        let previous_site = dependencies.join("previous-output");
        fs::create_dir(&previous_site).unwrap();
        fs::write(
            previous_site.join(".diplodocus-site"),
            "diplodocus-site-v1\n",
        )
        .unwrap();
        fs::write(previous_site.join("index.html"), "generated").unwrap();
        assert_eq!(observer.capture(), first);
        let added = dependencies.join("added");
        fs::write(&added, "added").unwrap();
        assert!(observer.capture().contains_key(&added));
        fs::remove_file(&file).unwrap();
        assert_eq!(observer.capture().get(&file).unwrap(), "NotFound");
        assert!(!observer.external_fingerprints.contains_key(&file));
        observer.assets.clear();
        assert!(!observer.capture().contains_key(&file));
        observer.native_roots.clear();
        observer.capture();
        assert!(observer.external_fingerprints.is_empty());
    }

    #[test]
    fn native_library_search_roots_ignore_unrelated_files_and_subdirectories() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("repository")).unwrap();
        let libraries = root.path().join("libraries");
        fs::create_dir(&libraries).unwrap();
        let library = libraries.join("libexample.so");
        fs::write(&library, "first").unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(
            &config,
            "project:\n  name: Libraries\nrepositories:\n  - id: repo\n    path: repository\n",
        )
        .unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        observer.native_library_roots.push(libraries.clone());
        let first = observer.capture();
        assert!(first.contains_key(&library));
        let unrelated = libraries.join("unrelated.rlib");
        fs::write(&unrelated, "compiler output").unwrap();
        fs::create_dir(libraries.join("nested.so")).unwrap();
        fs::write(libraries.join("nested.so/libignored.so"), "nested output").unwrap();
        assert_eq!(observer.capture(), first);
        assert!(!observer.external_fingerprints.contains_key(&unrelated));
        let added = libraries.join("libadded.so.1");
        fs::write(&added, "added").unwrap();
        let changed = observer.capture();
        assert!(changed.contains_key(&added));
        assert_ne!(changed, first);
        fs::write(&library, "other").unwrap();
        assert_ne!(observer.capture(), changed);
    }

    #[tokio::test(start_paused = true)]
    async fn changed_paths_include_edits_creations_and_deletions() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        let source = root.path().join("R/source.R");
        fs::write(&source, "f <- function() 2L\n").unwrap();
        assert_eq!(
            watcher.next_change().await,
            BTreeSet::from([source.clone()])
        );
        let added = root.path().join("R/added.R");
        fs::write(&added, "g <- function() 1L\n").unwrap();
        assert_eq!(watcher.next_change().await, BTreeSet::from([added]));
        fs::remove_file(&source).unwrap();
        assert_eq!(watcher.next_change().await, BTreeSet::from([source]));
    }

    #[test]
    fn incremental_extra_inputs_use_the_repository_root() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::create_dir(root.path().join("package")).unwrap();
        fs::write(root.path().join("package/manifest"), "metadata").unwrap();
        fs::write(&config, "project:\n  name: Extra inputs\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: pkg\n    name: Package\n    slug: package\n    ecosystem: python\n    repository: repo\n    path: package\n    metadata-path: manifest\n    incremental:\n      extra-inputs: [shared]\n    targets: []\ncontent:\n  - id: guide\n    owner: project\n    repository: repo\n    path: guide\n    mount: guide\n    format: gfm\n    incremental:\n      extra-inputs: [environment.lock]\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let before = observer.capture();
        fs::create_dir(root.path().join("shared")).unwrap();
        fs::write(root.path().join("shared/input"), "first").unwrap();
        let created = observer.capture();
        assert_ne!(created, before);
        fs::create_dir(root.path().join("shared/empty")).unwrap();
        assert_ne!(observer.capture(), created);
        fs::remove_dir(root.path().join("shared/empty")).unwrap();
        fs::write(root.path().join("shared/input"), "second").unwrap();
        assert_ne!(observer.capture(), created);
        fs::remove_dir_all(root.path().join("shared")).unwrap();
        assert_eq!(observer.capture(), before);
        fs::write(root.path().join("environment.lock"), "environment").unwrap();
        assert_ne!(observer.capture(), before);
    }

    #[tokio::test]
    async fn generated_creation_acknowledges_missing_directories_without_rebuilding() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        assert_eq!(watcher.attempted[&root.path().join("man")], "NotFound");
        fs::create_dir(root.path().join("man")).unwrap();
        fs::write(
            root.path().join("man/generated.Rd"),
            "% Generated by roxygen2:\n",
        )
        .unwrap();
        fs::write(
            root.path().join("NAMESPACE"),
            "# Generated by roxygen2:\nexport(f)\n",
        )
        .unwrap();
        let mut attempt = generated_attempt(root.path(), &["man/generated.Rd", "NAMESPACE"]);
        attempt.watch_inputs = Some(subscribed_inputs(root.path(), &["man", "man/generated.Rd"]));
        watcher.acknowledge(attempt);
        assert_eq!(watcher.observer.capture(), watcher.attempted);
    }

    #[tokio::test]
    async fn generated_acknowledgment_preserves_concurrent_maintained_edits() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("man")).unwrap();
        fs::write(root.path().join("man/maintained.Rd"), "Maintained topic.\n").unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        let before = watcher.attempted.clone();
        fs::write(
            root.path().join("man/generated.Rd"),
            "% Generated by roxygen2:\n",
        )
        .unwrap();
        let attempt = generated_attempt(root.path(), &["man/generated.Rd"]);
        fs::write(root.path().join("R/source.R"), "f <- function() 2L\n").unwrap();
        fs::write(
            root.path().join("man/maintained.Rd"),
            "Changed maintained topic.\n",
        )
        .unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(
            &config,
            fs::read_to_string(&config)
                .unwrap()
                .replace("Initial", "Changed"),
        )
        .unwrap();
        watcher.acknowledge(attempt);
        let observed = watcher.observer.capture();
        for path in ["R/source.R", "man/maintained.Rd", "diplodocus.yaml"] {
            let path = root.path().join(path);
            assert_eq!(watcher.attempted[&path], before[&path]);
            assert_ne!(observed[&path], watcher.attempted[&path]);
        }
        let generated = root.path().join("man/generated.Rd");
        assert_eq!(observed[&generated], watcher.attempted[&generated]);
    }

    #[tokio::test]
    async fn generated_deletion_acknowledges_success_and_failure_subscriptions() {
        for successful in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("man")).unwrap();
            fs::write(
                root.path().join("man/deleted.Rd"),
                "% Generated by roxygen2:\n",
            )
            .unwrap();
            let mut watcher = journal_watcher(root.path(), &["man/deleted.Rd"]);
            fs::remove_file(root.path().join("man/deleted.Rd")).unwrap();
            let mut attempt = generated_attempt(root.path(), &["man/deleted.Rd"]);
            if successful {
                attempt.watch_inputs = Some(subscribed_inputs(root.path(), &[]));
            }
            watcher.acknowledge(attempt);
            assert_eq!(
                watcher.observer.capture(),
                watcher.attempted,
                "successful={successful}"
            );
        }
    }

    #[tokio::test]
    async fn generated_directory_deletion_acknowledges_missing_ancestor() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("man")).unwrap();
        fs::write(
            root.path().join("man/deleted.Rd"),
            "% Generated by roxygen2:\n",
        )
        .unwrap();
        let mut watcher = journal_watcher(root.path(), &["man/deleted.Rd"]);
        fs::remove_dir_all(root.path().join("man")).unwrap();
        let attempt = generated_attempt(root.path(), &["man/deleted.Rd"]);
        watcher.acknowledge(attempt);
        assert_eq!(watcher.observer.capture(), watcher.attempted);
    }

    #[tokio::test]
    async fn generated_edits_after_journaling_remain_pending() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        fs::create_dir(root.path().join("man")).unwrap();
        let generated = root.path().join("man/generated.Rd");
        fs::write(&generated, "% Generated by roxygen2:\n").unwrap();
        let attempt = generated_attempt(root.path(), &["man/generated.Rd"]);
        fs::write(&generated, "% Generated by roxygen2:\nConcurrent edit.\n").unwrap();
        watcher.acknowledge(attempt);
        assert_ne!(
            watcher.observer.capture()[&generated],
            watcher.attempted[&generated]
        );
    }

    #[tokio::test]
    async fn new_asset_subscriptions_acknowledge_the_consumed_observation() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        let asset = root.path().join("figure.png");
        fs::write(&asset, "Figure bytes observed by assembly.").unwrap();
        let attempt = pipeline::AttemptState {
            writes: BTreeMap::new(),
            watch_inputs: Some(subscribed_inputs(root.path(), &["figure.png"])),
            ..pipeline::AttemptState::default()
        };
        watcher.acknowledge(attempt);
        assert_eq!(watcher.observer.capture(), watcher.attempted);
    }

    #[tokio::test]
    async fn new_asset_edits_after_observation_remain_pending() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = journal_watcher(root.path(), &[]);
        let asset = root.path().join("figure.png");
        fs::write(&asset, "Consumed image bytes.").unwrap();
        let attempt = pipeline::AttemptState {
            writes: BTreeMap::new(),
            watch_inputs: Some(subscribed_inputs(root.path(), &["figure.png"])),
            ..pipeline::AttemptState::default()
        };
        fs::write(&asset, "Concurrent image edit.").unwrap();
        watcher.acknowledge(attempt);
        assert_ne!(
            watcher.observer.capture()[&asset],
            watcher.attempted[&asset]
        );
    }

    #[tokio::test]
    async fn retired_asset_subscriptions_do_not_trigger_another_attempt() {
        let root = tempfile::tempdir().unwrap();
        let asset = root.path().join("figure.png");
        fs::write(&asset, "Formerly referenced image.").unwrap();
        let mut watcher = journal_watcher(root.path(), &["figure.png"]);
        watcher.acknowledge(pipeline::AttemptState {
            writes: BTreeMap::new(),
            watch_inputs: Some(subscribed_inputs(root.path(), &[])),
            ..pipeline::AttemptState::default()
        });
        assert_eq!(watcher.observer.capture(), watcher.attempted);
        assert!(!watcher.attempted.contains_key(&asset));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn package_symlink_aliases_use_canonical_generated_write_keys() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        fs::create_dir(&package).unwrap();
        let mut watcher = journal_watcher(&package, &[]);
        std::os::unix::fs::symlink(&package, root.path().join("alias")).unwrap();
        fs::write(&watcher.observer.config, "project:\n  name: Alias\nrepositories:\n  - id: repo\n    path: ..\npackages:\n  - id: r\n    name: R\n    slug: r\n    ecosystem: r\n    repository: repo\n    path: alias\n    metadata-path: DESCRIPTION\n    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n").unwrap();
        watcher.attempted = watcher.observer.capture();
        fs::create_dir(package.join("man")).unwrap();
        fs::write(
            package.join("man/generated.Rd"),
            "% Generated by roxygen2:\n",
        )
        .unwrap();
        watcher.acknowledge(generated_attempt(&package, &["man/generated.Rd"]));
        assert_eq!(watcher.observer.capture(), watcher.attempted);
        assert!(
            !watcher
                .attempted
                .keys()
                .any(|path| path.starts_with(root.path().join("alias")))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn overdue_watcher_waits_between_scans() {
        async fn poll_change_once(watcher: &mut Watcher) {
            let mut change = std::pin::pin!(watcher.next_change());
            std::future::poll_fn(|context| {
                assert!(std::future::Future::poll(change.as_mut(), context).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
        }

        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: Initial\n").unwrap();
        let mut watcher =
            Watcher::new(&config, &root.path().join("site"), &Reporting::default()).unwrap();

        // A build can leave hundreds of polling ticks overdue. Scans must still
        // wait between observations so other select branches can handle shutdown.
        tokio::time::advance(Duration::from_secs(60)).await;
        fs::write(&config, "project:\n  name: First edit\n").unwrap();
        poll_change_once(&mut watcher).await;
        let first = watcher.pending.clone();
        assert_ne!(first, watcher.attempted);

        fs::write(&config, "project:\n  name: Second edit\n").unwrap();
        tokio::task::yield_now().await;
        poll_change_once(&mut watcher).await;
        assert_eq!(watcher.pending, first);

        tokio::time::advance(Duration::from_millis(100)).await;
        poll_change_once(&mut watcher).await;
        assert_ne!(watcher.pending, first);
    }

    #[test]
    fn watches_nested_include_targets_even_when_assembly_fails() {
        for (format, extension, include) in [
            ("gfm", "md", "<!--@include: ../parts/first.md-->\n"),
            ("qmd", "qmd", "{{< include ../parts/first.md >}}\n"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("diplodocus.yaml");
            fs::write(&config, format!("project:\n  name: Includes\nrepositories:\n  - id: repo\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: repo\n    path: guide/index.{extension}\n    mount: \"\"\n    format: {format}\n")).unwrap();
            fs::create_dir(root.path().join("guide")).unwrap();
            fs::create_dir(root.path().join("parts")).unwrap();
            fs::write(
                root.path().join(format!("guide/index.{extension}")),
                include,
            )
            .unwrap();
            let first = root.path().join("parts/first.md");
            let nested = root.path().join("parts/new/nested.md");
            let directive = if format == "gfm" {
                "<!--@include: new/nested.md-->\n"
            } else {
                "{{< include new/nested.md >}}\n"
            };
            fs::write(&first, directive).unwrap();
            assert!(assemble_workspace(&config).is_err());

            let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
            let missing = observer.capture();
            assert!(missing.contains_key(&first));
            assert_eq!(missing.get(&nested).map(String::as_str), Some("NotFound"));
            fs::create_dir(root.path().join("parts/new")).unwrap();
            fs::write(&nested, "Nested content.\n").unwrap();
            let created = observer.capture();
            assert_ne!(created, missing);
            fs::write(&nested, "Edited content.\n").unwrap();
            assert_ne!(observer.capture(), created);
            fs::remove_file(&nested).unwrap();
            observer.refresh_assets();
            assert_eq!(observer.capture(), missing);
            fs::write(&nested, "Nested content.\n").unwrap();
            assert_eq!(observer.capture(), created);
        }
    }

    #[test]
    fn watches_single_file_collections_and_their_assets_without_selecting_siblings() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: Files\nrepositories:\n  - id: repo\n    path: .\ncontent:\n  - id: changelog\n    owner: project\n    repository: repo\n    path: CHANGELOG.md\n    mount: project\n    format: gfm\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let missing = observer.capture();
        let page = root.path().join("CHANGELOG.md");
        let image = root.path().join("plot.svg");
        fs::write(&page, "# Changelog\n\n![Plot](plot.svg)\n").unwrap();
        let created = observer.capture();
        assert_ne!(created, missing);
        fs::write(root.path().join("IGNORED.md"), "# Unselected\n").unwrap();
        assert_eq!(observer.capture(), created);
        fs::write(&image, "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        let with_image = observer.capture();
        assert_ne!(with_image, created);
        fs::write(
            &image,
            "<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>",
        )
        .unwrap();
        assert_ne!(observer.capture(), with_image);
        fs::write(&page, "# Updated changelog\n\n![Plot](plot.svg)\n").unwrap();
        assert_ne!(observer.capture(), with_image);
        fs::remove_file(page).unwrap();
        assert_eq!(observer.capture(), missing);
    }

    #[test]
    fn generated_storage_does_not_trigger_rebuilds_but_source_edits_do() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\ncontent:\n  - id: \"guide\"\n    owner: \"project\"\n    repository: \"repo\"\n    path: \".\"\n    mount: \"guide\"\n    format: \"gfm\"\n").unwrap();
        let page = root.path().join("index.md");
        fs::write(&page, "# Guide\n").unwrap();
        let sources = assemble_workspace(&config).unwrap();
        let snapshot = crate::snapshots::Snapshot::from_sources(
            &sources,
            &resolve_workspace(&sources).unwrap(),
        )
        .unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let before = observer.capture();
        let output = root.path().join("export.md");
        snapshot.publish(&output).unwrap();
        let temporary = root.path().join(".diplodocus-snapshot-pending.md");
        fs::write(&temporary, b"\xffpartial").unwrap();
        assert_eq!(observer.capture(), before);
        fs::write(&temporary, b"\xffchanged").unwrap();
        snapshot.publish(&output).unwrap();
        assert_eq!(observer.capture(), before);
        fs::remove_file(output).unwrap();
        fs::remove_file(temporary).unwrap();
        assert_eq!(observer.capture(), before);
        fs::write(page, "# Updated guide\n").unwrap();
        assert_ne!(observer.capture(), before);
    }

    #[test]
    fn watches_logo_creation_edits_and_removal_outside_content_collections() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\npresentation:\n  logo:\n    repository: \"repo\"\n    path: \"logo.svg\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let missing = observer.capture();
        let logo = root.path().join("logo.svg");
        fs::write(
            &logo,
            "<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>",
        )
        .unwrap();
        let created = observer.capture();
        assert_ne!(created, missing);
        fs::write(
            &logo,
            "<svg xmlns='http://www.w3.org/2000/svg'><circle r='4'/></svg>",
        )
        .unwrap();
        assert_ne!(observer.capture(), created);
        fs::remove_file(logo).unwrap();
        assert_eq!(observer.capture(), missing);
    }

    #[test]
    fn watches_open_graph_image_creation_edits_and_removal() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\npresentation:\n  site-url: \"https://example.test/\"\n  open-graph:\n    repository: \"repo\"\n    path: \"og.png\"\n    alt: \"A preview\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let missing = observer.capture();
        let image = root.path().join("og.png");
        fs::write(&image, b"first image").unwrap();
        let created = observer.capture();
        assert_ne!(created, missing);
        fs::write(&image, b"updated image").unwrap();
        assert_ne!(observer.capture(), created);
        fs::remove_file(image).unwrap();
        assert_eq!(observer.capture(), missing);
    }

    #[test]
    fn watches_assets_referenced_only_from_api_documentation() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("python/pkg")).unwrap();
        fs::write(
            root.path().join("pyproject.toml"),
            "[project]\nname='pkg'\nversion='1.0.0'\n",
        )
        .unwrap();
        fs::write(
            root.path().join("python/pkg/__init__.py"),
            "def documented():\n    \"\"\"`Download <payload.txt>`_.\"\"\"\n    pass\n",
        )
        .unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\npackages:\n  - id: \"pkg\"\n    name: \"Package\"\n    slug: \"package\"\n    ecosystem: \"python\"\n    repository: \"repo\"\n    path: \".\"\n    metadata-path: \"pyproject.toml\"\n    targets:\n      - id: \"api\"\n        extractor: \"python\"\n        path: \"python/pkg\"\n        role: \"public-api\"\n").unwrap();
        let asset = root.path().join("python/pkg/payload.txt");
        fs::write(&asset, "before").unwrap();
        let sources = assemble_workspace(&config).unwrap();
        assert_eq!(resolve_workspace(&sources).unwrap().assets().len(), 1);
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        observer.refresh_assets();
        let before = observer.capture();
        fs::write(&asset, "after").unwrap();
        assert_ne!(observer.capture(), before);
    }

    #[test]
    fn watches_julia_extension_entries_and_their_includes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("ext")).unwrap();
        fs::write(
            root.path().join("Project.toml"),
            "name = \"Demo\"\nuuid = \"12345678-1234-1234-1234-123456789abc\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("src/Demo.jl"),
            "module Demo\nexport plot\nfunction plot end\nend\n",
        )
        .unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: Observe\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Demo\n    slug: julia\n    ecosystem: julia\n    repository: repo\n    path: .\n    metadata-path: Project.toml\n    targets:\n      - id: api\n        extractor: julia\n        path: src/Demo.jl\n        role: public-api\n        julia:\n          extensions: [ext/DemoExt.jl]\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let missing = observer.capture();
        let extension = root.path().join("ext/DemoExt.jl");
        fs::write(
            &extension,
            "module DemoExt\nimport Demo: plot\ninclude(\"plot.jl\")\nend\n",
        )
        .unwrap();
        let created = observer.capture();
        assert_ne!(created, missing);
        let included = root.path().join("ext/plot.jl");
        fs::write(&included, "plot(x::Int) = x\n").unwrap();
        observer.refresh_assets();
        let before = observer.capture();
        fs::write(&included, "plot(x::Float64) = x\n").unwrap();
        assert_ne!(observer.capture(), before);
        let before = observer.capture();
        fs::write(
            &extension,
            "module DemoExt\nimport Demo: plot\nplot(x) = x\nend\n",
        )
        .unwrap();
        assert_ne!(observer.capture(), before);
        let before = observer.capture();
        fs::remove_file(extension).unwrap();
        assert_ne!(observer.capture(), before);
    }

    #[test]
    fn watches_package_readmes_and_explicit_overviews_in_every_ecosystem() {
        for ecosystem in ["python", "julia", "rust"] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("package")).unwrap();
            fs::write(root.path().join("package/manifest"), "metadata").unwrap();
            let config = root.path().join("diplodocus.yaml");
            fs::write(&config, format!("project:\n  name: Observe\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: pkg\n    name: Package\n    slug: package\n    ecosystem: {ecosystem}\n    repository: repo\n    path: package\n    metadata-path: manifest\n    overview: introduction.md\n    targets: []\n")).unwrap();
            let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
            let before = observer.capture();
            fs::write(root.path().join("package/README.md"), "# Readme\n").unwrap();
            assert_ne!(observer.capture(), before);
            let before = observer.capture();
            fs::write(
                root.path().join("package/introduction.md"),
                "# Introduction\n",
            )
            .unwrap();
            assert_ne!(observer.capture(), before);
            let before = observer.capture();
            fs::write(root.path().join("package/introduction.md"), "# Updated\n").unwrap();
            assert_ne!(observer.capture(), before);
            let before = observer.capture();
            fs::write(root.path().join("package/LICENSE"), "License terms\n").unwrap();
            assert_ne!(observer.capture(), before);
        }
    }

    #[test]
    fn watches_missing_and_custom_r_package_documents() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("r")).unwrap();
        fs::write(
            root.path().join("r/DESCRIPTION"),
            "Package: observed\nTitle: Observed package\nVersion: 1.0.0\nLicense: file COPYING\n",
        )
        .unwrap();
        fs::write(root.path().join("r/NAMESPACE"), "").unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\npackages:\n  - id: \"r\"\n    name: \"R package\"\n    slug: \"r\"\n    ecosystem: \"r\"\n    repository: \"repo\"\n    path: \"r\"\n    metadata-path: \"DESCRIPTION\"\n    targets:\n      - id: \"api\"\n        extractor: \"r\"\n        path: \".\"\n        role: \"public-api\"\n").unwrap();
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let before = observer.capture();
        fs::write(root.path().join("r/COPYING"), "First license text").unwrap();
        assert_ne!(observer.capture(), before);
        observer.refresh_assets();
        let before = observer.capture();
        fs::write(root.path().join("r/COPYING"), "Second license text").unwrap();
        assert_ne!(observer.capture(), before);
        let before = observer.capture();
        fs::write(root.path().join("r/README.md"), "# New overview\n").unwrap();
        assert_ne!(observer.capture(), before);
    }
}
