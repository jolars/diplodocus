mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::render_site;
use diplodocus::site::{Site, SiteError};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(style: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        format!(
            r#"project:
  name: "Home & docs"
presentation:
  canonical-url-style: "{style}"
  home:
    tagline: "One <home>"
    description: "Read & explore."
    guide: "guide/index.html"
    actions:
      - label: "Start <here>"
        path: "guide/quick-start.html"
    features:
      - title: "One & all"
        description: "Shared <references>."
        path: "guide/index.html"
repositories:
  - id: "docs"
    path: "."
content:
  - id: "guide"
    owner: "project"
    repository: "docs"
    path: "docs"
    mount: "guide"
    format: "gfm"
"#
        ),
    );
    root.write("docs/index.md", "# Guide\n\nWelcome to the guide.\n");
    root.write("docs/quick-start.md", "# Quick start\n\nStart here.\n");
    root
}

#[test]
fn homepage_survives_snapshot_round_trip_and_respects_url_style() {
    for style in ["file", "clean"] {
        let root = workspace(style);
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let path = root.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        let snapshot = Snapshot::load(&path).unwrap();
        let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let home = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        assert!(home.contains("data-layout=\"home\""));
        assert!(home.contains("<h1>One &lt;home&gt;</h1>"));
        assert!(home.contains("Read &amp; explore."));
        assert!(home.contains("Start &lt;here&gt;"));
        assert!(home.contains("Shared &lt;references&gt;."));
        assert_eq!(home.matches("<h1").count(), 1);
        assert!(!home.contains("class=\"page-toc"));
        let guide_url = if style == "file" {
            "guide/index.html"
        } else {
            "guide/"
        };
        let quick_start_url = if style == "file" {
            "guide/quick-start.html"
        } else {
            "guide/quick-start"
        };
        assert!(home.contains(&format!("href=\"{guide_url}\">Guide</a>")));
        assert!(home.contains(&format!(
            "href=\"{quick_start_url}\">Start &lt;here&gt;</a>"
        )));
        assert!(!home.contains("class=\"home-action-icon\""));
        assert!(home.contains(&format!(
            "<a class=\"home-feature\" href=\"{guide_url}\"><h2>One &amp; all<span aria-hidden=\"true\"> &nearr;</span></h2>"
        )));
        let guide = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
        assert!(guide.contains("data-layout=\"docs\""));
        assert!(guide.contains("Welcome to the guide."));
        let header = guide.split("</header>").next().unwrap();
        assert!(header.contains("class=\"brand\""));
        assert!(!guide.contains("aria-label=\"Documentation\"><a class=\"brand\""));
        assert!(header.contains("class=\"site-search\""));
        assert!(!guide.contains("home-hero"));
        let markdown = std::str::from_utf8(&rendered.files()["index.md"].bytes).unwrap();
        assert!(markdown.contains("Read & explore."));
        assert!(markdown.contains(quick_start_url));
    }
}

#[test]
fn homepage_action_icons_survive_snapshot_round_trip_without_sources() {
    let root = workspace("file");
    let icons = ["arrow-right", "book-open", "code", "download", "terminal"];
    let actions: String = icons
        .iter()
        .map(|icon| {
            format!(
                "      - label: {icon}\n        path: guide/quick-start.html\n        icon: {icon}\n"
            )
        })
        .collect();
    let config = root
        .read("diplodocus.yaml")
        .replace("    actions:\n", &format!("    actions:\n{actions}"));
    root.write("diplodocus.yaml", config);
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let portable = support::TestWorkspace::new();
    let path = portable.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.presentation(), snapshot.presentation());
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let home = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    let actions = home
        .split("<a class=\"home-action\"")
        .skip(1)
        .map(|action| action.split_once("</a>").unwrap().0)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), icons.len() + 1);
    for (action, icon) in actions.iter().zip(icons) {
        assert!(action.ends_with(&format!("</svg>{icon}")));
        let start = action.find("<svg").unwrap();
        let end = action.find("</svg>").unwrap() + "</svg>".len();
        let document = roxmltree::Document::parse(&action[start..end]).unwrap();
        let element = document.root_element();
        assert_eq!(element.attribute("aria-hidden"), Some("true"));
        assert_eq!(element.attribute("focusable"), Some("false"));
        assert_eq!(element.attribute("stroke"), Some("currentColor"));
        assert_eq!(element.attribute("viewBox"), Some("0 0 24 24"));
        assert!(element.children().any(|child| child.has_tag_name("path")));
    }
    assert!(!actions.last().unwrap().contains("<svg"));
    let markdown = std::str::from_utf8(&rendered.files()["index.md"].bytes).unwrap();
    for icon in icons {
        assert!(markdown.contains(&format!("- [{icon}](<guide/quick-start.html>)")));
    }
    assert!(!markdown.contains("<svg"));
}

#[test]
fn homepage_rejects_missing_and_self_destinations() {
    for (original, replacement) in [
        ("guide/index.html", "missing.html"),
        ("guide/quick-start.html", "missing.html"),
        ("guide/index.html", "index.html"),
    ] {
        let root = workspace("file");
        let config = root.read("diplodocus.yaml").replace(original, replacement);
        root.write("diplodocus.yaml", config);
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        assert!(matches!(
            Site::new(&snapshot),
            Err(SiteError::Presentation(_))
        ));
    }
}

#[test]
fn homepage_preserves_authored_body_and_title_anchor_without_a_second_heading() {
    let root = workspace("file");
    root.write(
        "diplodocus.yaml",
        support::merge_configurations(&root.read("diplodocus.yaml"), "content:\n  - id: \"home\"\n    owner: \"project\"\n    repository: \"docs\"\n    path: \"home\"\n    mount: \"\"\n    format: \"qmd\"\n"),
    );
    root.write(
        "home/index.qmd",
        "---\ntitle: Overview\nlayout: full\n---\n\n# Welcome {#welcome}\n\nAn authored introduction.\n",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(html.contains("data-layout=\"home\""));
    assert_eq!(html.matches("<h1").count(), 1);
    assert!(html.contains("<h1 id=\"welcome\">One &lt;home&gt;</h1>"));
    assert!(html.contains("An authored introduction."));
}
