mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, BuildOptions, GenerateOptions, ServeOptions};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path style="fill:#073984" d="M0 0h32v32H0z"/></svg>"##;

fn workspace(extra: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Public assets\npresentation:\n{extra}\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: pages\n    mount: ''\n    format: gfm\n"));
    root.write("pages/index.md", "# Home\n");
    root.write("pages/guide/index.md", "# Guide\n");
    root.write("pages/guide/start.md", "# Start\n");
    root.write("pages/myindex.md", "# Another index\n");
    root
}

fn eunoia() -> support::TestWorkspace {
    // These declarations replace Eunoia's static-file copy and four injected links.
    let root = workspace(
        "  site-url: https://example.test/docs/\n  canonical-url-style: clean\n  asset-directories: [{ repository: docs, path: catalog, publish: datasets }]\n  assets:\n    - { repository: docs, path: static/favicon.svg, publish: favicon.svg }\n    - { repository: docs, path: static/favicon.ico, publish: favicon.ico }\n    - { repository: docs, path: static/apple-touch-icon.png, publish: apple-touch-icon.png }\n    - { repository: docs, path: static/site.webmanifest, publish: site.webmanifest }\n    - { repository: docs, path: static/llms.txt, publish: llms.txt }\n    - { repository: docs, path: static/192.png, publish: icons/192.png }\n    - { repository: docs, path: static/site.css, publish: public/site.css }\n    - { repository: docs, path: static/site.js, publish: public/site.js }\n    - { repository: docs, path: static/site.mjs, publish: public/site.mjs }\n  stylesheets: [public/site.css]\n  scripts: [public/site.js, public/site.mjs]\n  apple-mobile-web-app-title: 'Eunoia & docs'\n  icons:\n    - { path: favicon.svg, type: image/svg+xml, sizes: any }\n    - { path: favicon.ico }\n    - { path: apple-touch-icon.png, rel: apple-touch-icon, sizes: 180x180 }\n    - { path: site.webmanifest, rel: manifest }\n",
    );
    root.write("catalog/nested/item.json", "{}");
    root.write("static/site.css", "body { color: green; }");
    root.write("static/site.js", "window.siteLoaded = true;");
    root.write("static/site.mjs", "export const loaded = true;");
    root.write("static/favicon.svg", SVG);
    root.write("static/favicon.ico", b"\0\0\x01\0icon bytes");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    root.write("static/apple-touch-icon.png", png.get_ref());
    root.write("static/192.png", png.get_ref());
    root.write("static/site.webmanifest", r#"{"name":"Eunoia","icons":[{"src":"icons/192.png?v=20260507","sizes":"192x192","type":"image/png"}]}"#);
    root.write("static/llms.txt", "# Eunoia\n\nPublic documentation.\n");
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn public_bytes(snapshot: &Snapshot) -> BTreeMap<String, Vec<u8>> {
    snapshot
        .public_assets()
        .iter()
        .map(|(path, digest)| (path.clone(), snapshot.assets()[digest].bytes.clone()))
        .collect()
}

#[test]
fn build_and_relocated_snapshot_preserve_public_paths_bytes_and_icon_links() {
    let root = eunoia();
    let expected = public_bytes(&snapshot(&root));
    commands::build(BuildOptions {
        config: root.path().join("diplodocus.yaml"),
        output: root.path().join("site"),
    })
    .unwrap();
    let original = std::fs::read(root.path().join("site/guide/start.html")).unwrap();
    for (path, bytes) in &expected {
        assert_eq!(
            &std::fs::read(root.path().join("site").join(path)).unwrap(),
            bytes
        );
    }
    let detached = tempfile::tempdir().unwrap();
    let relocated = detached.path().join("relocated.sqlite");
    std::fs::copy(
        root.path().join(".diplodocus/documentation.sqlite"),
        &relocated,
    )
    .unwrap();
    drop(root);
    let loaded = Snapshot::load(&relocated).unwrap();
    assert_eq!(public_bytes(&loaded), expected);
    commands::generate(GenerateOptions {
        input: relocated.clone(),
        output: detached.path().join("site"),
    })
    .unwrap();
    let inactive = std::fs::read_to_string(detached.path().join("site/guide/start.html")).unwrap();
    assert!(inactive.contains("href=\"../public/site.css\""));
    assert!(!inactive.contains("src=\"../public/site.js\""));
    assert_eq!(
        std::fs::read(detached.path().join("site/public/site.js")).unwrap(),
        expected["public/site.js"]
    );
    let generated = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["generate", "--allow-site-scripts", "--input"])
        .arg(&relocated)
        .arg("--output")
        .arg(detached.path().join("site"))
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let site = Site::new(&loaded).unwrap().with_site_scripts(true);
    let fragments = diplodocus::rendering::render_fragments(&site).unwrap();
    assert_eq!(
        fragments.files()["public/site.js"].bytes,
        expected["public/site.js"]
    );
    assert!(
        !String::from_utf8_lossy(&fragments.files()["index.html"].bytes).contains("public/site.")
    );
    assert_eq!(
        std::fs::read(detached.path().join("site/guide/start.html")).unwrap(),
        original
    );
    let rendered = render_site(&site).unwrap();
    for (path, bytes) in &expected {
        assert_eq!(&rendered.files()[path].bytes, bytes);
    }
    assert_eq!(
        rendered.files()["site.webmanifest"].media_type,
        "application/manifest+json"
    );
    assert_eq!(rendered.files()["favicon.ico"].media_type, "image/x-icon");
    for (route, prefix) in [("index.html", ""), ("guide/start.html", "../")] {
        let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
        // Clean URLs resolve assets relative to their public location.
        assert!(
            html.contains(&format!("href=\"{prefix}favicon.svg\"")),
            "{route}: missing icon link"
        );
        assert!(html.contains("rel=\"apple-touch-icon\""));
        assert!(html.contains("rel=\"manifest\""));
        assert!(html.contains("name=\"apple-mobile-web-app-title\" content=\"Eunoia &amp; docs\""));
        assert!(html.contains(&format!("href=\"{prefix}public/site.css\"")));
        assert!(html.contains(&format!("<script defer src=\"{prefix}public/site.js\"")));
        assert!(html.contains(&format!(
            "<script type=\"module\" src=\"{prefix}public/site.mjs\""
        )));
    }
}

#[test]
fn declarations_reject_missing_icons_unknown_repositories_and_unsafe_paths() {
    for extra in [
        "  icons: [{ path: missing.svg }]",
        "  stylesheets: [https://example.test/site.css]",
        "  scripts: [../site.js]",
        "  assets: [{ repository: docs, path: site.js, publish: site.js }]\n  stylesheets: [site.js]",
        "  assets: [{ repository: docs, path: site.js, publish: site.js }]\n  scripts: [site.js, site.js]",
        "  assets: [{ repository: missing, path: icon.svg, publish: icon.svg }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: ../icon.svg }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: /icon.svg }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: a//b }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: a }, { repository: docs, path: icon.svg, publish: a }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: a }, { repository: docs, path: icon.svg, publish: a/b }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: icon.svg }]\n  icons: [{ path: icon.svg, type: image/png }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: icon.svg }]\n  icons: [{ path: icon.svg, rel: manifest }]",
        "  assets: [{ repository: docs, path: icon.svg, publish: icon.svg }]\n  icons: [{ path: icon.svg, sizes: invalid }]",
    ] {
        let root = workspace(extra);
        assert!(
            assemble_workspace(root.path().join("diplodocus.yaml")).is_err(),
            "{extra}"
        );
    }
}

#[test]
fn public_assets_cannot_shadow_generated_artifacts_or_redirects() {
    for path in [
        "index.html",
        "index.md",
        "index.html/child",
        "guide",
        "guide/start",
        "guide/start.md",
        "guide/start.html",
        "myindex",
        "assets/site.css",
        "assets",
        "__diplodocus/reload",
        ".diplodocus-site",
        "robots.txt",
        "sitemap.xml",
        "documents.json",
        "_redirects",
        "old",
    ] {
        let root = workspace(&format!(
            "  assets: [{{ repository: docs, path: static.txt, publish: '{path}' }}]\n  redirects: {{ /old: index.html }}"
        ));
        root.write("static.txt", "public file");
        let snapshot = snapshot(&root);
        assert!(Site::new(&snapshot).is_err(), "{path}");
    }
    let root =
        workspace("  assets: [{ repository: docs, path: static.txt, publish: guide/extra.txt }]");
    root.write("static.txt", "allowed beneath a page directory");
    assert!(Site::new(&snapshot(&root)).is_ok());
}

#[test]
fn public_directory_membership_is_revalidated_and_collisions_are_rejected() {
    let root = workspace(
        "  asset-directories: [{ repository: docs, path: public, publish: '' }]\n  stylesheets: [theme.css]",
    );
    root.write("public/theme.css", "body {}");
    assert!(matches!(
        commands::build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("public/site"),
        }),
        Err(commands::CommandError::InputOverlap)
    ));
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    root.write("public/new.txt", "new");
    assert!(Snapshot::from_sources(&sources, &resolved).is_err());
    root.write("public/guide/start.html", "collision");
    assert!(Site::new(&snapshot(&root)).is_err());
    root.remove("public/theme.css");
    assert!(resolve_workspace(&sources).is_err());
    #[cfg(unix)]
    {
        root.write("theme.css", "body {}");
        std::os::unix::fs::symlink("../theme.css", root.path().join("public/theme.css")).unwrap();
        assert!(resolve_workspace(&sources).is_err());
    }
}

#[test]
fn missing_files_and_changed_bytes_prevent_snapshot_publication() {
    let root = eunoia();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    root.write("static/llms.txt", "changed");
    assert!(Snapshot::from_sources(&sources, &resolved).is_err());
    root.remove("static/llms.txt");
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn public_files_share_bytes_with_document_images_and_support_fragment_url_maps() {
    let root = workspace(
        "  assets: [{ repository: docs, path: pages/icon.svg, publish: public/icon.svg }]\n  icons: [{ path: public/icon.svg }]",
    );
    root.write(
        "pages/icon.svg",
        "<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>",
    );
    root.write("pages/index.md", "# Home\n\n![Icon](icon.svg)\n");
    let snapshot = snapshot(&root);
    assert_eq!(snapshot.assets().len(), 1);
    let site = Site::new(&snapshot)
        .unwrap()
        .with_url_map(diplodocus::site::UrlMap {
            format_version: 1,
            documents: BTreeMap::new(),
            assets: BTreeMap::from([(
                "public/icon.svg".into(),
                "https://example.test/static/icon.svg".into(),
            )]),
        })
        .unwrap();
    let fragments = diplodocus::rendering::render_fragments(&site).unwrap();
    assert_eq!(
        fragments.files()["public/icon.svg"].bytes,
        snapshot.assets().values().next().unwrap().bytes
    );
    assert!(
        !String::from_utf8_lossy(&fragments.files()["index.html"].bytes).contains("rel=\"icon\"")
    );
}

#[cfg(unix)]
#[test]
fn public_files_cannot_follow_symlinks_outside_their_repository() {
    let root =
        workspace("  assets: [{ repository: docs, path: outside.txt, publish: public.txt }]");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        root.path().join("outside.txt"),
    )
    .unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

async fn response(port: u16, path: &str) -> Option<Vec<u8>> {
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
    stream.read_to_end(&mut bytes).await.ok()?;
    Some(bytes)
}

async fn wait_for(port: u16, path: &str, body: &[u8]) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(bytes) = response(port, path).await
                && bytes.starts_with(b"HTTP/1.1 200")
                && bytes.ends_with(body)
            {
                return bytes;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("preview should serve the expected asset bytes")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_preview_serves_identical_assets_and_rebuilds_after_edits_and_recreation() {
    let root = eunoia();
    let expected = public_bytes(&snapshot(&root));
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(commands::serve_with(
        ServeOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("site"),
            host: std::net::Ipv4Addr::LOCALHOST.into(),
            port,
            live_reload: false,
            open: false,
        },
        ExecutionDeadlines::default(),
        Box::pin(async {
            let _ = stopped.await;
        }),
    ));
    for (path, bytes) in &expected {
        wait_for(port, &format!("/{path}"), bytes).await;
    }
    let icon = response(port, "/favicon.svg").await.unwrap();
    assert!(String::from_utf8_lossy(&icon).contains("Content-Type: image/svg+xml\r\n"));
    let page = response(port, "/guide/start").await.unwrap();
    assert!(String::from_utf8_lossy(&page).contains("rel=\"manifest\""));
    assert!(String::from_utf8_lossy(&page).contains("src=\"../public/site.js\""));
    let css = response(port, "/public/site.css").await.unwrap();
    assert!(String::from_utf8_lossy(&css).contains("Content-Type: text/css; charset=utf-8"));
    let js = response(port, "/public/site.js").await.unwrap();
    assert!(String::from_utf8_lossy(&js).contains("Content-Type: text/javascript; charset=utf-8"));
    root.write("static/site.js", "window.siteLoaded = 'updated';");
    wait_for(port, "/public/site.js", b"window.siteLoaded = 'updated';").await;
    root.remove("static/site.js");
    // A failed rebuild must keep the preceding complete generation available.
    tokio::time::sleep(Duration::from_millis(600)).await;
    wait_for(port, "/public/site.js", b"window.siteLoaded = 'updated';").await;
    root.write("static/site.js", "window.siteLoaded = 'recreated';");
    wait_for(port, "/public/site.js", b"window.siteLoaded = 'recreated';").await;
    root.write("catalog/new.json", "[]");
    wait_for(port, "/datasets/new.json", b"[]").await;
    root.remove("catalog/new.json");
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if response(port, "/datasets/new.json")
                .await
                .is_some_and(|bytes| bytes.starts_with(b"HTTP/1.1 404"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("preview should remove deleted directory entries");
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
