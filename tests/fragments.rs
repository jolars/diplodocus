mod support;

use diplodocus::{
    assembly::assemble_workspace,
    rendering::{FragmentIndex, render_fragments, render_site},
    site::{Site, UrlMap},
    snapshots::Snapshot,
    validation::resolve_workspace,
};

#[test]
fn detached_snapshot_supplies_content_without_a_site_shell() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "diplodocus.yaml",
        format!(
            "{}\ncontent:\n  - id: \"guide\"\n    owner: \"project\"\n    repository: \"demo\"\n    path: \"docs\"\n    mount: \"guide\"\n    format: \"gfm\"\n",
            root.read("diplodocus.yaml")
        ),
    );
    root.write(
        "docs/index.md",
        "# A guide\n\n![A retained figure](figure.svg)\n\n[`demo.fetch_data`]\n",
    );
    let figure = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><circle cx="10" cy="10" r="5"/></svg>"#;
    root.write("docs/figure.svg", figure);
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
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
    assert_eq!(index["format_version"], 2);
    let mut overloads = false;
    for document in index["documents"].as_array().unwrap() {
        let path = document["path"].as_str().unwrap();
        let body = std::str::from_utf8(&fragments.files()[path].bytes).unwrap();
        let markdown_path = document["markdown_path"].as_str().unwrap();
        let markdown = std::str::from_utf8(&fragments.files()[markdown_path].bytes).unwrap();
        assert!(markdown.starts_with("# "));
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
            assert!(markdown.contains("overload-"));
            assert!(markdown.contains("Literal[True]"));
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

    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let urls = UrlMap {
        format_version: 1,
        documents: index
            .documents
            .iter()
            .enumerate()
            .map(|(i, d)| (d.path.clone(), format!("/reference/{i}/")))
            .collect(),
        assets: fragments
            .files()
            .keys()
            .filter(|path| path.starts_with("assets/"))
            .map(|path| {
                (
                    path.clone(),
                    format!("/_api/{}", path.strip_prefix("assets/").unwrap()),
                )
            })
            .collect(),
    };
    let mapped_site = Site::new(&snapshot)
        .unwrap()
        .with_url_map(urls.clone())
        .unwrap();
    let mapped = render_fragments(&mapped_site).unwrap();
    let guide = std::str::from_utf8(&mapped.files()["guide/index.html"].bytes).unwrap();
    let guide_markdown = std::str::from_utf8(&mapped.files()["guide/index.md"].bytes).unwrap();
    let function = index
        .documents
        .iter()
        .find(|d| d.public_name.as_deref() == Some("demo.fetch_data"))
        .unwrap();
    assert!(guide.contains(&format!("href=\"{}\"", urls.documents[&function.path])));
    assert!(guide_markdown.contains(&urls.documents[&function.path]));
    assert!(guide.contains(&format!("src=\"/_api/{digest}.svg\"")));
    assert!(guide_markdown.contains(&format!("/_api/{digest}.svg")));
    let search = std::str::from_utf8(&mapped.files()["assets/search.json"].bytes).unwrap();
    assert!(search.contains(&urls.documents[&function.path]));
    assert_eq!(
        mapped.files().keys().collect::<Vec<_>>(),
        fragments.files().keys().collect::<Vec<_>>()
    );
    let css = std::str::from_utf8(&mapped.files()["assets/content.css"].bytes).unwrap();
    assert!(css.contains(".diplodocus-content .syntax-"));
    assert!(css.contains(".dark .diplodocus-content .syntax-"));
    for (key, value) in [
        ("missing.html", "/missing/"),
        (function.path.as_str(), "javascript:alert(1)"),
    ] {
        let mut invalid = urls.clone();
        invalid.documents.insert(key.into(), value.into());
        assert!(Site::new(&snapshot).unwrap().with_url_map(invalid).is_err());
    }
}

#[test]
fn cli_fragment_generation_preserves_output_after_invalid_mapping() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("docs.sqlite");
    snapshot.publish(&database).unwrap();
    drop(snapshot);
    drop(sources);
    drop(root);
    let output = temporary.path().join("fragments");
    let command = || std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    let result = command()
        .args(["generate", "--format", "fragments", "--input"])
        .arg(&database)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let before = std::fs::read(output.join("documents.json")).unwrap();
    let map = temporary.path().join("map.json");
    std::fs::write(
        &map,
        r#"{"format_version":1,"documents":{},"assets":{"assets/missing.png":"/missing.png"}}"#,
    )
    .unwrap();
    let result = command()
        .args(["generate", "--format", "fragments", "--input"])
        .arg(&database)
        .arg("--output")
        .arg(&output)
        .arg("--url-map")
        .arg(&map)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown asset"));
    assert_eq!(
        std::fs::read(output.join("documents.json")).unwrap(),
        before
    );
}
