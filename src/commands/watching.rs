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
use crate::reporting::{Event, Reporting};

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
    let mut watcher = Watcher::new(&options.config, &output)?;
    let mut initial = true;
    loop {
        if !initial {
            tokio::select! {
                _ = cancelled(stop.clone()) => return Ok(()),
                _ = watcher.next_change() => {},
            }
        }
        let command = reporting.command("extract");
        let mut attempt = pipeline::AttemptState::default();
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
    observer: Observer,
    attempted: Observation,
    pending: Observation,
    changed: Instant,
    interval: tokio::time::Interval,
}

impl Watcher {
    pub(super) fn new(config: &Path, output: &Path) -> Result<Self, CommandError> {
        let mut observer = Observer::new(config, output)?;
        let attempted = observer.capture();
        Ok(Self {
            observer,
            pending: attempted.clone(),
            attempted,
            changed: Instant::now(),
            interval: tokio::time::interval(Duration::from_millis(100)),
        })
    }

    pub(super) fn acknowledge(&mut self, attempt: pipeline::AttemptState) {
        let previous_assets = self.observer.assets.clone();
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
        }
        let observed = self.observer.capture();
        for path in previous_assets.difference(&self.observer.assets) {
            if !observed.contains_key(path) {
                self.attempted.remove(path);
            }
        }
        self.attempted.retain(|path, value| {
            !(value == "NotFound"
                && path.is_dir()
                && attempt.writes.keys().any(|output| output.starts_with(path)))
        });
        for (path, value) in &observed {
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
    }

    pub(super) async fn next_change(&mut self) {
        loop {
            self.interval.tick().await;
            let observed = self.observer.capture();
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
            self.observer.refresh_assets();
            self.attempted = self.observer.capture();
            self.pending = self.attempted.clone();
            return;
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
struct Observer {
    config: PathBuf,
    output: PathBuf,
    configuration: Option<WorkspaceConfiguration>,
    assets: BTreeSet<PathBuf>,
    native_roots: Vec<PathBuf>,
}
impl Observer {
    fn new(config: &Path, output: &Path) -> Result<Self, CommandError> {
        let mut observer = Self {
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
        };
        observer.refresh_assets();
        Ok(observer)
    }
    fn capture(&mut self) -> Observation {
        let mut observed = BTreeMap::new();
        observe(&self.config, &mut observed);
        for asset in &self.assets {
            observe(asset, &mut observed);
        }
        for root in &self.native_roots {
            discover_doxygen(root, &self.output, &mut observed);
        }
        if let Ok(text) = fs::read_to_string(&self.config)
            && let Ok(config) = crate::configuration::parse_configuration(&text)
        {
            self.configuration = Some(config);
        }
        let Some(config) = &self.configuration else {
            return observed;
        };
        let parent = self.config.parent().expect("absolute config");
        let roots: BTreeMap<_, _> = config
            .repositories
            .iter()
            .filter_map(|r| {
                pipeline::absolute(&parent.join(&r.path))
                    .ok()
                    .map(|path| (r.id.as_str(), path))
            })
            .collect();
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
        observed
    }
    fn refresh_assets(&mut self) {
        let Ok(sources) = assemble_workspace(&self.config) else {
            return;
        };
        let inputs = SourceWatchInputs::capture(&sources, &self.output);
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
pub(super) struct SourceWatchInputs {
    assets: Observation,
    native_roots: Vec<PathBuf>,
}
impl SourceWatchInputs {
    pub(super) fn capture(sources: &crate::assembly::WorkspaceSources, output: &Path) -> Self {
        let observer = Observer {
            config: PathBuf::new(),
            output: output.to_owned(),
            configuration: None,
            assets: BTreeSet::new(),
            native_roots: Vec::new(),
        };
        let workspace = sources.workspace();
        let native_roots = sources.doxygen_watch_roots().cloned().collect();
        let mut assets = Observation::new();
        for path in sources.input_paths() {
            observe(&path, &mut assets);
        }
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
                &mut assets,
            );
        }
        Self {
            assets,
            native_roots,
        }
    }
}

fn observe(path: &Path, observed: &mut Observation) {
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

fn discover_doxygen(root: &Path, output: &Path, observed: &mut Observation) {
    if root.starts_with(output) {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(root) else {
        observe(root, observed);
        return;
    };
    if metadata.is_file() || metadata.is_symlink() {
        observe(root, observed);
    } else if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if !crate::extractors::doxygen::configuration::excluded(
                &entry.file_name().to_string_lossy(),
            ) {
                discover_doxygen(&entry.path(), output, observed);
            }
        }
    }
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
        };
        let attempted = observer.capture();
        Watcher {
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
            watch_inputs: None,
        }
    }

    fn subscribed_inputs(root: &Path, paths: &[&str]) -> SourceWatchInputs {
        let mut assets = BTreeMap::new();
        for path in paths {
            observe(&root.join(path), &mut assets);
        }
        SourceWatchInputs {
            assets,
            native_roots: Vec::new(),
        }
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
        attempt.watch_inputs = Some(subscribed_inputs(root.path(), &["man/generated.Rd"]));
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
        let mut watcher = Watcher::new(&config, &root.path().join("site")).unwrap();

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
            "def documented():\n    \"\"\"[Download](payload.txt).\"\"\"\n    pass\n",
        )
        .unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Observe\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\npackages:\n  - id: \"pkg\"\n    name: \"Package\"\n    slug: \"package\"\n    ecosystem: \"python\"\n    repository: \"repo\"\n    path: \".\"\n    metadata-path: \"pyproject.toml\"\n    targets:\n      - id: \"api\"\n        extractor: \"python\"\n        path: \"python/pkg\"\n        role: \"public-api\"\n").unwrap();
        let asset = root.path().join("python/pkg/payload.txt");
        fs::write(&asset, "before").unwrap();
        let sources = assemble_workspace(&config).unwrap();
        assert_eq!(resolve_workspace(&sources).unwrap().assets().len(), 1);
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
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
