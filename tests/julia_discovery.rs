mod support;

use std::collections::BTreeSet;

use diplodocus::{
    assembly::assemble_workspace,
    ir::{ItemKind, ItemLanguageData, JuliaCallableRole, JuliaDeclaration, Package},
    rendering::{FragmentDocument, FragmentIndex, RenderedSite, render_fragments, render_site},
    site::{Site, UrlMap},
    snapshots::Snapshot,
    validation::{DocumentIdentity, resolve_workspace},
};

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(
        sources.workspace().diagnostics.is_empty(),
        "{:?}",
        sources.workspace().diagnostics
    );
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn document<'a>(index: &'a FragmentIndex, name: &str) -> &'a FragmentDocument {
    index
        .documents
        .iter()
        .find(|document| document.qualified_name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text<'a>(rendered: &'a RenderedSite, path: &str) -> &'a str {
    std::str::from_utf8(&rendered.files()[path].bytes).unwrap()
}

fn public_names(package: &Package) -> BTreeSet<&str> {
    package
        .discoverable_items()
        .into_iter()
        .filter_map(|id| {
            let item = &package.items[id];
            (item.kind != ItemKind::Method).then_some(item.qualified_name.as_str())
        })
        .collect()
}

#[test]
fn julia_discovery_uses_public_bindings_owners_and_required_modules() {
    let root = support::TestWorkspace::from_fixture("julia-discovery");
    let snapshot = snapshot(&root);
    let package = &snapshot.workspace().packages["demo"];
    assert_eq!(
        public_names(package),
        BTreeSet::from([
            "Base.length",
            "Demo",
            "Demo.@twice",
            "Demo.Containers",
            "Demo.Containers.Nested",
            "Demo.Containers.Nested.score",
            "Demo.Impl",
            "Demo.Impl.InternalType",
            "Demo.Impl.Shared",
            "Demo.Impl.internal",
            "Demo.Impl.other",
            "Demo.LIMIT",
            "Demo.PUBLIC_VALUE",
            "Demo.PublicModule",
            "Demo.Shape",
            "Demo.fit",
            "Demo.qualified",
        ])
    );
    let discoverable = package.discoverable_items();
    for (id, item) in &package.items {
        let Some(ItemLanguageData::Julia(data)) = &item.language_data else {
            panic!("missing Julia data");
        };
        match &data.declaration {
            JuliaDeclaration::Callable {
                role: JuliaCallableRole::Method { family, .. },
                ..
            } => assert_eq!(
                discoverable.contains(id.as_str()),
                discoverable.contains(family.item.as_str()),
                "{}",
                item.qualified_name
            ),
            JuliaDeclaration::Field { .. } => {
                assert!(!item.discoverable());
                assert!(!discoverable.contains(id.as_str()));
            }
            _ => {}
        }
        if item.qualified_name == "Demo.helper" || item.qualified_name == "Demo.Hidden" {
            assert!(!item.discoverable());
        }
    }
}

#[test]
fn a_private_only_root_respects_package_visibility_without_losing_routes() {
    for visibility in ["public", "internal", "hidden"] {
        let root = support::TestWorkspace::from_fixture("julia-discovery");
        root.remove("docs/api.md");
        root.remove("docs/catalog.md");
        root.write(
            "src/Demo.jl",
            "module Demo\n\"A retained private helper.\"\nhelper(x) = x\nend\n",
        );
        root.write(
            "diplodocus.yaml",
            root.read("diplodocus.yaml").replace(
                "    ecosystem: \"julia\"",
                &format!("    visibility: \"{visibility}\"\n    ecosystem: \"julia\""),
            ),
        );
        let snapshot = snapshot(&root);
        assert_eq!(
            public_names(&snapshot.workspace().packages["demo"]),
            BTreeSet::from(["Demo"])
        );
        let fragments = render_fragments(&Site::new(&snapshot).unwrap()).unwrap();
        let index: FragmentIndex =
            serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
        let module = document(&index, "Demo");
        assert_eq!(module.visible, visibility != "hidden");
        assert!(!document(&index, "Demo.helper").visible);
        let search: Vec<serde_json::Value> =
            serde_json::from_slice(&fragments.files()["assets/search.json"].bytes).unwrap();
        assert_eq!(
            search
                .iter()
                .any(|entry| entry["title"] == "Demo" && entry["api"] == true),
            visibility != "hidden"
        );
        assert!(fragments.files().contains_key(&module.path));
    }
}

#[test]
fn detached_julia_indexes_agree_and_preserve_private_routes_and_method_anchors() {
    let root = support::TestWorkspace::from_fixture("julia-discovery");
    let original = snapshot(&root);
    let artifact = tempfile::tempdir().unwrap();
    let database = artifact.path().join("documentation.sqlite");
    original.publish(&database).unwrap();
    drop((original, root));

    let snapshot = Snapshot::load(database).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    assert_eq!(index.format_version, 2);
    let package = &snapshot.workspace().packages["demo"];
    let visible_names: BTreeSet<_> = index
        .documents
        .iter()
        .filter(|document| document.visible)
        .filter_map(|document| document.qualified_name.as_deref())
        .collect();
    assert_eq!(visible_names, public_names(package));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&fragments.files()["assets/search.json"].bytes).unwrap();
    assert_eq!(
        search
            .iter()
            .filter(|entry| entry["api"] == true)
            .map(|entry| entry["title"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        visible_names
    );
    assert_eq!(
        fragments.files()["assets/search.json"].bytes,
        standalone.files()["assets/search.json"].bytes
    );

    let overview = "packages/julia/index.html";
    let html = text(&standalone, overview);
    let navigation = html
        .split("aria-label=\"API reference\"")
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    let html_index = text(&standalone, "index.html");
    let markdown = text(&standalone, "index.md");
    let sitemap = text(&standalone, "sitemap.xml");
    for descriptor in index
        .documents
        .iter()
        .filter(|document| document.qualified_name.is_some())
    {
        let filename = descriptor.path.rsplit('/').next().unwrap();
        assert_eq!(
            navigation.contains(filename),
            descriptor.visible,
            "{}",
            descriptor.title
        );
        assert_eq!(
            html_index.contains(filename),
            descriptor.visible,
            "{}",
            descriptor.title
        );
        assert_eq!(
            markdown.contains(filename),
            descriptor.visible,
            "{}: {markdown}",
            descriptor.title
        );
        assert_eq!(
            sitemap.contains(&format!("https://example.test/docs/{}", descriptor.path)),
            descriptor.visible,
            "{}",
            descriptor.title
        );
        assert!(fragments.files().contains_key(&descriptor.path));
        assert!(fragments.files().contains_key(&descriptor.markdown_path));
        assert!(standalone.files().contains_key(&descriptor.path));
    }

    for (canonical, aliases) in [
        ("Demo.fit", &["Demo.local_fit"][..]),
        ("Demo.Impl.internal", &["Demo.train"][..]),
        ("Demo.Impl.other", &["Demo.public_train"][..]),
        ("Demo.Impl.InternalType", &["Demo.AliasedType"][..]),
        ("Demo.Impl.Shared", &["Demo.SharedModule"][..]),
        ("Base.length", &["Demo.shape_length"][..]),
    ] {
        let descriptor = document(&index, canonical);
        assert_eq!(descriptor.public_name.as_deref(), Some(canonical));
        let entry = search
            .iter()
            .find(|entry| entry["title"] == canonical)
            .unwrap();
        let lookup = entry["text"].as_str().unwrap();
        for name in std::iter::once(canonical).chain(aliases.iter().copied()) {
            assert!(lookup.contains(name), "{entry}");
        }
        assert_eq!(
            search
                .iter()
                .filter(|entry| entry["title"] == canonical)
                .count(),
            1
        );
    }

    let fit = document(&index, "Demo.fit");
    let fit_entry = search
        .iter()
        .find(|entry| entry["title"] == "Demo.fit")
        .unwrap();
    assert!(
        fit_entry["text"]
            .as_str()
            .unwrap()
            .contains("Fit integer inputs precisely.")
    );
    for (name, count) in [
        ("Demo.fit", 2),
        ("Demo.Shape", 2),
        ("Demo.Impl.InternalType", 1),
        ("Demo.helper", 1),
        ("Demo.PrivateType", 1),
    ] {
        let descriptor = document(&index, name);
        let mut methods = 0;
        for identity in &descriptor.identities {
            let DocumentIdentity::Item { item } = identity else {
                continue;
            };
            if package.items[&item.item].kind != ItemKind::Method {
                continue;
            }
            let anchor = format!(
                "method-{}",
                diplodocus::provenance::fingerprint_bytes(item.item.as_bytes()).value
            );
            assert!(descriptor.headings.iter().any(|(_, id, _)| id == &anchor));
            for path in [&descriptor.path, &descriptor.markdown_path] {
                assert!(text(&fragments, path).contains(&format!("id=\"{anchor}\"")));
            }
            methods += 1;
        }
        assert_eq!(methods, count, "{name}");
    }

    let urls = UrlMap {
        format_version: 1,
        documents: index
            .documents
            .iter()
            .enumerate()
            .map(|(i, document)| (document.path.clone(), format!("/host/{i}/")))
            .collect(),
        assets: Default::default(),
    };
    let mapped = render_fragments(
        &Site::new(&snapshot)
            .unwrap()
            .with_url_map(urls.clone())
            .unwrap(),
    )
    .unwrap();
    for path in [&fit.path, &fit.markdown_path] {
        let body = text(&mapped, path);
        let private = document(&index, "Demo.helper");
        assert!(!private.visible);
        assert!(body.contains(&urls.documents[&private.path]), "{body}");
        let shape = document(&index, "Demo.Shape");
        assert!(
            body.contains(&format!("{}#field-value", urls.documents[&shape.path])),
            "{body}"
        );
        assert!(
            body.contains(&format!("{}#method-", urls.documents[&fit.path])),
            "{body}"
        );
    }
}

#[test]
fn authored_private_selections_remain_visible_without_promoting_api_items() {
    let root = support::TestWorkspace::from_fixture("julia-discovery");
    let snapshot = snapshot(&root);
    let fragments = render_fragments(&Site::new(&snapshot).unwrap()).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let api = index
        .documents
        .iter()
        .find(|document| document.title == "Selected internals")
        .unwrap();
    let catalog = index
        .documents
        .iter()
        .find(|document| document.title == "Authored catalog")
        .unwrap();
    assert!(api.visible);
    assert!(catalog.visible);
    for path in [&api.path, &api.markdown_path] {
        let body = text(&fragments, path);
        for prose in [
            "Private implementation details.",
            "The stored value.",
            "A hidden secret selected by authored autodocs.",
        ] {
            assert!(body.contains(prose), "{body}");
        }
    }
    for name in ["Demo.helper", "Demo.Shape.value", "Demo.Hidden.secret"] {
        if name == "Demo.Shape.value" {
            assert!(
                !index
                    .documents
                    .iter()
                    .any(|document| document.qualified_name.as_deref() == Some(name))
            );
        } else {
            assert!(!document(&index, name).visible);
        }
        for path in [&catalog.path, &catalog.markdown_path] {
            assert!(text(&fragments, path).contains(name));
        }
    }
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&fragments.files()["assets/search.json"].bytes).unwrap();
    let entry = search
        .iter()
        .find(|entry| entry["title"] == "Selected internals")
        .unwrap();
    assert_eq!(entry["api"], false);
    assert!(entry["text"].as_str().unwrap().contains("Demo.helper"));
}

#[test]
fn detached_julia_fields_share_type_pages_anchors_and_mapped_links() {
    let root = support::TestWorkspace::from_fixture("julia-discovery");
    root.remove("docs/api.md");
    root.remove("docs/catalog.md");
    root.write(
        "src/Demo.jl",
        r#"module Demo
export Shape, Other, Empty

"A shape with [stored data](@ref Demo.Shape.value)."
mutable struct Shape
    """
    The first field stores observations.

    ## Details

    See [stored data](@ref Demo.Shape.value), [other data](@ref Demo.Other.value), and [details](#details).
    """
    zeta::Vector{Float64}
    "The stored value."
    value::Int
    untyped
    Shape(zeta, value, untyped) = new(zeta, value, untyped)
end

struct Other
    "Another stored value."
    value::Float64
end

abstract type Empty end

"A documented private type."
struct Private
    "A private field."
    value::Int
end
end
"#,
    );
    let original = snapshot(&root);
    let artifact = tempfile::tempdir().unwrap();
    let database = artifact.path().join("documentation.sqlite");
    original.publish(&database).unwrap();
    drop((original, root));

    let snapshot = Snapshot::load(database).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let package = &snapshot.workspace().packages["demo"];
    let shape = document(&index, "Demo.Shape");
    let other = document(&index, "Demo.Other");
    let private = document(&index, "Demo.Private");
    assert!(shape.visible);
    assert!(!private.visible);
    for (owner, name, anchor) in [
        (shape, "Demo.Shape.zeta", "field-zeta"),
        (shape, "Demo.Shape.value", "field-value"),
        (shape, "Demo.Shape.untyped", "field-untyped"),
        (other, "Demo.Other.value", "field-value"),
        (private, "Demo.Private.value", "field-value"),
    ] {
        let (id, _) = package
            .items
            .iter()
            .find(|(_, item)| item.qualified_name == name)
            .unwrap();
        let identity = DocumentIdentity::Item {
            item: diplodocus::ir::ItemReference {
                package: "demo".into(),
                item: id.clone(),
            },
        };
        assert!(owner.identities.contains(&identity));
        assert!(
            owner
                .headings
                .iter()
                .any(|(level, id, _)| *level == 3 && id == anchor)
        );
        assert!(
            !index
                .documents
                .iter()
                .any(|document| document.qualified_name.as_deref() == Some(name))
        );
        let old_path = format!(
            "packages/julia/reference/{}.html",
            diplodocus::provenance::fingerprint_bytes(id.as_bytes()).value
        );
        assert!(!standalone.files().contains_key(&old_path));
        for rendered in [&fragments, &standalone] {
            for path in [&owner.path, &owner.markdown_path] {
                assert!(text(rendered, path).contains(&format!("id=\"{anchor}\"")));
            }
        }
    }
    let headings: Vec<_> = shape
        .headings
        .iter()
        .map(|(_, id, title)| (id.as_str(), title.as_str()))
        .collect();
    assert!(headings.contains(&("field-value", "value::Int")));
    assert!(
        headings.contains(&("field-zeta-details", "Details")),
        "{headings:?}"
    );
    for path in [&shape.path, &shape.markdown_path] {
        let body = text(&fragments, path);
        assert!(body.find("id=\"field-zeta\"").unwrap() < body.find("id=\"field-value\"").unwrap());
        assert!(
            body.find("id=\"field-value\"").unwrap() < body.find("id=\"field-untyped\"").unwrap()
        );
        assert!(body.contains("The stored value."));
        assert!(body.contains("id=\"methods\""));
        assert!(body.contains("#field-zeta-details"));
    }
    let empty = document(&index, "Demo.Empty");
    assert!(!text(&fragments, &empty.path).contains("id=\"fields\""));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&fragments.files()["assets/search.json"].bytes).unwrap();
    let entry = search
        .iter()
        .find(|entry| entry["title"] == "Demo.Shape")
        .unwrap();
    assert!(
        entry["text"]
            .as_str()
            .unwrap()
            .contains("The first field stores")
    );
    assert!(
        entry["text"]
            .as_str()
            .unwrap()
            .contains("Demo.Shape.untyped")
    );
    assert!(
        !search
            .iter()
            .any(|entry| entry["title"] == "Demo.Shape.value")
    );
    let navigation = text(&standalone, &shape.path)
        .split("aria-label=\"API reference\"")
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    assert!(!navigation.contains("#field-"));

    let urls = UrlMap {
        format_version: 1,
        documents: index
            .documents
            .iter()
            .enumerate()
            .map(|(i, document)| (document.path.clone(), format!("/host/{i}/")))
            .collect(),
        assets: Default::default(),
    };
    let mapped = render_fragments(
        &Site::new(&snapshot)
            .unwrap()
            .with_url_map(urls.clone())
            .unwrap(),
    )
    .unwrap();
    for path in [&shape.path, &shape.markdown_path] {
        let body = text(&mapped, path);
        assert!(
            body.contains(&format!("{}#field-value", urls.documents[&shape.path])),
            "{body}"
        );
        assert!(
            body.contains(&format!("{}#field-value", urls.documents[&other.path])),
            "{body}"
        );
    }
}
