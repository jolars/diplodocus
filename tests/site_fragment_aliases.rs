mod support;

use std::time::Duration;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, BuildOptions, CheckOptions, GenerateOptions, ServeOptions};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::rendering::{RenderedSite, render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Legacy fragments\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: ''\n    format: qmd\n");
    root.write("docs/index.qmd", "# Home\n");
    root.write("docs/guide/start.qmd", "# Getting started {#start}\n\n## Installation {#install}\n\nInstall the package.\n\n## Repeated\n\nFirst.\n\n## Repeated\n\nSecond.\n\n## Getting *started* with `code`\n\n::: {.panel-tabset}\n\n## First tab\n\nFirst panel.\n\n## Second tab\n\nSecond panel.\n\n:::\n");
    root
}

fn configure(root: &support::TestWorkspace, presentation: serde_json::Value) {
    let mut config: serde_json::Value =
        serde_saphyr::from_str(&root.read("diplodocus.yaml")).unwrap();
    config["presentation"] = presentation;
    root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn contents(site: &RenderedSite, route: &str) -> String {
    String::from_utf8(site.files()[route].bytes.clone()).unwrap()
}

fn declarations() -> serde_json::Value {
    json!({
        "guide/start.html": {
            "Getting-Started": {"heading":"Getting started"},
            "start": {"anchor":"start"},
            "autotoc_md0": {"anchor":"install"},
            "second-repeat": {"anchor":"repeated-2"},
            "formatted": {"heading":" Getting started  with code "},
            "Eunoia.label_boxes-Tuple{AbstractDict, AbstractDict}": {"heading":"Second tab"},
            "café<&\"%": {"anchor":"install"}
        }
    })
}

#[test]
fn title_level_tab_headings_keep_the_same_aliases_in_html_and_markdown() {
    let root = workspace();
    root.write("docs/guide/start.qmd", "# Guide\n\n::: {.panel-tabset}\n\n# First panel\n\nFirst.\n\n# Second panel\n\nSecond.\n\n:::\n");
    configure(
        &root,
        json!({"fragment-aliases":{"guide/start.html":{
            "old-panel":{"heading":"Second panel"},
            "first-panel":{"anchor":"first-panel"}
        }}}),
    );
    commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap();
    let snapshot = snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    for route in ["guide/start.html", "guide/start.md"] {
        let output = contents(&rendered, route);
        for id in ["old-panel", "first-panel", "second-panel"] {
            assert_eq!(
                output.matches(&format!("id=\"{id}\"")).count(),
                1,
                "{route}: {id}"
            );
        }
    }
}

#[test]
fn aliases_preserve_canonical_html_markdown_search_and_fragment_headings() {
    for style in ["file", "clean"] {
        let root = workspace();
        configure(&root, json!({"canonical-url-style":style}));
        let original = snapshot(&root);
        let before = render_site(&Site::new(&original).unwrap()).unwrap();
        let original_fragments = render_fragments(&Site::new(&original).unwrap()).unwrap();
        configure(
            &root,
            json!({"canonical-url-style":style,"fragment-aliases":declarations()}),
        );
        commands::check(CheckOptions {
            config: root.path().join("diplodocus.yaml"),
        })
        .unwrap();
        let snapshot = snapshot(&root);
        let site = Site::new(&snapshot).unwrap();
        let rendered = render_site(&site).unwrap();
        let html = contents(&rendered, "guide/start.html");
        let markdown = contents(&rendered, "guide/start.md");
        assert!(html.contains("<h1 id=\"start\"><span id=\"Getting-Started\""));
        assert!(html.contains("<h2 id=\"install\"><span id=\"autotoc_md0\""));
        assert!(html.contains("<h2 id=\"repeated-2\"><span id=\"second-repeat\""));
        assert!(html.contains("id=\"café&lt;&amp;&quot;%\""));
        assert!(html.contains("class=\"tabset-heading\" id=\"second-tab\""));
        for (id, _) in declarations()["guide/start.html"].as_object().unwrap() {
            let escaped = id
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            assert_eq!(
                html.matches(&format!("id=\"{escaped}\"")).count(),
                1,
                "{id}"
            );
            assert_eq!(
                markdown.matches(&format!("id=\"{escaped}\"")).count(),
                1,
                "{id}"
            );
        }
        assert_eq!(
            contents(&before, "assets/search.json"),
            contents(&rendered, "assets/search.json")
        );
        let toc = html.split("<aside class=\"page-toc\">").nth(1).unwrap();
        assert!(toc.contains("href=\"#install\""));
        assert!(!toc.contains("autotoc_md0"));
        let fragments = render_fragments(&site).unwrap();
        assert_eq!(
            contents(&original_fragments, "documents.json"),
            contents(&fragments, "documents.json")
        );
        assert!(contents(&fragments, "guide/start.html").contains("id=\"Getting-Started\""));
        assert_eq!(contents(&fragments, "guide/start.md"), markdown);
        assert!(!contents(&before, "guide/start.html").contains("class=\"fragment-alias\""));
    }
}

#[test]
fn invalid_aliases_fail_checks_without_creating_storage() {
    for (aliases, message) in [
        (
            json!({"missing.html":{"old":{"heading":"Missing"}}}),
            "generated site-relative HTML",
        ),
        (
            json!({"/guide/start.html":{"old":{"anchor":"install"}}}),
            "generated site-relative HTML",
        ),
        (
            json!({"guide/start.html":{"old":{"heading":"Missing"}}}),
            "missing",
        ),
        (
            json!({"guide/start.html":{"old":{"heading":"Repeated"}}}),
            "ambiguous",
        ),
        (
            json!({"guide/start.html":{"old":{"anchor":"missing"}}}),
            "missing",
        ),
        (
            json!({"guide/start.html":{"old":{"heading":"installation"}}}),
            "missing",
        ),
        (
            json!({"guide/start.html":{"old":{"anchor":"install"},"other":{"anchor":"old"}}}),
            "missing",
        ),
        (
            json!({"guide/start.html":{"repeated":{"anchor":"install"}}}),
            "conflicts",
        ),
        (
            json!({"guide/start.html":{"main":{"anchor":"install"}}}),
            "conflicts",
        ),
        (
            json!({"guide/start.html":{"desktop-sections":{"anchor":"install"}}}),
            "conflicts",
        ),
        (
            json!({"guide/start.html":{"":{"anchor":"install"}}}),
            "nonempty",
        ),
        (
            json!({"guide/start.html":{"bad\nID":{"anchor":"install"}}}),
            "control characters",
        ),
    ] {
        let root = workspace();
        configure(&root, json!({"fragment-aliases":aliases}));
        let error = commands::check(CheckOptions {
            config: root.path().join("diplodocus.yaml"),
        })
        .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert!(!root.path().join(".diplodocus").exists());
    }
}

#[test]
fn aliases_diagnose_duplicate_document_ids_and_declarations() {
    let root = workspace();
    root.write(
        "docs/guide/start.qmd",
        "# Title\n\n## First {#duplicate}\n\n## Second {#duplicate}\n",
    );
    configure(
        &root,
        json!({"fragment-aliases":{"guide/start.html":{"old":{"heading":"First"}}}}),
    );
    let error = commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap_err();
    assert!(error.to_string().contains("duplicate ID"), "{error}");
    root.write("docs/guide/start.qmd", "# Start\n");
    root.write("diplodocus.yaml", format!("{}\npresentation:\n  fragment-aliases:\n    index.html:\n      old: {{heading: Home}}\n      old: {{heading: Home}}\n", workspace().read("diplodocus.yaml")));
    let error = commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap_err();
    assert!(error.to_string().contains("duplicate"), "{error}");
}

#[test]
fn detached_generation_retains_aliases_and_route_redirects() {
    for style in ["file", "clean"] {
        let root = workspace();
        configure(
            &root,
            json!({"canonical-url-style":style,"site-url":"https://example.test/manual/","redirects":{"/old":"guide/start.html"},"fragment-aliases":declarations()}),
        );
        let output = tempfile::tempdir().unwrap();
        commands::build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: output.path().join("built"),
        })
        .unwrap();
        let database = output.path().join("portable.sqlite");
        std::fs::copy(
            root.path().join(".diplodocus/documentation.sqlite"),
            &database,
        )
        .unwrap();
        let export = Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap();
        drop(root);
        let loaded = Snapshot::load(&database).unwrap();
        assert_eq!(loaded.canonical_export().unwrap(), export);
        assert_eq!(
            serde_json::to_value(&loaded.presentation().fragment_aliases).unwrap(),
            declarations()
        );
        commands::generate(GenerateOptions {
            input: database,
            output: output.path().join("regenerated"),
        })
        .unwrap();
        support::assert_output_tree(
            &output.path().join("built"),
            &output.path().join("regenerated"),
        );
        let rules = std::fs::read_to_string(output.path().join("regenerated/_redirects")).unwrap();
        assert_eq!(
            rules,
            format!(
                "/manual/old /manual/guide/start{} 301\n",
                if style == "file" { ".html" } else { "" }
            )
        );
    }
}

#[test]
fn removing_aliases_cleans_output_and_failed_build_preserves_publication() {
    let root = workspace();
    configure(&root, json!({"fragment-aliases":declarations()}));
    let config = root.path().join("diplodocus.yaml");
    let output = root.path().join("site");
    let build = || {
        commands::build(BuildOptions {
            config: config.clone(),
            output: output.clone(),
        })
    };
    build().unwrap();
    let before = root.read("site/guide/start.html");
    configure(
        &root,
        json!({"fragment-aliases":{"guide/start.html":{"old":{"heading":"Missing"}}}}),
    );
    assert!(build().is_err());
    assert_eq!(root.read("site/guide/start.html"), before);
    configure(&root, json!({"fragment-aliases":{}}));
    build().unwrap();
    assert!(
        !root
            .read("site/guide/start.html")
            .contains("class=\"fragment-alias\"")
    );
    assert!(
        serde_json::to_value(snapshot(&root).presentation())
            .unwrap()
            .get("fragment-aliases")
            .is_none()
    );
}

async fn request(port: u16, path: &str) -> String {
    let mut socket = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(socket) =
                tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await
            {
                break socket;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    socket.read_to_end(&mut bytes).await.unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_preview_keeps_aliases_and_the_last_successful_generation() {
    let root = workspace();
    configure(
        &root,
        json!({"canonical-url-style":"clean","redirects":{"/old":"guide/start.html"},"fragment-aliases":declarations()}),
    );
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let options = ServeOptions {
        config: root.path().join("diplodocus.yaml"),
        output: root.path().join("site"),
        host: std::net::Ipv4Addr::LOCALHOST.into(),
        port,
        live_reload: false,
    };
    let server = tokio::spawn(commands::serve_with(
        options,
        ExecutionDeadlines::default(),
        Box::pin(async {
            let _ = stopped.await;
        }),
    ));
    assert!(
        request(port, "/guide/start")
            .await
            .contains("id=\"Getting-Started\"")
    );
    assert!(
        request(port, "/old")
            .await
            .contains("Location: /guide/start\r\n")
    );
    let mut updated = declarations();
    updated["guide/start.html"]["Updated.Alias"] = json!({"anchor":"install"});
    configure(
        &root,
        json!({"canonical-url-style":"clean","fragment-aliases":updated}),
    );
    let before = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let response = request(port, "/guide/start").await;
            if response.contains("id=\"Updated.Alias\"") {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    configure(
        &root,
        json!({"fragment-aliases":{"guide/start.html":{"old":{"heading":"Missing"}}}}),
    );
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            if let Ok(snapshot) = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
                && matches!(snapshot.presentation().fragment_aliases.get("guide/start.html").and_then(|aliases| aliases.get("old")),Some(diplodocus::configuration::FragmentAliasTarget::Heading(label)) if label == "Missing") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    assert_eq!(request(port, "/guide/start").await, before);
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn eunoia_compatibility_aliases_cover_julia_titles_fields_and_methods() {
    let root = workspace();
    let mut config: serde_json::Value =
        serde_saphyr::from_str(&root.read("diplodocus.yaml")).unwrap();
    config["packages"] = json!([{
        "id":"julia","name":"Eunoia","slug":"julia","ecosystem":"julia","repository":"docs","path":"julia","metadata-path":"Project.toml",
        "targets":[{"id":"api","extractor":"julia","path":"src/Eunoia.jl","role":"public-api"}]
    }]);
    root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
    root.write(
        "julia/Project.toml",
        "name = \"Eunoia\"\nuuid = \"12345678-1234-1234-1234-123456789abc\"\nversion = \"1.0.0\"\n",
    );
    root.write("julia/README.md", "# Eunoia\n\n## Installation\n\nInstall.\n\n## Quick start\n\nUse.\n\n## Plotting\n\nPlot.\n\n## Citation\n\nCite.\n");
    root.write(
        "julia/src/Eunoia.jl",
        r#"module Eunoia
export BoundingBox, label_boxes
struct Point
    x::Float64
    y::Float64
end
"A bounding box."
struct BoundingBox
    center::Point
    width::Float64
    height::Float64
end
"""
Construct a bounding box.

## Details

Use real coordinates.

## More details

The box retains its dimensions.
"""
BoundingBox(x::Real, y::Real, width::Real, height::Real) = BoundingBox(Point(x, y), width, height)
"Place boxes."
label_boxes(placements::AbstractDict, sizes::AbstractDict; padding::Real = 0) = placements
end
"#,
    );
    let baseline = snapshot(&root);
    let rendered = render_fragments(&Site::new(&baseline).unwrap()).unwrap();
    let index: diplodocus::rendering::FragmentIndex =
        serde_json::from_str(&contents(&rendered, "documents.json")).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy-fragments/eunoia.json")).unwrap();
    let mut mappings = serde_json::Map::new();
    for (old_route, name) in [
        ("/docs/reference/julia/", None),
        (
            "/docs/reference/julia/BoundingBox/",
            Some("Eunoia.BoundingBox"),
        ),
        (
            "/docs/reference/julia/label_boxes/",
            Some("Eunoia.label_boxes"),
        ),
    ] {
        let destination = if let Some(name) = name {
            &index
                .documents
                .iter()
                .find(|page| page.qualified_name.as_deref() == Some(name))
                .unwrap()
                .path
        } else {
            "packages/julia/index.html"
        };
        let entry = &fixture[old_route];
        let mut aliases = serde_json::Map::new();
        for field in ["headings", "aliases"] {
            for (id, label) in entry[field].as_object().unwrap() {
                aliases.insert(id.clone(), json!({"heading":label}));
            }
        }
        if name == Some("Eunoia.BoundingBox") {
            let page = index
                .documents
                .iter()
                .find(|page| page.path == destination)
                .unwrap();
            let details: Vec<_> = page
                .headings
                .iter()
                .filter(|(_, _, title)| matches!(title.as_str(), "Details" | "More details"))
                .collect();
            assert_eq!(details.len(), 2);
            for (i, (_, anchor, _)) in details.iter().enumerate() {
                aliases.insert(
                    format!("old-prefixed-details-{i}"),
                    json!({"anchor":anchor}),
                );
            }
        }
        mappings.insert(destination.into(), aliases.into());
    }
    configure(&root, json!({"fragment-aliases":mappings}));
    commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap();
    let snapshot = self::snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    for (route, aliases) in &snapshot.presentation().fragment_aliases {
        let html = contents(&rendered, route);
        let markdown = contents(&rendered, &route.replace(".html", ".md"));
        for id in aliases.keys() {
            assert!(html.contains(&format!("id=\"{id}\"")), "{route}: {id}");
            assert!(markdown.contains(&format!("id=\"{id}\"")), "{route}: {id}");
        }
    }
}

#[test]
fn python_member_and_overload_aliases_preserve_signature_anchors() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    let original = snapshot(&root);
    let fragments = render_fragments(&Site::new(&original).unwrap()).unwrap();
    let index: diplodocus::rendering::FragmentIndex =
        serde_json::from_str(&contents(&fragments, "documents.json")).unwrap();
    let mut declarations = serde_json::Map::new();
    for (name, member) in [(".fetch_data", None), (".Reader", Some("read"))] {
        let page = index
            .documents
            .iter()
            .find(|page| {
                page.qualified_name
                    .as_ref()
                    .is_some_and(|qualified| qualified.ends_with(name))
            })
            .unwrap();
        let html = contents(&fragments, &page.path);
        let overload = format!(
            "overload-{}",
            html.split("id=\"overload-")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        );
        let mut aliases = serde_json::Map::new();
        aliases.insert("legacy-overload".into(), json!({"anchor":overload}));
        if let Some(member) = member {
            aliases.insert("legacy-member".into(), json!({"heading":member}));
        }
        declarations.insert(page.path.clone(), aliases.into());
    }
    configure(&root, json!({"fragment-aliases":declarations}));
    commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap();
    let snapshot = snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    for (route, aliases) in &snapshot.presentation().fragment_aliases {
        for id in aliases.keys() {
            assert_eq!(
                contents(&rendered, route)
                    .matches(&format!("id=\"{id}\""))
                    .count(),
                1
            );
            assert_eq!(
                contents(&rendered, &route.replace(".html", ".md"))
                    .matches(&format!("id=\"{id}\""))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn qualpal_compatibility_ids_attach_to_authored_headings_and_cpp_members() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy-fragments/qualpal.json")).unwrap();
    let original = snapshot(&root);
    let rendered = render_fragments(&Site::new(&original).unwrap()).unwrap();
    let index: diplodocus::rendering::FragmentIndex =
        serde_json::from_str(&contents(&rendered, "documents.json")).unwrap();
    let builder = index
        .documents
        .iter()
        .find(|page| page.qualified_name.as_deref() == Some("demo::Builder"))
        .unwrap();
    let member = &builder
        .headings
        .iter()
        .find(|(_, _, title)| title == "setWhitePoint")
        .unwrap()
        .1;
    let ids = fixture["anchors"]["classqualpal_1_1Qualpal.html"]
        .as_array()
        .unwrap();
    let mut aliases = serde_json::Map::new();
    for id in ids
        .iter()
        .filter_map(|id| id.as_str())
        .filter(|id| id.starts_with("a040") || id.starts_with("r_a040"))
    {
        aliases.insert(id.into(), json!({"anchor":member}));
    }
    aliases.insert("details".into(), json!({"heading":"demo::Builder"}));
    configure(
        &root,
        json!({"fragment-aliases":{builder.path.clone():aliases}}),
    );
    commands::check(CheckOptions {
        config: root.path().join("diplodocus.yaml"),
    })
    .unwrap();
    let snapshot = snapshot(&root);
    for rendered in [
        render_site(&Site::new(&snapshot).unwrap()).unwrap(),
        render_fragments(&Site::new(&snapshot).unwrap()).unwrap(),
    ] {
        for id in aliases.keys() {
            for route in [builder.path.clone(), builder.path.replace(".html", ".md")] {
                assert!(
                    contents(&rendered, &route).contains(&format!("id=\"{id}\"")),
                    "{route}: {id}"
                );
            }
        }
    }
    let root = workspace();
    let mut page = String::from("# Algorithm\n\n");
    let mut aliases = serde_json::Map::new();
    for (i, id) in fixture["anchors"]["algorithm.html"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        page.push_str(&format!("## Step {i}\n\nDetails.\n\n"));
        aliases.insert(
            id.as_str().unwrap().into(),
            json!({"heading":format!("Step {i}")}),
        );
    }
    root.write("docs/algorithm.qmd", page);
    configure(
        &root,
        json!({"fragment-aliases":{"algorithm.html":aliases,"index.html":{"mainpage":{"heading":"Home"}}}}),
    );
    let snapshot = self::snapshot(&root);
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    for id in aliases.keys() {
        assert!(contents(&rendered, "algorithm.html").contains(&format!("id=\"{id}\"")));
        assert!(contents(&rendered, "algorithm.md").contains(&format!("id=\"{id}\"")));
    }
}
