mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn sidebar(html: &str) -> &str {
    html.split("class=\"contextual-nav\"")
        .nth(1)
        .unwrap()
        .split("</details>")
        .next()
        .unwrap()
}

#[test]
fn collection_navigation_survives_snapshots_without_hiding_pages_or_search_results() {
    for owner in ["project", "rstats"] {
        for style in ["file", "clean"] {
            for policy in ["all", "index", "none"] {
                let root = support::TestWorkspace::new();
                root.write(
                    "diplodocus.yaml",
                    format!(
                        r#"project:
  name: Navigation
presentation:
  site-url: https://example.test/docs/
  canonical-url-style: {style}
repositories:
  - id: docs
    path: .
packages:
  - id: rstats
    name: Stats for R
    slug: r
    ecosystem: r
    repository: docs
    path: .
    metadata-path: DESCRIPTION
    targets: []
content:
  - id: guide
    owner: project
    repository: docs
    path: guide
    mount: ""
    format: gfm
  - id: datasets
    owner: {owner}
    repository: docs
    path: datasets
    mount: datasets
    format: gfm
    navigation: {policy}
"#
                    ),
                );
                root.write(
                    "DESCRIPTION",
                    support::load_fixture("acceptance/r/DESCRIPTION"),
                );
                root.write("guide/index.md", "# Guide\n");
                root.write("guide/start.md", "# Getting started\n");
                root.write("datasets/index.md", "# Datasets\n\n[Iris](uci/iris.md)\n");
                root.write("datasets/uci/index.md", "# Nested catalog\n");
                root.write("datasets/uci/iris.md", "# Iris\n\n[Catalog](../index.md)\n");
                let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
                let snapshot =
                    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap())
                        .unwrap();
                let path = root.path().join("snapshot.sqlite");
                snapshot.publish(&path).unwrap();
                std::fs::remove_dir_all(root.path().join("datasets")).unwrap();
                let snapshot = Snapshot::load(path).unwrap();
                let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
                let prefix = if owner == "project" {
                    ""
                } else {
                    "packages/r/"
                };
                for suffix in ["index.html", "uci/index.html", "uci/iris.html"] {
                    let route = format!("{prefix}datasets/{suffix}");
                    let html = std::str::from_utf8(&rendered.files()[&route].bytes).unwrap();
                    let navigation = sidebar(html);
                    assert_eq!(
                        navigation.contains("aria-label=\"Datasets\""),
                        policy != "none"
                    );
                    assert_eq!(navigation.contains(">Iris</a>"), policy == "all");
                    assert_eq!(navigation.contains(">Nested catalog</a>"), policy == "all");
                    assert!(html.contains("<h1"));
                    assert!(
                        rendered
                            .files()
                            .contains_key(&route.replace(".html", ".md"))
                    );
                }
                let guide = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
                assert!(sidebar(guide).contains("Getting started"));
                let search =
                    std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
                assert!(search.contains("Iris"));
                assert!(search.contains("Nested catalog"));
                let sitemap = std::str::from_utf8(&rendered.files()["sitemap.xml"].bytes).unwrap();
                assert!(sitemap.contains(&format!("{prefix}datasets/uci/iris")));
            }
        }
    }
}

#[test]
fn navigation_none_also_omits_the_automatic_overview_link() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        "project:\n  name: Navigation\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n    navigation: none\n",
    );
    root.write("docs/index.md", "# Overview\n");
    root.write("docs/detail.md", "# Detail\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    for route in ["index.html", "detail.html"] {
        let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
        assert!(!sidebar(html).contains("<a "));
    }
}

#[test]
fn navigation_index_does_not_promote_nested_indexes_when_the_root_index_is_missing() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        "project:\n  name: Navigation\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: guide\n    mount: \"\"\n    format: gfm\n  - id: catalog\n    owner: project\n    repository: docs\n    path: catalog\n    mount: catalog\n    format: gfm\n    navigation: index\n",
    );
    root.write("guide/index.md", "# Guide\n");
    root.write("catalog/nested/index.md", "# Nested catalog\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["catalog/nested/index.html"].bytes).unwrap();
    assert!(!sidebar(html).contains("Nested catalog"));
    assert!(!sidebar(html).contains("aria-label=\"Catalog\""));
}
