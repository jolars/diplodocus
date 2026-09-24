mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

#[test]
fn static_site_uses_only_snapshot_bytes_and_preserves_output_on_failure() {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    drop(root);
    let site = diplodocus::site::Site::new(&snapshot).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    assert!(rendered.files().contains_key("index.html"));
    assert!(rendered.files().contains_key("guide/index.html"));
    assert!(rendered.files().contains_key("packages/python/index.html"));
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("../assets/"));
    assert!(!html.contains("src=\"/"));
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("site");
    rendered.publish(&path).unwrap();
    let before = std::fs::read(path.join("index.html")).unwrap();
    std::fs::write(path.join("obsolete.html"), "old").unwrap();
    rendered.publish(&path).unwrap();
    assert!(!path.join("obsolete.html").exists());
    assert_eq!(std::fs::read(path.join("index.html")).unwrap(), before);
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
}

#[test]
fn all_rendered_local_links_and_images_resolve_beneath_a_hosting_prefix() {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};
    fn links(node: &Handle, urls: &mut Vec<String>, ids: &mut std::collections::BTreeSet<String>) {
        if let NodeData::Element { attrs, .. } = &node.data {
            for attribute in attrs.borrow().iter() {
                match attribute.name.local.as_ref() {
                    "href" | "src" => urls.push(attribute.value.to_string()),
                    "id" => {
                        ids.insert(attribute.value.to_string());
                    }
                    _ => {}
                }
            }
        }
        for child in node.children.borrow().iter() {
            links(child, urls, ids);
        }
    }
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let mut records = std::collections::BTreeMap::new();
    for (path, file) in rendered
        .files()
        .iter()
        .filter(|(p, _)| p.ends_with(".html"))
    {
        let dom = html5ever::parse_document(RcDom::default(), Default::default())
            .one(std::str::from_utf8(&file.bytes).unwrap());
        let mut urls = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        links(&dom.document, &mut urls, &mut ids);
        records.insert(path.clone(), (urls, ids));
    }
    for (path, (links, _)) in &records {
        let base = url::Url::parse(&format!("https://example.test/diplodocus/{path}")).unwrap();
        for link in links {
            let target = base.join(link).unwrap();
            if target.host_str() != Some("example.test") {
                continue;
            }
            let route = percent_encoding::percent_decode_str(
                target.path().strip_prefix("/diplodocus/").unwrap(),
            )
            .decode_utf8()
            .unwrap();
            assert!(
                rendered.files().contains_key(route.as_ref()),
                "{path} -> {link}"
            );
            if let Some(fragment) = target.fragment() {
                let fragment = percent_encoding::percent_decode_str(fragment)
                    .decode_utf8()
                    .unwrap();
                assert!(
                    records[route.as_ref()].1.contains(fragment.as_ref()),
                    "{path} -> {link}"
                );
            }
        }
    }
}
