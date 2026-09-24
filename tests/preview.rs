#![cfg(target_os = "linux")]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use diplodocus::commands::{ServeOptions, serve_with};
use diplodocus::execution::ExecutionDeadlines;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(port: u16, path: &str) -> Option<String> {
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .ok()?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
        .await
        .ok()?
        .ok()?;
    String::from_utf8(bytes).ok()
}
async fn until(mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if condition().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("preview condition did not become true");
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    support::files_under(root)
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(root.join(&p)).unwrap();
            (p, bytes)
        })
        .collect()
}
fn code(label: &str, hanging: bool) -> String {
    let body = if hanging {
        "import time\ntime.sleep(120)".into()
    } else {
        format!(
            "print('{label}')\nfrom IPython.display import SVG, display\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\"))"
        )
    };
    format!(
        "# Preview\n\n```{{python}}\nimport os, json\nfrom pathlib import Path\nfrom ipykernel.connect import get_connection_file\nPath('../process.json').write_text(json.dumps({{'pid': os.getpid(), 'connection': get_connection_file()}}))\n{body}\n```\n\n```{{python}}\nPath('../next-cell').write_text('ran')\n```\n"
    )
}
fn process(root: &Path) -> Option<(u32, PathBuf)> {
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("process.json")).ok()?).ok()?;
    Some((
        value["pid"].as_u64()? as u32,
        value["connection"].as_str()?.into(),
    ))
}
fn gone(process: &(u32, PathBuf)) -> bool {
    !PathBuf::from(format!("/proc/{}", process.0)).exists() && !process.1.parent().unwrap().exists()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watched_timeout_keeps_serving_the_complete_site_and_recovers() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Preview'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\ndeclared_environment_inputs=['environment.txt']\n");
    root.write("environment.txt", "first environment");
    root.write("guide/index.qmd", code("first generation", false));
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let options = ServeOptions {
        config: root.path().join("diplodocus.toml"),
        output: root.path().join("site"),
        host: std::net::Ipv4Addr::LOCALHOST.into(),
        port,
    };
    let deadlines = ExecutionDeadlines {
        cell: 1500,
        terminal_sync: 500,
        interrupt: 1000,
        shutdown: 1000,
        termination: 1000,
        forced_exit: 1000,
        ..ExecutionDeadlines::default()
    };
    let preview = tokio::spawn(serve_with(
        options,
        deadlines,
        Box::pin(async {
            let _ = stopped.await;
        }),
    ));
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("first generation"))
    })
    .await;
    let first_process = process(root.path()).unwrap();
    assert!(gone(&first_process));
    let previous = files(&root.path().join("site"));
    let snapshot = std::fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    root.remove("next-cell");
    root.write("guide/index.qmd", code("timeout generation", true));
    until(async || process(root.path()).is_some_and(|p| p.0 != first_process.0)).await;
    let failed_process = process(root.path()).unwrap();
    let served = request(port, "/").await.unwrap();
    assert!(served.starts_with("HTTP/1.1 200") && served.contains("first generation"));
    until(async || gone(&failed_process)).await;
    assert!(!root.path().join("next-cell").exists());
    assert_eq!(files(&root.path().join("site")), previous);
    assert_eq!(
        std::fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap(),
        snapshot
    );
    assert!(
        request(port, "/")
            .await
            .unwrap()
            .contains("first generation")
    );
    root.write("guide/index.qmd", code("second generation", false));
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("second generation"))
    })
    .await;
    let recovered_process = process(root.path()).unwrap();
    assert!(gone(&recovered_process));
    assert!(root.path().join("next-cell").exists());
    root.write("environment.txt", "second environment");
    until(async || process(root.path()).is_some_and(|p| p.0 != recovered_process.0)).await;
    let recovered_process = process(root.path()).unwrap();
    until(async || gone(&recovered_process)).await;
    let svg = previous
        .keys()
        .find(|p| p.extension().is_some_and(|e| e == "svg"))
        .unwrap();
    let asset_response = request(port, &format!("/{}", svg.display())).await.unwrap();
    assert!(asset_response.contains("Content-Type: image/svg+xml"));
    assert!(asset_response.ends_with(std::str::from_utf8(&previous[svg]).unwrap()));
    root.write("unrelated.txt", "ignore");
    root.write("site/ignored.qmd", "# Output is not input\n");
    root.write(".diplodocus/ignored.qmd", "# Storage is not input\n");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(process(root.path()).unwrap(), recovered_process);
    assert!(
        request(port, "/%2e%2e/diplodocus.toml")
            .await
            .unwrap()
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        request(port, "/diplodocus.toml")
            .await
            .unwrap()
            .starts_with("HTTP/1.1 404")
    );
    root.write("guide/index.qmd", code("cancel generation", true));
    until(async || process(root.path()).is_some_and(|p| p.0 != recovered_process.0)).await;
    let canceled_process = process(root.path()).unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(15), preview)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(gone(&canceled_process));
    assert!(
        std::fs::read_to_string(root.path().join("site/index.html"))
            .unwrap()
            .contains("second generation")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_tracks_missing_assets_and_configuration_changes() {
    let root = support::TestWorkspace::new();
    let config = "[project]\nname='Static preview'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='gfm'\n";
    root.write("diplodocus.toml", config);
    root.write("guide/index.md", "# Original\n");
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(serve_with(
        ServeOptions {
            config: root.path().join("diplodocus.toml"),
            output: root.path().join("site"),
            host: std::net::Ipv4Addr::LOCALHOST.into(),
            port,
        },
        ExecutionDeadlines::default(),
        Box::pin(async {
            let _ = stopped.await;
        }),
    ));
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("Original"))
    })
    .await;
    let previous = files(&root.path().join("site"));
    root.write("guide/index.md", "# Added asset\n\n![New](new.svg)\n");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(request(port, "/").await.unwrap().contains("Original"));
    assert_eq!(files(&root.path().join("site")), previous);
    root.write(
        "guide/new.svg",
        "<svg xmlns='http://www.w3.org/2000/svg'><circle r='2'/></svg>",
    );
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("Added asset"))
    })
    .await;
    root.write(
        "guide/new.svg",
        "<svg xmlns='http://www.w3.org/2000/svg'><circle r='5'/></svg>",
    );
    let digest =
        diplodocus::provenance::fingerprint_bytes(root.read("guide/new.svg").as_bytes()).value;
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|body| body.contains(&digest))
    })
    .await;
    root.write("other/index.md", "# Configured collection\n");
    root.write(
        "diplodocus.toml",
        config.replace("path='guide'", "path='other'"),
    );
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("Configured collection"))
    })
    .await;
    root.remove("other/index.md");
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("Static preview") && !s.contains("Configured collection"))
    })
    .await;
    root.write("other/index.md", "# Restored input\n");
    until(async || {
        request(port, "/")
            .await
            .is_some_and(|s| s.contains("Restored input"))
    })
    .await;
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
