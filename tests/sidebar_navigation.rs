mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::configuration::parse_configuration;
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::{Site, SiteError};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(sidebar: &str, style: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        format!(
            r#"project:
  name: Sidebar
presentation:
  canonical-url-style: {style}
  site-url: https://example.test/docs/
  sidebar: {sidebar}
repositories:
  - id: docs
    path: .
content:
  - id: guide
    owner: project
    repository: docs
    path: guide
    mount: ""
    format: gfm
"#
        ),
    );
    for (path, title) in [
        ("index.md", "Guide"),
        ("intro/index.md", "Introduction"),
        ("intro/start.md", "Getting started"),
        ("intro/deep/topic.md", "Nested topic"),
        ("reference/index.md", "Reference"),
    ] {
        root.write(format!("guide/{path}"), format!("# {title}\n"));
    }
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn sidebar<'a>(rendered: &'a RenderedSite, route: &str) -> &'a str {
    std::str::from_utf8(&rendered.files()[route].bytes)
        .unwrap()
        .split_once("class=\"contextual-nav\"")
        .unwrap()
        .1
        .split_once("</nav>")
        .unwrap()
        .0
}

const TREE: &str = r#"
    - text: Introduction & basics
      link: intro/index.html
      collapsed: true
      items:
        - text: Start <here>
          link: intro/start.html
        - text: Default Theme
          items:
            - text: Overview
              link: index.html
            - text: Nested group
              collapsed: false
              items:
                - text: Nested topic
                  link: intro/deep/topic.html
    - text: Reference
      items:
        - text: Reference overview
          link: reference/index.html
"#;

#[test]
fn nested_sidebar_survives_snapshots_without_sources_and_respects_url_styles() {
    for style in ["file", "clean"] {
        let root = workspace(TREE, style);
        let original = snapshot(&root);
        let portable = support::TestWorkspace::new();
        let path = portable.path().join("snapshot.sqlite");
        original.publish(&path).unwrap();
        drop(root);
        let loaded = Snapshot::load(path).unwrap();
        assert_eq!(loaded.presentation(), original.presentation());
        let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
        let nav = sidebar(&rendered, "reference/index.html");
        assert!(nav.contains("<h2 class=\"sidebar-heading\"><a"));
        assert!(nav.contains("Introduction &amp; basics"));
        assert!(nav.contains("Start &lt;here&gt;"));
        assert!(nav.contains(
            "<span class=\"sidebar-heading\">Default Theme</span><ul class=\"sidebar-items\">"
        ));
        assert!(nav.contains("data-sidebar-key=\"0\"><summary>"));
        assert!(nav.contains("data-sidebar-key=\"0.1.1\" open>"));
        assert!(nav.find("Start &lt;here&gt;").unwrap() < nav.find("Nested topic").unwrap());
        let destination = if style == "file" {
            "../intro/start.html"
        } else {
            "../intro/start"
        };
        assert!(nav.contains(&format!("href=\"{destination}\"")), "{nav}");
        let active = sidebar(&rendered, "intro/start.html");
        assert!(active.contains("data-sidebar-key=\"0\" open>"));
        assert!(active.contains("aria-current=\"page\">Start &lt;here&gt;</a>"));
        let landing = sidebar(&rendered, "intro/index.html");
        assert!(landing.contains("data-sidebar-key=\"0\" open>"));
    }
}

#[test]
fn directory_sidebars_choose_the_longest_prefix_and_preserve_automatic_fallback() {
    let root = workspace(
        r#"
    /intro/:
      - text: Intro menu
        link: intro/start.html
    /intro/deep/:
      - text: Deep menu
        link: intro/deep/topic.html
    /reference/: []
"#,
        "file",
    );
    let loaded = snapshot(&root);
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let intro = sidebar(&rendered, "intro/start.html");
    assert!(intro.contains("data-section=\"/intro/\""));
    assert!(intro.contains("Intro menu"));
    assert!(!intro.contains("Deep menu"));
    let deep = sidebar(&rendered, "intro/deep/topic.html");
    assert!(deep.contains("Deep menu"));
    assert!(!deep.contains("Intro menu"));
    assert!(!sidebar(&rendered, "reference/index.html").contains("<a "));
    let automatic = sidebar(&rendered, "index.html");
    assert!(automatic.contains("aria-label=\"Guide\""));
    assert!(automatic.contains("Getting started"));
}

#[test]
fn root_prefix_sidebar_matches_pages_outside_more_specific_directories() {
    let root = workspace(
        "\n    /:\n      - text: Everywhere\n        link: index.html\n    /intro/: []\n",
        "clean",
    );
    let loaded = snapshot(&root);
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    assert!(sidebar(&rendered, "reference/index.html").contains("Everywhere"));
    assert!(!sidebar(&rendered, "intro/start.html").contains("Everywhere"));
}

#[test]
fn invalid_sidebar_items_and_prefixes_report_presentation_errors() {
    for (config, message) in [
        (
            "[{text: Missing, link: missing.html}]",
            "visible generated page",
        ),
        ("[{text: ' ', link: index.html}]", "need text"),
        ("[{text: Empty}]", "link or children"),
        (
            "[{text: Leaf, link: index.html, collapsed: false}]",
            "collapsed requires children",
        ),
        ("{intro: []}", "site-root directories"),
        ("{'/intro': []}", "site-root directories"),
        ("{'//': []}", "site-root directories"),
        ("{'/../': []}", "site-root directories"),
        ("{'/intro//deep/': []}", "site-root directories"),
    ] {
        let root = workspace(config, "file");
        assert!(
            matches!(Site::new(&snapshot(&root)), Err(SiteError::Presentation(error)) if error.contains(message)),
            "{config}"
        );
    }
    let mut tree = "{text: Leaf, link: index.html}".to_owned();
    for _ in 0..5 {
        tree = format!("{{text: Group, items: [{tree}]}}");
    }
    let root = workspace(&format!("[{tree}]"), "file");
    assert!(Site::new(&snapshot(&root)).is_ok());
    tree = format!("{{text: Group, items: [{tree}]}}");
    let root = workspace(&format!("[{tree}]"), "file");
    assert!(
        matches!(Site::new(&snapshot(&root)), Err(SiteError::Presentation(error)) if error.contains("six levels"))
    );
}

#[test]
fn explicit_sidebar_preserves_package_api_trees_and_rejects_hidden_targets() {
    for visibility in ["public", "hidden"] {
        let root = workspace(
            "\n    /packages/python/:\n      - text: Package guide\n        link: packages/python/index.html\n",
            "file",
        );
        root.write(
            "diplodocus.yaml",
            format!(
                "{}\npackages:\n  - id: tiny\n    name: Tiny\n    slug: python\n    ecosystem: python\n    repository: docs\n    path: .\n    metadata-path: pyproject.toml\n    visibility: {visibility}\n    targets:\n      - id: api\n        extractor: python\n        path: tiny\n        role: public-api\n",
                root.read("diplodocus.yaml")
            ),
        );
        root.write(
            "pyproject.toml",
            "[project]\nname = 'tiny'\nversion = '0.1.0'\n",
        );
        root.write(
            "tiny/__init__.py",
            "def mean(values):\n    return sum(values) / len(values)\n",
        );
        let loaded = snapshot(&root);
        if visibility == "hidden" {
            assert!(
                matches!(Site::new(&loaded), Err(SiteError::Presentation(error)) if error.contains("visible generated page"))
            );
        } else {
            let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
            let nav = sidebar(&rendered, "packages/python/index.html");
            assert!(nav.contains("Package guide"));
            assert!(nav.contains("aria-label=\"API reference\""));
            assert!(nav.contains("<code>mean</code>"));
        }
    }
}

#[test]
fn explicit_sidebar_can_link_to_pages_omitted_from_automatic_navigation() {
    let root = workspace("[{text: Custom start, link: intro/start.html}]", "file");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml") + "    navigation: none\n",
    );
    let loaded = snapshot(&root);
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    assert!(sidebar(&rendered, "index.html").contains("Custom start"));
    assert!(rendered.files().contains_key("intro/deep/topic.html"));
    let search = std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
    assert!(search.contains("Nested topic"));
}

#[test]
fn sidebar_configuration_round_trips_and_rejects_unknown_fields_and_bad_types() {
    for tree in [TREE, "[]", "{ '/intro/': [] }"] {
        let root = workspace(tree, "file");
        let config = parse_configuration(&root.read("diplodocus.yaml")).unwrap();
        assert_eq!(
            parse_configuration(&serde_saphyr::to_string(&config).unwrap()).unwrap(),
            config
        );
    }
    for tree in [
        "[{text: Intro, link: index.html, collapse: true}]",
        "[{text: Intro, link: index.html, collapsed: 'yes'}]",
        "[{text: Intro, items: false}]",
        "[{text: Intro, link: '../index.html'}]",
        "[{text: Intro, link: 'javascript:alert(1)'}]",
        "[{text: Intro, link: '/index.html'}]",
        "[{text: Intro, text: Duplicate, link: index.html}]",
        "{'/intro/': [], '/intro/': []}",
        "false",
    ] {
        let root = workspace(tree, "file");
        assert!(
            parse_configuration(&root.read("diplodocus.yaml")).is_err(),
            "{tree}"
        );
    }
}
