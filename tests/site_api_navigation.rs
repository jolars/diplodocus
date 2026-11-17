mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn render(root: &support::TestWorkspace) -> RenderedSite {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    render_site(&Site::new(&snapshot).unwrap()).unwrap()
}

fn html<'a>(rendered: &'a RenderedSite, route: &str) -> &'a str {
    std::str::from_utf8(&rendered.files()[route].bytes).unwrap()
}

#[test]
fn api_dropdown_survives_detached_generation_and_preserves_package_sections() {
    for style in ["file", "clean"] {
        let root = support::TestWorkspace::from_fixture("api-navigation");
        root.write(
            "diplodocus.yaml",
            root.read("diplodocus.yaml").replace(
                "presentation:\n",
                &format!("presentation:\n  canonical-url-style: {style}\n"),
            ),
        );
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let portable = support::TestWorkspace::new();
        let path = portable.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        drop(sources);
        drop(root);
        let snapshot = Snapshot::load(&path).unwrap();
        let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let guide = html(&rendered, "guide.html");
        let header = guide.split("</header>").next().unwrap();
        let desktop = header.split("class=\"api-menu\"").nth(1).unwrap();
        let panel = desktop.split("</details>").next().unwrap();
        let mut position = 0;
        for language in ["R", "Julia", "Rust"] {
            position += panel[position..].find(&format!(">{language}</a>")).unwrap();
        }
        assert!(!header.contains("Hidden Rust tools"));
        assert!(header.contains("class=\"mobile-api\"><strong>API</strong>"));
        assert!(header.contains(">Reference</a>"));
        let destination = if style == "file" {
            "packages/rust/index.html"
        } else {
            "packages/rust/"
        };
        assert!(panel.contains(&format!("href=\"{destination}\">Rust</a>")));
        for (slug, label) in [("r", "R"), ("julia", "Julia"), ("rust", "Rust")] {
            let package = html(&rendered, &format!("packages/{slug}/index.html"));
            let header = package.split("</header>").next().unwrap();
            assert_eq!(
                header.matches("aria-current=\"true\"").count(),
                2,
                "one selected link in each navbar"
            );
            assert!(header.contains(&format!("aria-current=\"true\">{label}</a>")));
            assert!(package.contains(&format!("data-section=\"{slug}\"")));
        }
    }
}

#[test]
fn api_dropdown_is_default_and_links_can_be_selected_explicitly() {
    for mode in [None, Some("links"), Some("dropdown")] {
        for count in [0, 1, 3] {
            let root = support::TestWorkspace::from_fixture("api-navigation");
            let mut config = support::fixture_configuration(&root, "diplodocus.yaml");
            if let Some(mode) = mode {
                config["presentation"]["header"] = serde_json::json!({"api": mode});
            }
            config["packages"].as_array_mut().unwrap().truncate(count);
            root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
            let rendered = render(&root);
            let header = html(&rendered, "index.html")
                .split("</header>")
                .next()
                .unwrap();
            assert_eq!(
                header.contains("class=\"api-menu\""),
                mode != Some("links") && count > 1,
                "{mode:?}, {count} packages"
            );
            assert_eq!(header.matches("href=\"packages/").count(), 2 * count);
        }
    }
}

#[test]
fn api_dropdown_uses_escaped_package_names_when_languages_are_shared() {
    let root = support::TestWorkspace::from_fixture("api-navigation");
    let mut config = support::fixture_configuration(&root, "diplodocus.yaml");
    config["packages"][3]["visibility"] = "public".into();
    config["packages"][3]["name"] = "Rust <extras> & tools".into();
    root.write("diplodocus.yaml", serde_saphyr::to_string(&config).unwrap());
    let rendered = render(&root);
    let header = html(&rendered, "index.html")
        .split("</header>")
        .next()
        .unwrap();
    assert!(header.contains(">Rust &lt;extras&gt; &amp; tools</a>"));
    assert!(header.contains(">Rust tools</a>"));
    assert!(!header.contains(">Rust</a>"));
}
