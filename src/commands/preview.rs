use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::configuration::{ContentConfiguration, WorkspaceConfiguration};
use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::provenance::fingerprint_bytes;
use crate::rendering::RenderedSite;

/// Run the production preview loop with explicit deadlines and shutdown signal.
/// A failed rebuild retains both the published tree and the served generation.
/// Shutdown waits for the active kernel's supervised cleanup before returning.
pub async fn serve_with(
    options: ServeOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<(), CommandError> {
    let (stop, receiver) = watch::channel(false);
    let worker = run(options, deadlines, receiver);
    tokio::pin!(worker);
    tokio::select! {
        result = &mut worker => result,
        _ = cancellation => {
            let _ = stop.send(true);
            worker.await
        }
    }
}
async fn cancelled(mut receiver: watch::Receiver<bool>) {
    if !*receiver.borrow() {
        let _ = receiver.wait_for(|value| *value).await;
    }
}
async fn run(
    options: ServeOptions,
    deadlines: ExecutionDeadlines,
    stop: watch::Receiver<bool>,
) -> Result<(), CommandError> {
    let listener = TcpListener::bind((options.host, options.port)).await?;
    let address = listener.local_addr()?;
    let build = BuildOptions {
        config: options.config.clone(),
        output: options.output.clone(),
    };
    let mut observer = Observer::new(&options.config, &options.output)?;
    let mut attempted = observer.capture();
    let initial =
        pipeline::build_attempt(&build, deadlines, Box::pin(cancelled(stop.clone()))).await;
    let initial = match initial {
        Ok(site) => site,
        Err(error) if *stop.borrow() && clean_cancellation(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let current = Arc::new(RwLock::new(Arc::new(initial)));
    let http = tokio::spawn(http(listener, current.clone(), stop.clone()));
    let mut http = AbortOnDrop(http);
    eprintln!("Serving http://{address}");
    let mut pending = attempted.clone();
    let mut changed = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut http_finished = false;
    let result = loop {
        tokio::select! {
            _ = cancelled(stop.clone()) => break Ok(()),
            result = &mut http.0 => {
                http_finished = true;
                break match result {
                    Ok(result) => result.map_err(CommandError::from),
                    Err(error) => Err(std::io::Error::other(error).into()),
                };
            }
            _ = interval.tick() => {
                let observed = observer.capture();
                if observed != pending { pending = observed; changed = Instant::now(); }
                if pending == attempted || changed.elapsed() < Duration::from_millis(200) { continue; }
                observer.refresh_assets();
                attempted = observer.capture();
                pending = attempted.clone();
                let result = pipeline::build_attempt(&build, deadlines, Box::pin(cancelled(stop.clone()))).await;
                match result {
                    Ok(site) => {
                        *current.write().expect("preview generation lock") = Arc::new(site);
                        eprintln!("Rebuilt documentation.");
                    }
                    Err(error) => {
                        if *stop.borrow() && clean_cancellation(&error) { break Ok(()); }
                        eprintln!("error: {error}");
                        for diagnostic in error.diagnostics().iter().chain(error.cleanup_diagnostics()) { eprintln!("{}", format_diagnostic(diagnostic)); }
                        if *stop.borrow() { break Err(error); }
                    }
                }
            }
        }
    };
    // The task owns only HTTP connections. Kernel supervision has already completed.
    if !http_finished {
        http.0.abort();
        let _ = (&mut http.0).await;
    }
    result
}
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn clean_cancellation(error: &CommandError) -> bool {
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

async fn http(
    listener: TcpListener,
    site: Arc<RwLock<Arc<RenderedSite>>>,
    stop: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = cancelled(stop.clone()) => return Ok(()),
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                let site = site.read().expect("preview generation lock").clone();
                requests.spawn(async move { let _ = tokio::time::timeout(Duration::from_secs(5), response(socket, &site)).await; });
            }
            _ = requests.join_next(), if !requests.is_empty() => {}
        }
    }
}
async fn response(mut socket: TcpStream, site: &RenderedSite) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 2048];
    while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buffer).await?;
        if n == 0 || bytes.len() + n > 16384 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let line = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.lines().next())
        .unwrap_or("");
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let decoded = percent_encoding::percent_decode_str(target.split('?').next().unwrap_or(""))
        .decode_utf8()
        .ok();
    let path = decoded
        .as_deref()
        .and_then(|p| p.strip_prefix('/'))
        .map(|p| {
            if p.is_empty() || p.ends_with('/') {
                format!("{p}index.html")
            } else {
                p.into()
            }
        });
    let file = path
        .as_ref()
        .filter(|p| crate::diagnostics::DiagnosticPath::try_from(p.as_str()).is_ok())
        .and_then(|p| site.files().get(p));
    let (status, media, content) = if !matches!(method, "GET" | "HEAD") {
        (
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Method not allowed\n".as_slice(),
        )
    } else if let Some(file) = file {
        ("200 OK", file.media_type.as_str(), file.bytes.as_slice())
    } else {
        (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not found\n".as_slice(),
        )
    };
    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {media}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", content.len()).as_bytes()).await?;
    if method != "HEAD" {
        socket.write_all(content).await?;
    }
    socket.shutdown().await
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
        for package in &config.packages {
            let Some(root) = roots.get(package.repository.as_str()) else {
                continue;
            };
            let root = root.join(&package.path);
            observe(&root.join(&package.metadata_path), &mut observed);
            if package.ecosystem == "r" {
                observe(&root.join("NAMESPACE"), &mut observed);
            }
            for target in &package.targets {
                let mut selected = Vec::new();
                discover(
                    &root.join(&target.path),
                    &self.output,
                    &mut observed,
                    &mut selected,
                    &|path| {
                        matches!(
                            path.extension().and_then(|s| s.to_str()),
                            Some("py" | "pyi" | "R" | "r" | "Rd")
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
            };
            let mut pages = Vec::new();
            discover(
                &root.join(&content.path),
                &self.output,
                &mut observed,
                &mut pages,
                &|p| p.extension().is_some_and(|e| e == extension),
            );
            for page in pages {
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
fn discover(
    root: &Path,
    output: &Path,
    observed: &mut Observation,
    selected: &mut Vec<PathBuf>,
    accept: &impl Fn(&Path) -> bool,
) {
    if root.starts_with(output)
        || matches!(
            root.file_name().and_then(|s| s.to_str()),
            Some(".git" | ".diplodocus")
        )
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
        observe(root, observed);
        selected.push(root.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fs::write(&config, "[project]\nname='Observe'\n[[repository]]\nid='repo'\npath='.'\n[[package]]\nid='pkg'\nname='Package'\nslug='package'\necosystem='python'\nrepository='repo'\npath='.'\nmetadata_path='pyproject.toml'\ntargets=[{id='api', extractor='python', path='python/pkg', role='public-api'}]\n").unwrap();
        let asset = root.path().join("python/pkg/payload.txt");
        fs::write(&asset, "before").unwrap();
        let sources = assemble_workspace(&config).unwrap();
        assert_eq!(resolve_workspace(&sources).unwrap().assets().len(), 1);
        let mut observer = Observer::new(&config, &root.path().join("site")).unwrap();
        let before = observer.capture();
        fs::write(&asset, "after").unwrap();
        assert_ne!(observer.capture(), before);
    }
}
