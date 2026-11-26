use super::*;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::provenance::fingerprint_bytes;
use crate::rendering::{ImageCache, RenderedFile, RenderedSite};
use crate::site::Site;

use super::preview_edits::{PreviewSources, SearchEntries};
use super::watching::{FileEvents, PreviewChange, Watcher, cancelled, clean_cancellation};
use crate::reporting::{Event, Reporting};

/// Run the production preview loop with explicit deadlines and shutdown signal.
/// A failed rebuild retains both the published tree and the served generation.
/// Shutdown waits for the active kernel's supervised cleanup before returning.
pub async fn serve_with(
    options: ServeOptions,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
) -> Result<(), CommandError> {
    CommandRunner::default()
        .serve_with(options, deadlines, cancellation)
        .await
}

pub(super) async fn serve_reported(
    options: ServeOptions<Option<PathBuf>>,
    deadlines: ExecutionDeadlines,
    cancellation: ExecutionCancellation<'_>,
    watch_sources: bool,
    reporting: &Reporting,
) -> Result<(), CommandError> {
    let (stop, receiver) = watch::channel(false);
    let worker = run(options, deadlines, receiver, watch_sources, reporting);
    tokio::pin!(worker);
    tokio::select! {
        biased;
        // Register signal handlers before the worker can announce readiness.
        _ = cancellation => {
            let _ = stop.send(true);
            worker.await
        }
        result = &mut worker => result,
    }
}

async fn run(
    options: ServeOptions<Option<PathBuf>>,
    deadlines: ExecutionDeadlines,
    stop: watch::Receiver<bool>,
    watch_sources: bool,
    reporting: &Reporting,
) -> Result<(), CommandError> {
    let setup = reporting.stage("Starting preview", None);
    let listener = TcpListener::bind((options.host, options.port)).await?;
    let address = listener.local_addr()?;
    let mut listener = Some(listener);
    let mut watcher = watch_sources
        .then(|| {
            Watcher::new(
                &options.config,
                options
                    .output
                    .as_deref()
                    .unwrap_or(Path::new(".diplodocus")),
                reporting,
            )
        })
        .transpose()?;
    setup.finish();
    let mut images = ImageCache::default();
    let mut server: Option<PreviewServer> = None;
    let mut events = None;

    loop {
        let command = reporting.command(if server.is_some() { "rebuild" } else { "build" });
        let mut attempt = pipeline::AttemptState {
            watch_sources,
            ..Default::default()
        };
        let snapshot = pipeline::prepare_snapshot(
            ExtractOptions {
                config: options.config.clone(),
                output: None,
            },
            options.output.as_deref(),
            deadlines,
            Box::pin(cancelled(stop.clone())),
            reporting,
            &mut attempt,
            false,
        )
        .await;
        let sources = attempt.preview.take();
        if let Some(watcher) = &mut watcher {
            watcher.acknowledge(attempt);
        }
        let owned;
        let rendered = match snapshot {
            Ok((snapshot_path, snapshot)) => {
                owned = snapshot;
                options
                    .output
                    .as_deref()
                    .map_or(Ok(()), |output| {
                        pipeline::reject_overlap(output, &snapshot_path)
                    })
                    .and_then(|()| {
                        render_full(&owned, options.output.as_deref(), &mut images, reporting)
                    })
            }
            Err(error) => Err(error),
        };
        let (site, rendered) = match rendered {
            Ok(success) => success,
            Err(error) => {
                drop(command);
                if *stop.borrow() && clean_cancellation(&error) {
                    if let Some(server) = &mut server {
                        server.stop().await;
                    }
                    return Ok(());
                }
                let Some(server) = &mut server else {
                    return Err(error);
                };
                reporting.error(&error);
                if *stop.borrow() {
                    server.stop().await;
                    return Err(error);
                }
                reporting.emit(Event::Watching {
                    retained_previous: true,
                });
                if server
                    .next_change(watcher.as_mut(), &mut events, None, &stop)
                    .await?
                    .is_none()
                {
                    return Ok(());
                }
                continue;
            }
        };
        if *stop.borrow() {
            if let Some(server) = &mut server {
                server.stop().await;
            }
            return Ok(());
        }
        let pages = rendered.page_count();
        let base = Arc::new(rendered);
        events = watcher
            .as_mut()
            .and_then(|watcher| watcher.file_events(sources.as_ref()));
        if let Some(server) = &mut server {
            server.advance(base.clone(), BTreeMap::new(), None);
            command.finish(options.output.as_deref(), Some(pages));
            reporting.emit(Event::Rebuilt);
        } else {
            command.finish(options.output.as_deref(), Some(pages));
            server = Some(PreviewServer::start(
                listener.take().expect("initial listener"),
                base.clone(),
                preview_url(address),
                options.live_reload && watch_sources,
                options.open,
                stop.clone(),
                reporting,
            )?);
        }
        if watch_sources {
            reporting.emit(Event::Watching {
                retained_previous: false,
            });
        }
        let server = server.as_mut().expect("preview started");
        let mut search = None;

        loop {
            let Some(change) = server
                .next_change(watcher.as_mut(), &mut events, sources.as_ref(), &stop)
                .await?
            else {
                return Ok(());
            };
            let PreviewChange::Page(path) = change else {
                break;
            };
            let watcher = watcher.as_mut().expect("watching page edits");
            let command = reporting.command("rebuild");
            let update = (|| -> Result<bool, CommandError> {
                let source = fs::read_to_string(&path)?;
                let Some((route, page)) = sources.as_ref().expect("known page").render(
                    &site,
                    &path,
                    source.clone(),
                    &base,
                    &images,
                )?
                else {
                    return Ok(false);
                };
                if *stop.borrow()
                    || !watcher.can_commit_page(
                        events.as_mut().expect("event watcher"),
                        &path,
                        source.as_bytes(),
                    )
                {
                    return Ok(false);
                }
                let mut changes = server.current.borrow().changes.clone();
                changes.insert(route.clone(), Arc::new(page.html));
                changes.insert(page.markdown.0, Arc::new(page.markdown.1));
                if let Some(entry) = page.search {
                    if search.is_none() {
                        search = Some(SearchEntries::load(&base)?);
                    }
                    search
                        .as_mut()
                        .expect("search entries")
                        .replace(entry, &mut changes)?;
                }
                if let Some(output) = &options.output {
                    base.publish_with(output, reporting, &changes)?;
                }
                watcher.acknowledge_page(path, fingerprint_bytes(source.as_bytes()).value);
                server.advance(base.clone(), changes, Some(format!("/{route}")));
                Ok(true)
            })();
            if matches!(update, Ok(true)) {
                command.finish(options.output.as_deref(), Some(pages));
                reporting.emit(Event::Rebuilt);
                reporting.emit(Event::Watching {
                    retained_previous: false,
                });
            } else {
                drop(command);
                if *stop.borrow() {
                    server.stop().await;
                    return Ok(());
                }
                watcher.reconcile();
                break;
            }
        }
    }
}

fn render_full<'a>(
    snapshot: &'a crate::snapshots::Snapshot,
    output: Option<&Path>,
    images: &mut ImageCache,
    reporting: &Reporting,
) -> Result<(Site<'a>, RenderedSite), CommandError> {
    use crate::reporting::Phase;
    let site = reporting
        .run_phase(Phase::SitePreparation, "Preparing site", || {
            Site::preview(snapshot)
        })?
        .with_components(true)
        .with_site_scripts(true);
    let rendering = reporting.phase(Phase::Rendering, None, None);
    let rendered = crate::rendering::render_with_image_cache(&site, true, reporting, Some(images))?;
    rendering.finish();
    if let Some(output) = output {
        let publication = reporting.phase(Phase::Publication, None, None);
        rendered.publish_reported(output, reporting)?;
        publication.finish();
    }
    Ok((site, rendered))
}

struct PreviewServer {
    current: watch::Sender<Arc<ServedGeneration>>,
    http: AbortOnDrop<std::io::Result<()>>,
    _browser: Option<AbortOnDrop<()>>,
}

impl PreviewServer {
    fn start(
        listener: TcpListener,
        site: Arc<RenderedSite>,
        url: String,
        live_reload: bool,
        open: bool,
        stop: watch::Receiver<bool>,
        reporting: &Reporting,
    ) -> Result<Self, CommandError> {
        let (current, generations) = watch::channel(Arc::new(ServedGeneration {
            site,
            changes: BTreeMap::new(),
            changed_route: None,
            revision: getrandom::u64().map_err(std::io::Error::other)?,
        }));
        let http = AbortOnDrop(tokio::spawn(http(listener, generations, live_reload, stop)));
        reporting.emit(Event::Serving { url: &url });
        let reporting = reporting.clone();
        let browser = open.then(|| {
            AbortOnDrop(tokio::spawn(async move {
                if let Err(error) = open_browser(&url).await {
                    reporting.emit(Event::Warning(&format!(
                        "could not open browser for {url}: {error}"
                    )));
                }
            }))
        });
        Ok(Self {
            current,
            http,
            _browser: browser,
        })
    }

    fn advance(
        &self,
        site: Arc<RenderedSite>,
        changes: BTreeMap<String, Arc<RenderedFile>>,
        changed_route: Option<String>,
    ) {
        let revision = self.current.borrow().revision.wrapping_add(1);
        self.current.send_replace(Arc::new(ServedGeneration {
            site,
            changes,
            revision,
            changed_route,
        }));
    }

    async fn stop(&mut self) {
        self.http.0.abort();
        let _ = (&mut self.http.0).await;
    }

    async fn next_change(
        &mut self,
        watcher: Option<&mut Watcher>,
        events: &mut Option<FileEvents>,
        sources: Option<&PreviewSources>,
        stop: &watch::Receiver<bool>,
    ) -> Result<Option<PreviewChange>, CommandError> {
        tokio::select! {
            _ = cancelled(stop.clone()) => {
                self.stop().await;
                Ok(None)
            },
            result = &mut self.http.0 => {
                result.map_err(std::io::Error::other)??;
                Ok(None)
            }
            change = async {
                match watcher {
                    Some(watcher) => watcher.next_preview_change(events, sources).await,
                    None => std::future::pending().await,
                }
            } => Ok(Some(change)),
        }
    }
}

fn preview_url(mut address: SocketAddr) -> String {
    // Wildcard addresses bind every interface but are not browser destinations.
    if address.ip().is_unspecified() {
        address.set_ip(match address {
            SocketAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
            SocketAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
        });
    }
    format!("http://{address}/")
}

async fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = tokio::process::Command::new("xdg-open");

    // Launchers must not consume terminal input or block the preview loop.
    // Leave the browser running when the preview shuts down.
    let status = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "browser launcher exited with {status}"
        )))
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);
struct ServedGeneration {
    site: Arc<RenderedSite>,
    changes: BTreeMap<String, Arc<RenderedFile>>,
    revision: u64,
    changed_route: Option<String>,
}

impl ServedGeneration {
    fn file(&self, path: &str) -> Option<&RenderedFile> {
        self.changes
            .get(path)
            .map(Arc::as_ref)
            .or_else(|| self.site.files().get(path))
    }
}
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn http(
    listener: TcpListener,
    generations: watch::Receiver<Arc<ServedGeneration>>,
    live_reload: bool,
    stop: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = cancelled(stop.clone()) => return Ok(()),
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                let generations = generations.clone();
                requests.spawn(async move { let _ = tokio::time::timeout(Duration::from_secs(5), response(socket, generations, live_reload)).await; });
            }
            _ = requests.join_next(), if !requests.is_empty() => {}
        }
    }
}
async fn response(
    mut socket: TcpStream,
    mut generations: watch::Receiver<Arc<ServedGeneration>>,
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
    let (request_path, query) = target.split_once('?').unwrap_or((target, ""));
    let since = query
        .strip_prefix("since=")
        .and_then(|value| value.parse::<u64>().ok());
    if live_reload
        && method == "GET"
        && request_path == "/__diplodocus/revision"
        && let Some(revision) = since
    {
        // Return as soon as a newer generation is published. Bound the wait so a
        // browser on an idle site periodically releases and renews its request.
        let _ = tokio::time::timeout(
            Duration::from_secs(4),
            generations.wait_for(|generation| generation.revision != revision),
        )
        .await;
    }
    let generation = generations.borrow().clone();
    let decoded = percent_encoding::percent_decode_str(request_path)
        .decode_utf8()
        .ok();
    if matches!(method, "GET" | "HEAD")
        && let Some(destination) = generation.site.redirects().get(request_path)
    {
        let location = if query.is_empty() {
            destination.clone()
        } else {
            format!("{destination}?{query}")
        };
        socket.write_all(format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").as_bytes()).await?;
        return socket.shutdown().await;
    }
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
    let path = path.map(|path| {
        let clean = format!("{path}.html");
        if !generation.site.files().contains_key(&path)
            && !path.ends_with(".html")
            && generation.site.files().contains_key(&clean)
        {
            clean
        } else {
            path
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
        .and_then(|p| generation.file(p));
    let negotiable = file.is_some_and(|file| file.media_type.starts_with("text/html"))
        && markdown_path
            .as_ref()
            .is_some_and(|p| generation.site.files().contains_key(p));
    let file = if negotiable && markdown {
        markdown_path.as_ref().and_then(|p| generation.file(p))
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
        if accept.trim() == "application/json" {
            let route = generation
                .changed_route
                .as_deref()
                .filter(|_| since == Some(generation.revision.wrapping_sub(1)));
            let update = serde_json::json!({
                "revision": generation.revision.to_string(),
                "route": route,
            });
            (
                "200 OK",
                "application/json",
                Cow::Owned(update.to_string().into_bytes()),
            )
        } else {
            (
                "200 OK",
                "text/plain; charset=utf-8",
                Cow::Owned(generation.revision.to_string().into_bytes()),
            )
        }
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
const LIVE_RELOAD: &str = r#"<script>
(() => {
  const served='__REVISION__';
  let revision = served;

  async function check() {
    while (true) {
      try {
        const response = await fetch('/__diplodocus/revision?since=' + revision, {
          cache: 'no-store', headers: { Accept: 'application/json' },
        });
        if (response.ok) {
          const update = await response.json();
          if (update.revision === revision) continue;
          const detail = { route: update.route, applied: null };
          if (detail.route) document.dispatchEvent(new CustomEvent('diplodocus:preview-update', { detail }));
          if (!(await detail.applied)) {
            location.reload();
            return;
          }
          revision = update.revision;
          document.dispatchEvent(new Event('diplodocus:preview-search'));
          continue;
        }
      } catch {}
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
  }

  check();
})();
</script>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::assemble_workspace;
    use std::fs;

    #[test]
    fn browser_urls_use_bound_ports_loopback_and_ipv6_brackets() {
        for (address, expected) in [
            ("127.0.0.1:12345", "http://127.0.0.1:12345/"),
            ("0.0.0.0:45678", "http://127.0.0.1:45678/"),
            ("[::]:12345", "http://[::1]:12345/"),
            ("[::1]:45678", "http://[::1]:45678/"),
            ("192.0.2.1:12345", "http://192.0.2.1:12345/"),
        ] {
            assert_eq!(preview_url(address.parse().unwrap()), expected);
        }
    }

    #[tokio::test]
    async fn http_negotiates_markdown_and_preserves_head_and_assets() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: \"Negotiation\"\nrepositories:\n  - id: \"repo\"\n    path: \".\"\ncontent:\n  - id: \"guide\"\n    owner: \"project\"\n    repository: \"repo\"\n    path: \"docs\"\n    mount: \"guide\"\n    format: \"gfm\"\n").unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("docs/index.md"),
            "# Guide\n\nAgent content.\n",
        )
        .unwrap();
        fs::write(
            root.path().join("docs/start.md"),
            "# Start\n\nAgent content.\n",
        )
        .unwrap();
        let sources = assemble_workspace(&config).unwrap();
        let snapshot = crate::snapshots::Snapshot::from_sources(
            &sources,
            &resolve_workspace(&sources).unwrap(),
        )
        .unwrap();
        let site = crate::site::Site::new(&snapshot).unwrap();
        let (_, generations) = watch::channel(Arc::new(ServedGeneration {
            site: Arc::new(crate::rendering::render_site(&site).unwrap()),
            changes: BTreeMap::new(),
            changed_route: None,
            revision: 7,
        }));
        for (method, target, accept, media) in [
            ("GET", "/guide/", "text/markdown", "text/markdown"),
            (
                "GET",
                "/guide/index.html?query=1",
                "TEXT/MARKDOWN",
                "text/markdown",
            ),
            ("HEAD", "/guide/", "text/markdown", "text/markdown"),
            ("GET", "/guide/start", "text/markdown", "text/markdown"),
            ("HEAD", "/guide/start", "text/markdown", "text/markdown"),
            ("GET", "/guide/start", "text/html", "text/html"),
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
            let server = response(socket, generations.clone(), true);
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

    #[tokio::test]
    async fn http_redirects_use_local_canonical_urls_and_preserve_queries() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("diplodocus.yaml");
        fs::write(&config, "project:\n  name: Redirects\npresentation:\n  site-url: https://example.test/docs/\n  canonical-url-style: clean\n  redirects:\n    /old: guide/start.html\n    /old.html: guide/start.html\n    /café%20old/: guide/start.html\n    /old%2dpath: guide/start.html\n    /old&path: guide/start.html\nrepositories:\n  - id: repo\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: repo\n    path: docs\n    mount: guide\n    format: gfm\n").unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/start.md"), "# Start\n").unwrap();
        let sources = assemble_workspace(config).unwrap();
        let snapshot = crate::snapshots::Snapshot::from_sources(
            &sources,
            &resolve_workspace(&sources).unwrap(),
        )
        .unwrap();
        let (_, generations) = watch::channel(Arc::new(ServedGeneration {
            site: crate::rendering::render_site(&crate::site::Site::new(&snapshot).unwrap())
                .unwrap()
                .into(),
            changes: BTreeMap::new(),
            changed_route: None,
            revision: 1,
        }));
        for (method, target, expected_status, destination) in [
            ("GET", "/old", "301", Some("/guide/start")),
            ("HEAD", "/old.html", "301", Some("/guide/start")),
            (
                "GET",
                "/old?from=old&q=a%20b",
                "301",
                Some("/guide/start?from=old&q=a%20b"),
            ),
            ("GET", "/caf%C3%A9%20old/", "301", Some("/guide/start")),
            ("GET", "/old%2dpath", "301", Some("/guide/start")),
            ("GET", "/old&path", "301", Some("/guide/start")),
            ("GET", "/%6Fld", "404", None),
            ("GET", "/old-path", "404", None),
            ("GET", "/old/", "404", None),
            ("GET", "/guide/start", "200", None),
            ("GET", "/missing", "404", None),
            ("POST", "/old", "405", None),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (socket, _) = listener.accept().await.unwrap();
            let server = response(socket, generations.clone(), true);
            let client = async {
                client.write_all(format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\nAccept: text/markdown\r\n\r\n").as_bytes()).await.unwrap();
                let mut bytes = Vec::new();
                client.read_to_end(&mut bytes).await.unwrap();
                String::from_utf8(bytes).unwrap()
            };
            let (result, response) = tokio::join!(server, client);
            result.unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(
                headers.starts_with(&format!("HTTP/1.1 {expected_status}")),
                "{method} {target}: {response}"
            );
            if let Some(destination) = destination {
                assert!(
                    headers.contains(&format!("\r\nLocation: {destination}\r\n")),
                    "{headers}"
                );
                assert!(body.is_empty());
            } else {
                assert!(!headers.contains("Location:"));
            }
        }
    }
}
