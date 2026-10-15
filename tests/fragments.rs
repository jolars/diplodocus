mod support;

use diplodocus::{
    assembly::assemble_workspace,
    rendering::{render_fragments, render_site},
    site::Site,
    snapshots::Snapshot,
    validation::resolve_workspace,
};

#[test]
fn detached_snapshot_supplies_content_without_a_site_shell() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "diplodocus.toml",
        format!(
            "{}\n[[content]]\nid='guide'\nowner='project'\nrepository='demo'\npath='docs'\nmount='guide'\nformat='gfm'\n",
            root.read("diplodocus.toml")
        ),
    );
    root.write(
        "docs/index.md",
        "# A guide\n\n![A retained figure](figure.svg)\n",
    );
    let figure = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><circle cx="10" cy="10" r="5"/></svg>"#;
    root.write("docs/figure.svg", figure);
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let database = artifact.path().join("documentation.sqlite");
    snapshot.publish(&database).unwrap();
    drop(snapshot);
    drop(sources);
    drop(root);

    let snapshot = Snapshot::load(database).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    assert_eq!(index["format_version"], 1);
    let mut overloads = false;
    for document in index["documents"].as_array().unwrap() {
        let path = document["path"].as_str().unwrap();
        let body = std::str::from_utf8(&fragments.files()[path].bytes).unwrap();
        assert!(!body.contains("<!doctype"));
        assert!(!body.contains("<main"));
        assert!(!body.contains("<header"));
        assert!(!body.contains("<footer"));
        assert!(!body.contains("<script"));
        assert!(!body.contains("class=\"page-toc\""));
        assert!(body.contains("<h1>"));
        let full = std::str::from_utf8(&standalone.files()[path].bytes).unwrap();
        for block in body.split("<pre").skip(1) {
            let code = block.split("</pre>").next().unwrap();
            assert!(full.contains(code));
        }
        if document["qualified_name"] == "demo._api.fetch_data" {
            assert!(body.contains("overload-"));
            overloads = true;
        }
    }
    assert!(overloads);
    assert!(fragments.files().contains_key("assets/content.css"));
    assert!(fragments.files().contains_key("assets/search.json"));
    assert!(!fragments.files().contains_key("assets/site.css"));
    assert!(!fragments.files().contains_key("assets/nav.js"));
    let digest = diplodocus::provenance::fingerprint_bytes(figure).value;
    assert_eq!(
        fragments.files()[&format!("assets/{digest}.svg")].bytes,
        figure
    );
    let guide = std::str::from_utf8(&fragments.files()["guide/index.html"].bytes).unwrap();
    assert!(guide.contains(&format!("../assets/{digest}.svg")));
}
