mod support;

use std::time::Duration;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, BuildOptions, CheckOptions, GenerateOptions, ServeOptions};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::rendering::{RenderedSite, render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn workspace(style: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        format!(
            r#"
project:
  name: Eunoia
presentation:
  site-url: https://example.test/manual/
  canonical-url-style: {style}
  description: Site default
  open-graph:
    repository: docs
    path: og.png
    alt: Diagram
repositories:
  - id: docs
    path: .
packages:
  - id: python
    name: Python
    slug: python
    ecosystem: python
    repository: docs
    path: python
    metadata-path: pyproject.toml
    targets:
      - id: api
        extractor: python
        path: eunoia
        role: public-api
content:
  - id: guide
    owner: project
    repository: docs
    path: docs
    mount: ""
    format: gfm
"#
        ),
    );
    root.write("docs/index.md", "# Eunoia\n");
    root.write("docs/guide.md", "# Authored guide\n\nOriginal content.\n");
    root.write("docs/fallback.md", "# Fallback\n");
    root.write(
        "python/pyproject.toml",
        "[project]\nname = 'eunoia'\nversion = '1.2.3'\ndescription = 'Package description'\n",
    );
    root.write(
        "python/eunoia/__init__.py",
        "def fit(values):\n    \"\"\"Fit a diagram.\"\"\"\n    return values\n",
    );
    let mut image = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(12, 6)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    root.write("og.png", image.into_inner());
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn configure(root: &support::TestWorkspace, pages: serde_json::Value) {
    let mut config: serde_json::Value =
        serde_saphyr::from_str(&root.read("diplodocus.yaml")).unwrap();
    config["presentation"]["pages"] = pages;
    root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
}

fn html(site: &RenderedSite, path: &str) -> String {
    String::from_utf8(site.files()[path].bytes.clone()).unwrap()
}

fn assert_metadata(html: &str, title: &str, description: &str) {
    assert!(
        html.contains(&format!("<title>{title}</title>")),
        "missing title {title}"
    );
    for attribute in ["property=\"og:title\"", "name=\"twitter:title\""] {
        assert!(html.contains(&format!("<meta {attribute} content=\"{title}\">")));
    }
    for attribute in [
        "name=\"description\"",
        "property=\"og:description\"",
        "name=\"twitter:description\"",
    ] {
        assert!(html.contains(&format!("<meta {attribute} content=\"{description}\">")));
    }
}

#[test]
fn page_metadata_overrides_head_tags_and_preserves_authored_content() {
    for style in ["file", "clean"] {
        let root = workspace(style);
        let original = render_site(&Site::new(&snapshot(&root)).unwrap()).unwrap();
        configure(
            &root,
            json!({
                "index.html": {"title": "Eunoia"},
                "guide.html": {"title": "Guide <&> · Eunoia", "description": "Fit <sets> & \"regions\"."}
            }),
        );
        let snapshot = snapshot(&root);
        let site = Site::new(&snapshot).unwrap();
        let rendered = render_site(&site).unwrap();
        assert_metadata(
            &html(&rendered, "guide.html"),
            "Guide &lt;&amp;&gt; · Eunoia",
            "Fit &lt;sets&gt; &amp; &quot;regions&quot;.",
        );
        assert_metadata(&html(&rendered, "index.html"), "Eunoia", "Site default");
        assert_metadata(
            &html(&rendered, "fallback.html"),
            "Fallback · Eunoia",
            "Site default",
        );
        let before = html(&original, "guide.html");
        let after = html(&rendered, "guide.html");
        assert_eq!(
            before.split("</head>").nth(1),
            after.split("</head>").nth(1)
        );
        for path in ["guide.md", "assets/search.json"] {
            assert_eq!(original.files()[path].bytes, rendered.files()[path].bytes);
        }
        let fragments = render_fragments(&site).unwrap();
        assert!(!html(&fragments, "guide.html").contains("<meta "));
    }
}

#[test]
fn generated_package_and_api_metadata_survive_build_and_detached_generation() {
    let root = workspace("clean");
    let initial = snapshot(&root);
    let (id, _) = initial.workspace().packages["python"]
        .items
        .iter()
        .find(|(_, item)| item.qualified_name == "eunoia.fit")
        .unwrap();
    let api = format!(
        "packages/python/reference/{}.html",
        fingerprint_bytes(id.as_bytes()).value
    );
    configure(
        &root,
        json!({
            "packages/python/index.html": {"title": "Eunoia for Python · Eunoia", "description": "Python package documentation."},
            api.clone(): {"title": "Fit diagrams · Eunoia", "description": "Fit circles & ellipses."}
        }),
    );
    commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap();
    let output = tempfile::tempdir().unwrap();
    commands::build(BuildOptions {
        config: root.path().join("diplodocus.yaml"),
        output: output.path().join("built"),
    })
    .unwrap();
    let loaded = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let relocated = output.path().join("relocated/snapshot.sqlite");
    loaded.publish(&relocated).unwrap();
    drop(root);
    commands::generate(GenerateOptions {
        input: relocated.clone(),
        output: output.path().join("generated"),
    })
    .unwrap();
    support::assert_output_tree(
        &output.path().join("built"),
        &output.path().join("generated"),
    );
    let snapshot = Snapshot::load(relocated).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert_metadata(
        &html(&rendered, "packages/python/index.html"),
        "Eunoia for Python · Eunoia",
        "Python package documentation.",
    );
    assert_metadata(
        &html(&rendered, &api),
        "Fit diagrams · Eunoia",
        "Fit circles &amp; ellipses.",
    );
}

#[test]
fn metadata_paths_and_titles_are_checked_and_duplicate_titles_use_overrides() {
    let root = workspace("clean");
    for path in [
        "/guide",
        "guide",
        "/guide.html",
        "missing.html",
        "guide.md",
        "guide.html#heading",
        "../guide.html",
    ] {
        configure(&root, json!({path: {"description": "Missing target"}}));
        let error = commands::check(CheckOptions {
            config: root.path().join("diplodocus.yaml"),
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("presentation.pages key"),
            "{error}"
        );
    }
    configure(&root, json!({"guide.html": {"title": "  "}}));
    assert!(
        Site::new(&snapshot(&root))
            .err()
            .unwrap()
            .to_string()
            .contains("title must not be empty")
    );
    configure(
        &root,
        json!({"guide.html": {"title": "Same"}, "fallback.html": {"title": "Same"}}),
    );
    let snapshot = snapshot(&root);
    let warnings = Site::new(&snapshot).unwrap().title_diagnostics();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("\"Same\""));
    configure(
        &root,
        json!({"guide.html": {"title": "Different"}, "fallback.html": {"title": "Same"}}),
    );
    assert!(
        Site::new(&self::snapshot(&root))
            .unwrap()
            .title_diagnostics()
            .is_empty()
    );
}

#[test]
fn omitted_metadata_keeps_snapshot_shape_and_descriptions_can_be_cleared() {
    let root = workspace("file");
    assert!(
        serde_json::to_value(snapshot(&root).presentation())
            .unwrap()
            .get("pages")
            .is_none()
    );
    configure(&root, json!({"guide.html": {"description": ""}}));
    let snapshot = snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert_metadata(
        &html(&rendered, "guide.html"),
        "Authored guide · Eunoia",
        "",
    );
    let mut config: serde_json::Value =
        serde_saphyr::from_str(&root.read("diplodocus.yaml")).unwrap();
    config["presentation"]
        .as_object_mut()
        .unwrap()
        .remove("description");
    config["presentation"]
        .as_object_mut()
        .unwrap()
        .remove("pages");
    root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
    let snapshot = self::snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert!(!html(&rendered, "guide.html").contains(":description\""));
    assert!(!html(&rendered, "guide.html").contains("name=\"description\""));
}

async fn wait_for_metadata(port: u16, expected: &str) -> String {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(mut stream) =
                tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await
            {
                stream
                    .write_all(
                        b"GET /guide HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).await.unwrap();
                let response = String::from_utf8(bytes).unwrap();
                if response.starts_with("HTTP/1.1 200") && response.contains(expected) {
                    return response;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("preview should serve updated page metadata")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_preview_matches_snapshot_metadata_and_rebuilds_configuration_edits() {
    let root = workspace("clean");
    configure(
        &root,
        json!({"guide.html": {"title": "Guide preview", "description": "Preview description"}}),
    );
    let expected = render_site(&Site::new(&snapshot(&root)).unwrap()).unwrap();
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
        },
        ExecutionDeadlines::default(),
        Box::pin(async {
            let _ = stopped.await;
        }),
    ));
    let response = wait_for_metadata(port, "<title>Guide preview</title>").await;
    assert!(response.ends_with(&html(&expected, "guide.html")));
    configure(
        &root,
        json!({"guide.html": {"title": "Updated preview", "description": "Updated description"}}),
    );
    let response = wait_for_metadata(port, "<title>Updated preview</title>").await;
    assert_metadata(&response, "Updated preview", "Updated description");
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
