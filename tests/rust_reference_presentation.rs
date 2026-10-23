mod support;

use std::collections::BTreeMap;

use diplodocus::commands::{ExtractOptions, extract};
use diplodocus::ir::{ItemLanguageData, ItemReference, RustDeclarationKind};
use diplodocus::rendering::{FragmentIndex, render_fragments, render_site};
use diplodocus::site::{Site, UrlMap};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{DocumentIdentity, resolve_item};

fn route(reference: &ItemReference) -> String {
    format!(
        "packages/rust/reference/{}.html",
        diplodocus::provenance::fingerprint_bytes(reference.item.as_bytes()).value
    )
}

#[test]
fn portable_rust_pages_group_members_and_map_their_links() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    root.write("src/model.rs", root.read("src/model.rs").replace(
        "/// Construction and inherent access to [`Counter`].",
        "/// Construction and inherent access to [`Counter`].\n///\n/// ## Details\n///\n/// See [details](#details).",
    ).replace("/// Construct a value.", "/// Construct a value.\n    ///\n    /// # Examples\n    ///\n    /// See [examples](#examples).")
    .replace("/// A generic counter with a default value type.", "/// A generic counter with a default value type.\n///\n/// # Fields\n///\n/// See [fields](#fields)."));
    root.write("src/lib.rs", root.read("src/lib.rs").replace("/// A struct variant with a documented field.", "/// A struct variant with a documented field.\n    ///\n    /// # Field width\n    ///\n    /// See [field width](#field-width)."));
    let output = tempfile::tempdir().unwrap();
    let snapshot_path = output.path().join("snapshot.sqlite");
    extract(ExtractOptions {
        config: root.path().join("diplodocus.yaml"),
        output: Some(snapshot_path.clone()),
    })
    .unwrap();
    drop(root);
    let snapshot = Snapshot::load(&snapshot_path).unwrap();
    let counter =
        resolve_item(snapshot.workspace(), Some("rustdemo"), "rust_demo::Counter").unwrap();
    let mode = resolve_item(snapshot.workspace(), Some("rustdemo"), "rust_demo::Mode").unwrap();
    let named = resolve_item(snapshot.workspace(), Some("rustdemo"), "rust_demo::Named").unwrap();
    let site = Site::new(&snapshot).unwrap();
    let standalone = render_site(&site).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let counter_document = index
        .documents
        .iter()
        .find(|document| document.path == route(&counter))
        .unwrap();
    assert_eq!(
        counter_document.public_name.as_deref(),
        Some("rust_demo::Counter")
    );
    assert_eq!(counter_document.title, "rust_demo::Counter");
    let counter_html = std::str::from_utf8(&standalone.files()[&route(&counter)].bytes).unwrap();
    let counter_markdown =
        std::str::from_utf8(&fragments.files()[&route(&counter).replace(".html", ".md")].bytes)
            .unwrap();
    assert!(counter_html.contains("class=\"syntax-code\""));
    assert!(counter_html.contains("The current value."));
    assert!(counter_html.contains("Construction and inherent access"));
    assert!(counter_html.contains("A concrete implementation"));
    assert!(counter_markdown.contains("pub struct Counter<T: Clone = u32>"));
    assert!(counter_markdown.contains("where"));
    assert!(counter_markdown.contains("type Value = u32"));
    assert!(counter_html.contains("id=\"field-value\""));
    assert!(counter_html.contains("id=\"documentation-fields\""));
    assert!(counter_html.contains("href=\"#documentation-fields\""));
    assert!(counter_html.contains("id=\"fields\""));
    assert!(counter_markdown.contains("id=\"field-value\""));
    let read_anchors: Vec<_> = counter_document
        .headings
        .iter()
        .filter(|(_, _, title)| title == "read")
        .map(|(_, anchor, _)| anchor)
        .collect();
    assert_eq!(read_anchors.len(), 2);
    assert_ne!(read_anchors[0], read_anchors[1]);
    for (_, anchor, _) in &counter_document.headings {
        assert!(
            counter_html.contains(&format!("id=\"{anchor}\"")),
            "{anchor}"
        );
        assert!(
            counter_markdown.contains(&format!("id=\"{anchor}\"")),
            "{anchor}"
        );
    }
    let impl_details = counter_document
        .headings
        .iter()
        .find(|(_, _, title)| title == "Details")
        .unwrap()
        .1
        .as_str();
    assert!(impl_details.starts_with("impl-"));
    assert!(counter_html.contains(&format!("href=\"#{impl_details}\"")));
    assert!(counter_markdown.contains(&format!("#{impl_details}")));
    let method_examples = counter_document
        .headings
        .iter()
        .find(|(_, _, title)| title == "Examples")
        .unwrap();
    assert_eq!(method_examples.0, 4);
    assert!(method_examples.1.starts_with("method-new-"));
    assert!(counter_html.contains(&format!("href=\"#{}\"", method_examples.1)));
    for identity in &counter_document.identities {
        if let DocumentIdentity::Item { item } = identity
            && item != &counter
        {
            assert!(!standalone.files().contains_key(&route(item)));
        }
    }
    let mode_html = std::str::from_utf8(&standalone.files()[&route(&mode)].bytes).unwrap();
    assert!(mode_html.contains("id=\"variant-Fast\""));
    assert!(mode_html.contains("id=\"variant-Custom-field-width\""));
    assert!(mode_html.contains("id=\"variant-Custom-documentation-field-width\""));
    assert!(mode_html.contains("The requested width."));
    let named_html = std::str::from_utf8(&standalone.files()[&route(&named)].bytes).unwrap();
    assert!(named_html.contains("id=\"associated-type-Value\""));
    assert!(named_html.contains("id=\"associated-constant-LABEL\""));
    assert!(named_html.contains("id=\"method-read\""));
    assert!(!named_html.contains("hidden(&amp;self)"));

    let mapped = render_fragments(
        &Site::new(&snapshot)
            .unwrap()
            .with_url_map(UrlMap {
                format_version: 1,
                documents: BTreeMap::from([(route(&counter), "/api/counter/".into())]),
                assets: BTreeMap::new(),
            })
            .unwrap(),
    )
    .unwrap();
    let mapped_index: FragmentIndex =
        serde_json::from_slice(&mapped.files()["documents.json"].bytes).unwrap();
    assert_eq!(
        mapped_index
            .documents
            .iter()
            .find(|document| document.path == route(&counter))
            .unwrap()
            .url,
        "/api/counter/"
    );
    let guide =
        std::str::from_utf8(&mapped.files()["packages/rust/guide/index.html"].bytes).unwrap();
    assert!(guide.contains("href=\"/api/counter/\""));
    assert!(guide.contains("href=\"/api/counter/#method-new-"));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&mapped.files()["assets/search.json"].bytes).unwrap();
    let new_entry = search
        .iter()
        .find(|entry| {
            entry["title"]
                .as_str()
                .is_some_and(|title| title.ends_with("Counter::new"))
        })
        .unwrap();
    assert!(
        new_entry["path"]
            .as_str()
            .unwrap()
            .starts_with("/api/counter/#method-new-")
    );
    assert!(counter_document.identities.iter().any(|identity| {
        let DocumentIdentity::Item { item } = identity else { return false };
        matches!(&snapshot.workspace().packages[&item.package].items[&item.item].language_data,
            Some(ItemLanguageData::Rust(data)) if data.declaration == RustDeclarationKind::Implementation)
    }));
}
