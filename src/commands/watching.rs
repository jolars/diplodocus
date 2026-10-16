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
    let (stop, receiver) = watch::channel(false);
    let worker = extract_loop(options, deadlines, receiver, on_snapshot);
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
        let result = pipeline::extract_attempt(
            options.clone(),
            None,
            deadlines,
            Box::pin(cancelled(stop.clone())),
        )
        .await;
        match result {
            Ok(path) => on_snapshot(&pipeline::absolute(&path)?)?,
            Err(error) if *stop.borrow() && clean_cancellation(&error) => return Ok(()),
            Err(error) if initial || *stop.borrow() => return Err(error),
            Err(error) => report_rebuild_error(&error),
        }
        initial = false;
    }
}

pub(super) fn report_rebuild_error(error: &CommandError) {
    eprintln!("error: {error}");
    for diagnostic in error
        .diagnostics()
        .iter()
        .chain(error.cleanup_diagnostics())
    {
        eprintln!("{}", format_diagnostic(diagnostic));
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

    pub(super) async fn next_change(&mut self) {
        loop {
            self.interval.tick().await;
            let observed = self.observer.capture();
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
        for package in &config.packages {
            let Some(root) = roots.get(package.repository.as_str()) else {
                continue;
            };
            let root = root.join(&package.path);
            observe(&root.join(&package.metadata_path), &mut observed);
            if package.ecosystem == "r" {
                observe(&root.join("NAMESPACE"), &mut observed);
                let mut vignettes = Vec::new();
                discover(
                    &root.join("vignettes"),
                    &self.output,
                    &mut observed,
                    &mut vignettes,
                    &|_| true,
                );
                for name in [
                    "README.md",
                    "NEWS.md",
                    "NEWS.Rd",
                    "NEWS",
                    "inst/CITATION",
                    "LICENSE",
                    "LICENSE.md",
                    "LICENCE",
                    "LICENCE.md",
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
                let mut selected = Vec::new();
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
            let extension = match content.format {
                crate::documents::AuthoredFormat::Gfm => "md",
                crate::documents::AuthoredFormat::Qmd => "qmd",
                crate::documents::AuthoredFormat::Rmd => "rmd",
            };
            let mut pages = Vec::new();
            discover(
                &root.join(&content.path),
                &self.output,
                &mut observed,
                &mut pages,
                &|p| {
                    content.format == crate::documents::AuthoredFormat::Rmd
                        || p.extension().is_some_and(|e| e == extension)
                },
            );
            for page in pages {
                if content.format == crate::documents::AuthoredFormat::Rmd
                    && !page.extension().is_some_and(|e| e == "Rmd" || e == "rmd")
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
        let workspace = sources.workspace();
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
            self.document_assets(
                &root.path.join(source.path.as_str()),
                &document.document,
                &root.path,
                &mut assets,
            );
        }
        self.assets = assets.into_keys().collect();
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

    #[test]
    fn generated_storage_does_not_trigger_rebuilds_but_source_edits_do() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Observe'\n[[repository]]\nid='repo'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='repo'\npath='.'\nmount='guide'\nformat='gfm'\n").unwrap();
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
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Observe'\n[presentation.logo]\nrepository='repo'\npath='logo.svg'\n[[repository]]\nid='repo'\npath='.'\n").unwrap();
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
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Observe'\n[presentation]\nsite-url='https://example.test/'\n[presentation.open-graph]\nrepository='repo'\npath='og.png'\nalt='A preview'\n[[repository]]\nid='repo'\npath='.'\n").unwrap();
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
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Observe'\n[[repository]]\nid='repo'\npath='.'\n[[package]]\nid='pkg'\nname='Package'\nslug='package'\necosystem='python'\nrepository='repo'\npath='.'\nmetadata-path='pyproject.toml'\ntargets=[{id='api', extractor='python', path='python/pkg', role='public-api'}]\n").unwrap();
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
    fn watches_missing_and_custom_r_package_documents() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("r")).unwrap();
        fs::write(
            root.path().join("r/DESCRIPTION"),
            "Package: observed\nTitle: Observed package\nVersion: 1.0.0\nLicense: file COPYING\n",
        )
        .unwrap();
        fs::write(root.path().join("r/NAMESPACE"), "").unwrap();
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Observe'\n[[repository]]\nid='repo'\npath='.'\n[[package]]\nid='r'\nname='R package'\nslug='r'\necosystem='r'\nrepository='repo'\npath='r'\nmetadata-path='DESCRIPTION'\ntargets=[{id='api', extractor='r', path='.', role='public-api'}]\n").unwrap();
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
