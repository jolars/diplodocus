mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use diplodocus::commands::{self, BuildOptions, CheckOptions};
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

fn collect_urls(
    node: &Handle,
    urls: &mut Vec<String>,
    ids: &mut BTreeSet<String>,
    canonicals: &mut Vec<String>,
) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let canonical = name.local.as_ref() == "link"
            && attrs.borrow().iter().any(|attr| {
                attr.name.local.as_ref() == "rel" && attr.value.as_ref() == "canonical"
            });
        for attribute in attrs.borrow().iter() {
            match attribute.name.local.as_ref() {
                "href" if canonical => canonicals.push(attribute.value.to_string()),
                "href" | "src" => urls.push(attribute.value.to_string()),
                "id" => {
                    ids.insert(attribute.value.to_string());
                }
                _ => {}
            }
        }
    }
    for child in node.children.borrow().iter() {
        collect_urls(child, urls, ids, canonicals);
    }
}

#[test]
fn real_project_site_builds_and_resolves_at_its_domain_root() {
    let workspace = support::own_documentation_workspace();
    let config = workspace.path().join("diplodocus.toml");
    let output = workspace.path().join("site");

    let report = commands::check(CheckOptions {
        config: config.clone(),
    })
    .unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    commands::build(BuildOptions {
        config,
        output: output.clone(),
    })
    .unwrap();

    let home = fs::read_to_string(output.join("index.html")).unwrap();
    assert!(home.contains("<h1>Diplodocus</h1>"));
    assert!(home.contains("This guide is built by Diplodocus"));
    assert!(!output.join("guide/index.html").exists());
    let example = fs::read_to_string(output.join("examples/stateful.html")).unwrap();
    assert!(example.contains("Total: 12"));
    assert!(example.contains("Example stderr"));

    let mut pages = BTreeMap::new();
    let mut canonical_urls = BTreeSet::new();
    for path in support::files_under(&output) {
        if path.extension().is_none_or(|extension| extension != "html") {
            continue;
        }
        let route = path.to_str().unwrap().replace('\\', "/");
        let html = fs::read_to_string(output.join(&path)).unwrap();
        let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
        let mut urls = Vec::new();
        let mut ids = BTreeSet::new();
        let mut canonicals = Vec::new();
        collect_urls(&dom.document, &mut urls, &mut ids, &mut canonicals);
        let public_route = if route == "index.html" {
            ""
        } else if route.ends_with("/index.html") {
            route.strip_suffix("index.html").unwrap()
        } else {
            route.strip_suffix(".html").unwrap()
        };
        let expected = format!("https://diplodocus.cc/{public_route}");
        assert_eq!(canonicals.as_slice(), std::slice::from_ref(&expected));
        assert!(canonical_urls.insert(expected));
        pages.insert(route, (urls, ids));
    }

    let sitemap = fs::read_to_string(output.join("sitemap.xml")).unwrap();
    let xml = roxmltree::Document::parse(&sitemap).unwrap();
    let locations: BTreeSet<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name("loc"))
        .map(|node| node.text().unwrap().to_owned())
        .collect();
    assert_eq!(locations, canonical_urls);
    assert_eq!(
        fs::read_to_string(output.join("robots.txt")).unwrap(),
        "User-agent: *\nAllow: /\n\nSitemap: https://diplodocus.cc/sitemap.xml\n"
    );

    for (route, (urls, _)) in &pages {
        let base = url::Url::parse(&format!("https://diplodocus.cc/{route}")).unwrap();
        for reference in urls {
            let target = base.join(reference).unwrap();
            if target.host_str() != Some("diplodocus.cc") {
                continue;
            }
            let path = target.path().strip_prefix('/').unwrap();
            let path = if path.is_empty() { "index.html" } else { path };
            let path = percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .unwrap();
            assert!(
                output.join(path.as_ref()).is_file(),
                "{route} -> {reference}"
            );
            if let Some(fragment) = target.fragment() {
                let fragment = percent_encoding::percent_decode_str(fragment)
                    .decode_utf8()
                    .unwrap();
                assert!(
                    pages[path.as_ref()].1.contains(fragment.as_ref()),
                    "{route} -> {reference}"
                );
            }
        }
    }

    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(output.join("assets/search.json")).unwrap()).unwrap();
    assert_eq!(search.len(), pages.len());
    for entry in &search {
        let path = entry["path"].as_str().unwrap();
        let route = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .unwrap();
        assert!(pages.contains_key(route.as_ref()), "search -> {path}");
    }
    for (title, route) in [
        ("Diplodocus", "index.html"),
        ("Installation", "installation.html"),
        ("Commands", "cli.html"),
        ("Portable snapshots", "snapshots.html"),
        ("A deterministic Python page", "examples/stateful.html"),
    ] {
        let entry = search
            .iter()
            .find(|entry| entry["title"] == title)
            .unwrap_or_else(|| panic!("missing search entry for {title}"));
        let path = entry["path"].as_str().unwrap();
        assert_eq!(
            percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .unwrap(),
            route
        );
        assert!(output.join(route).is_file());
    }
    assert!(search.iter().any(|entry| {
        entry["text"]
            .as_str()
            .unwrap()
            .contains("run with the privileges")
    }));
}
