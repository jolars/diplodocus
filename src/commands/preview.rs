use super::*;
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::execution::{ExecutionCancellation, ExecutionDeadlines};
use crate::rendering::RenderedSite;

use super::watching::{Watcher, cancelled, clean_cancellation, report_rebuild_error};

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
    let mut watcher = Watcher::new(&options.config, &options.output)?;
    let initial =
        pipeline::build_attempt(&build, deadlines, Box::pin(cancelled(stop.clone()))).await;
    let initial = match initial {
        Ok(site) => site,
        Err(error) if *stop.borrow() && clean_cancellation(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let current = Arc::new(RwLock::new(Arc::new(ServedGeneration {
        site: initial,
        // Open pages must also detect a new server after the generator recompiles.
        revision: getrandom::u64().map_err(std::io::Error::other)?,
    })));
    let http = tokio::spawn(http(
        listener,
        current.clone(),
        options.live_reload,
        stop.clone(),
    ));
    let mut http = AbortOnDrop(http);
    eprintln!("Serving http://{address}");
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
            _ = watcher.next_change() => {
                let result = pipeline::build_attempt(&build, deadlines, Box::pin(cancelled(stop.clone()))).await;
                match result {
                    Ok(site) => {
                        let mut current = current.write().expect("preview generation lock");
                        *current = Arc::new(ServedGeneration {
                            site,
                            revision: current.revision.wrapping_add(1),
                        });
                        eprintln!("Rebuilt documentation.");
                    }
                    Err(error) => {
                        if *stop.borrow() && clean_cancellation(&error) { break Ok(()); }
                        report_rebuild_error(&error);
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
const LIVE_RELOAD: &str = r#"<script>(()=>{const served='__REVISION__';async function check(){try{const response=await fetch('/__diplodocus/revision',{cache:'no-store'});if(response.ok&&await response.text()!==served){location.reload();return}}catch{}setTimeout(check,1000)}setTimeout(check,1000)})()</script>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
