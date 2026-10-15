use super::*;
use std::borrow::Cow;
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
    let current = Arc::new(RwLock::new(Arc::new(ServedGeneration {
        site: initial,
        revision: 0,
    })));
    let http = tokio::spawn(http(
        listener,
        current.clone(),
        options.live_reload,
        stop.clone(),
    ));
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
                        let mut current = current.write().expect("preview generation lock");
                        *current = Arc::new(ServedGeneration {
                            site,
                            revision: current.revision + 1,
                        });
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
struct ServedGeneration {
    site: RenderedSite,
    revision: u64,
}
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
    site: Arc<RwLock<Arc<ServedGeneration>>>,
    live_reload: bool,
    stop: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = cancelled(stop.clone()) => return Ok(()),
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                let site = site.read().expect("preview generation lock").clone();
                requests.spawn(async move { let _ = tokio::time::timeout(Duration::from_secs(5), response(socket, &site, live_reload)).await; });
            }
            _ = requests.join_next(), if !requests.is_empty() => {}
        }
    }
}
async fn response(
    mut socket: TcpStream,
    generation: &ServedGeneration,
    live_reload: bool,
) -> std::io::Result<()> {
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
    let accept = std::str::from_utf8(&bytes)
        .unwrap_or("")
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("accept"))
        .map(|(_, value)| value)
        .collect::<Vec<_>>()
        .join(",");
    let markdown = accepts_markdown(&accept);
    let markdown_path = path
        .as_deref()
        .and_then(|p| p.strip_suffix(".html"))
        .map(|p| format!("{p}.md"));
    let file = path
        .as_ref()
        .filter(|p| crate::diagnostics::DiagnosticPath::try_from(p.as_str()).is_ok())
        .and_then(|p| generation.site.files().get(p));
    let negotiable = file.is_some_and(|file| file.media_type.starts_with("text/html"))
        && markdown_path
            .as_ref()
            .is_some_and(|p| generation.site.files().contains_key(p));
    let file = if negotiable && markdown {
        markdown_path
            .as_ref()
            .and_then(|p| generation.site.files().get(p))
    } else {
        file
    };
    let vary = if negotiable { "Vary: Accept\r\n" } else { "" };
    let (status, media, content) = if !matches!(method, "GET" | "HEAD") {
        (
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            Cow::Borrowed(b"Method not allowed\n".as_slice()),
        )
    } else if live_reload && path.as_deref() == Some("__diplodocus/revision") {
        (
            "200 OK",
            "text/plain; charset=utf-8",
            Cow::Owned(generation.revision.to_string().into_bytes()),
        )
    } else if let Some(file) = file {
        let content = if live_reload && file.media_type.starts_with("text/html") {
            Cow::Owned(with_live_reload(&file.bytes, generation.revision))
        } else {
            Cow::Borrowed(file.bytes.as_slice())
        };
        ("200 OK", file.media_type.as_str(), content)
    } else {
        (
            "404 Not Found",
            "text/plain; charset=utf-8",
            Cow::Borrowed(b"Not found\n".as_slice()),
        )
    };
    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {media}\r\n{vary}Content-Length: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", content.len()).as_bytes()).await?;
    if method != "HEAD" {
        socket.write_all(&content).await?;
    }
    socket.shutdown().await
}
fn accepts_markdown(accept: &str) -> bool {
    let mut markdown = 0.0_f32;
    let mut html = 0.0_f32;
    for range in accept.split(',') {
        let mut parts = range.trim().split(';');
        let media = parts.next().unwrap_or("").trim();
        let quality = parts
            .filter_map(|part| part.trim().split_once('='))
            .find(|(key, _)| key.eq_ignore_ascii_case("q"))
            .map_or(1.0, |(_, value)| value.parse::<f32>().unwrap_or(0.0));
        if !(0.0..=1.0).contains(&quality) {
            continue;
        }
        if media.eq_ignore_ascii_case("text/markdown") {
            markdown = markdown.max(quality);
        }
        if media.eq_ignore_ascii_case("text/html") {
            html = html.max(quality);
        }
    }
    markdown > 0.0 && markdown >= html
}

fn with_live_reload(html: &[u8], revision: u64) -> Vec<u8> {
    let marker = b"</body>";
    let Some(position) = html
        .windows(marker.len())
        .rposition(|window| window == marker)
    else {
        return html.to_vec();
    };
    let script = LIVE_RELOAD.replace("__REVISION__", &revision.to_string());
    let mut content = Vec::with_capacity(html.len() + script.len());
    content.extend_from_slice(&html[..position]);
    content.extend_from_slice(script.as_bytes());
    content.extend_from_slice(&html[position..]);
    content
}
const LIVE_RELOAD: &str = r#"<script>(()=>{const served=__REVISION__;async function check(){try{const response=await fetch('/__diplodocus/revision',{cache:'no-store'});if(response.ok&&Number(await response.text())!==served){location.reload();return}}catch{}setTimeout(check,1000)}setTimeout(check,1000)})()</script>"#;

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

    #[tokio::test]
    async fn http_negotiates_markdown_and_preserves_head_and_assets() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.toml");
        fs::write(&config, "[project]\nname='Negotiation'\n[[repository]]\nid='repo'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='repo'\npath='docs'\nmount='guide'\nformat='gfm'\n").unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("docs/index.md"),
            "# Guide\n\nAgent content.\n",
        )
        .unwrap();
        let sources = assemble_workspace(&config).unwrap();
        let snapshot = crate::snapshots::Snapshot::from_sources(
            &sources,
            &resolve_workspace(&sources).unwrap(),
        )
        .unwrap();
        let site = crate::site::Site::new(&snapshot).unwrap();
        let generation = ServedGeneration {
            site: crate::rendering::render_site(&site).unwrap(),
            revision: 7,
        };
        for (method, target, accept, media) in [
            ("GET", "/guide/", "text/markdown", "text/markdown"),
            (
                "GET",
                "/guide/index.html?query=1",
                "TEXT/MARKDOWN",
                "text/markdown",
            ),
            ("HEAD", "/guide/", "text/markdown", "text/markdown"),
            ("GET", "/guide/", "*/*", "text/html"),
            ("GET", "/guide/", "text/markdown;q=0", "text/html"),
            (
                "GET",
                "/assets/search.json",
                "text/markdown",
                "application/json",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (socket, _) = listener.accept().await.unwrap();
            let request = format!(
                "{method} {target} HTTP/1.1\r\nHost: localhost\r\nAccept: {accept}\r\n\r\n"
            );
            let server = response(socket, &generation, true);
            let client = async {
                client.write_all(request.as_bytes()).await.unwrap();
                let mut bytes = Vec::new();
                client.read_to_end(&mut bytes).await.unwrap();
                String::from_utf8(bytes).unwrap()
            };
            let (result, response) = tokio::join!(server, client);
            result.unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(headers.contains("200 OK"), "{response}");
            assert!(
                headers.contains(&format!("Content-Type: {media}")),
                "{headers}"
            );
            if target.starts_with("/guide/") {
                assert!(headers.contains("Vary: Accept"));
            }
            if method == "HEAD" {
                assert!(body.is_empty());
            } else if media == "text/markdown" {
                assert!(body.contains("Agent content."));
                assert!(!body.contains("<script>"));
            }
        }
    }

    #[test]
    fn markdown_accept_quality_and_browser_defaults() {
        for accept in [
            "text/markdown",
            "TEXT/MARKDOWN; charset=utf-8",
            "text/html, text/markdown",
            "text/markdown;q=0.8, */*;q=0.1",
        ] {
            assert!(accepts_markdown(accept), "{accept}");
        }
        for accept in [
            "",
            "*/*",
            "text/html",
            "text/markdown;q=0",
            "text/markdown;q=0.2,text/html;q=0.9",
            "text/markdown;q=invalid",
            "text/markdown;q=NaN",
        ] {
            assert!(!accepts_markdown(accept), "{accept}");
        }
    }

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
