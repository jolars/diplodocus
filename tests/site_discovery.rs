mod support;

use std::collections::BTreeMap;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, BuildOptions, GenerateOptions};
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

fn workspace(settings: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", format!("[project]\nname='Discovery'\n{settings}\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='gfm'\n"));
    for path in ["index.md", "guide/index.md", "guide/start.md", "café &%.md"] {
        root.write(format!("docs/{path}"), "# Documentation\n");
    }
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn render(snapshot: &Snapshot) -> RenderedSite {
    render_site(&Site::new(snapshot).unwrap()).unwrap()
}

fn locations(rendered: &RenderedSite) -> Vec<String> {
    let xml = std::str::from_utf8(&rendered.files()["sitemap.xml"].bytes).unwrap();
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    let document = roxmltree::Document::parse(xml).unwrap();
    let root = document.root_element();
    assert!(root.has_tag_name(("http://www.sitemaps.org/schemas/sitemap/0.9", "urlset")));
    root.children()
        .filter(|node| node.is_element())
        .map(|node| {
            assert_eq!(node.tag_name().name(), "url");
            let children: Vec<_> = node.children().filter(|child| child.is_element()).collect();
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].tag_name().name(), "loc");
            children[0].text().unwrap().to_owned()
        })
        .collect()
}

fn canonical(node: &Handle, urls: &mut Vec<String>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let attrs = attrs.borrow();
        if name.local.as_ref() == "link"
            && attrs
                .iter()
                .any(|attr| attr.name.local.as_ref() == "rel" && attr.value.as_ref() == "canonical")
        {
            urls.push(
                attrs
                    .iter()
                    .find(|attr| attr.name.local.as_ref() == "href")
                    .unwrap()
                    .value
                    .to_string(),
            );
        }
    }
    for child in node.children.borrow().iter() {
        canonical(child, urls);
    }
}

#[test]
fn sitemap_and_canonical_links_share_encoded_urls_in_both_styles() {
    for (style, suffixes) in [
        (
            "file",
            [
                "index.html",
                "guide/index.html",
                "guide/start.html",
                "caf%C3%A9%20%26%25.html",
            ],
        ),
        ("clean", ["", "guide/", "guide/start", "caf%C3%A9%20%26%25"]),
    ] {
        for base in [
            "https://example.test",
            "https://example.test/",
            "http://example.test/docs/a&b",
            "http://example.test/docs/a&b/",
        ] {
            let root = workspace(&format!(
                "[presentation]\nsite-url='{base}'\ncanonical-url-style='{style}'"
            ));
            let snapshot = snapshot(&root);
            let rendered = render(&snapshot);
            let prefix = format!("{}/", base.trim_end_matches('/'));
            let expected: BTreeMap<_, _> = [
                "index.html",
                "guide/index.html",
                "guide/start.html",
                "café &%.html",
            ]
            .into_iter()
            .zip(suffixes)
            .map(|(route, suffix)| (route, format!("{prefix}{suffix}")))
            .collect();
            let mut expected_locations: Vec<_> = expected.values().cloned().collect();
            expected_locations.sort();
            assert_eq!(locations(&rendered), expected_locations);
            for (route, url) in expected {
                let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
                let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
                let mut urls = Vec::new();
                canonical(&dom.document, &mut urls);
                assert_eq!(urls, [url]);
            }
            assert_eq!(
                rendered.files()["sitemap.xml"].media_type,
                "application/xml; charset=utf-8"
            );
            assert_eq!(
                rendered.files()["robots.txt"].media_type,
                "text/plain; charset=utf-8"
            );
            assert_eq!(
                std::str::from_utf8(&rendered.files()["robots.txt"].bytes).unwrap(),
                format!("User-agent: *\nAllow: /\n\nSitemap: {prefix}sitemap.xml\n")
            );
            assert_eq!(
                render(&snapshot).files()["sitemap.xml"].bytes,
                rendered.files()["sitemap.xml"].bytes
            );
        }
    }
}

#[test]
fn sitemap_matches_search_visibility_for_packages_and_concepts() {
    let root = support::acceptance_workspace();
    let mut config = support::fixture_configuration(&root, "workspace/diplodocus.toml");
    config.as_table_mut().unwrap().insert(
        "presentation".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "site-url".into(),
            "https://example.test/docs/".into(),
        )])),
    );
    config["package"][0]
        .as_table_mut()
        .unwrap()
        .insert("visibility".into(), "internal".into());
    config["package"][1]
        .as_table_mut()
        .unwrap()
        .insert("visibility".into(), "hidden".into());
    root.write(
        "workspace/diplodocus.toml",
        toml::to_string(&config).unwrap(),
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render(&snapshot);
    let urls = locations(&rendered);
    assert!(urls.iter().any(|url| url.contains("/packages/python/")));
    assert!(urls.iter().any(|url| url.contains("/concepts/")));
    assert!(!urls.iter().any(|url| url.contains("/packages/r/")));
    assert!(!urls.iter().any(|url| url.contains("/assets/")));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&rendered.files()["assets/search.json"].bytes).unwrap();
    let mut expected: Vec<_> = search
        .iter()
        .map(|entry| {
            let route = percent_encoding::percent_decode_str(entry["path"].as_str().unwrap())
                .decode_utf8()
                .unwrap();
            format!("https://example.test/docs/{route}")
        })
        .collect();
    expected.sort();
    assert_eq!(urls, expected);
}

#[test]
fn discovery_survives_portable_generation_and_is_removed_when_disabled() {
    let root = workspace(
        "[presentation]\nsite-url='https://example.test/docs'\ncanonical-url-style='clean'",
    );
    let snapshot = snapshot(&root);
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("documentation.sqlite");
    let output = directory.path().join("site");
    snapshot.publish(&input).unwrap();
    drop(root);
    let loaded = Snapshot::load(&input).unwrap();
    assert_eq!(
        loaded.canonical_export().unwrap(),
        snapshot.canonical_export().unwrap()
    );
    commands::generate(GenerateOptions {
        input,
        output: output.clone(),
    })
    .unwrap();
    let rendered = render(&loaded);
    for path in ["sitemap.xml", "robots.txt", "index.html"] {
        assert_eq!(
            std::fs::read(output.join(path)).unwrap(),
            rendered.files()[path].bytes
        );
    }
    let root = workspace("");
    let disabled = render(&self::snapshot(&root));
    disabled.publish(&output).unwrap();
    assert!(!output.join("sitemap.xml").exists());
    assert!(!output.join("robots.txt").exists());
    assert!(
        !std::fs::read_to_string(output.join("index.html"))
            .unwrap()
            .contains("rel=\"canonical\"")
    );
}

#[test]
fn ambiguous_clean_urls_fail_before_replacing_a_site() {
    for conflicting in [
        "guide.md",
        "guide/index/index.md",
        "sitemap.xml.md",
        "robots.txt.md",
    ] {
        let root = workspace(
            "[presentation]\nsite-url='https://example.test/'\ncanonical-url-style='clean'",
        );
        let output = root.path().join("site");
        render(&snapshot(&root)).publish(&output).unwrap();
        let before = std::fs::read(output.join("sitemap.xml")).unwrap();
        root.write(format!("docs/{conflicting}"), "# Collision\n");
        assert!(
            commands::build(BuildOptions {
                config: root.path().join("diplodocus.toml"),
                output: output.clone()
            })
            .is_err(),
            "{conflicting}"
        );
        assert_eq!(std::fs::read(output.join("sitemap.xml")).unwrap(), before);
    }
}
