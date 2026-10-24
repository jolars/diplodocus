use serde_json::{Value, json};

use super::*;

fn fixture(offset: u32) -> Vec<u8> {
    let id = |id: u32| id + offset;
    let generics = json!({"params":[], "where_predicates":[]});
    let mut items = BTreeMap::new();
    let mut put = |number, name, visibility: &str, inner| {
        let key = id(number);
        items.insert(key.to_string(), json!({"id":key,"crate_id":0,"name":name,"span":null,"visibility":visibility,"docs":null,"links":{},"attrs":[],"deprecation":null,"inner":inner}));
    };
    put(
        0,
        "sample",
        "public",
        json!({"module":{"is_crate":true,"items":[id(1),id(5),id(6),id(15),id(16)],"is_stripped":false}}),
    );
    put(
        1,
        "private",
        "default",
        json!({"module":{"is_crate":false,"items":[id(2),id(3),id(9)],"is_stripped":false}}),
    );
    put(
        2,
        "Widget",
        "public",
        json!({"struct":{"kind":{"plain":{"fields":[id(8)],"has_stripped_fields":false}},"generics":{"params":[{"name":"T","kind":{"type":{"bounds":[],"default":{"primitive":"usize"},"is_synthetic":false}}}],"where_predicates":[]},"impls":[id(4),id(13)]}}),
    );
    put(
        3,
        "Render",
        "public",
        json!({"trait":{"is_auto":false,"is_unsafe":false,"is_dyn_compatible":true,"items":[id(10)],"generics":generics,"bounds":[],"implementations":[id(13)]}}),
    );
    put(
        4,
        "",
        "default",
        json!({"impl":{"is_unsafe":false,"generics":generics,"provided_trait_methods":[],"trait":null,"for":{"resolved_path":{"path":"Widget","id":id(2),"args":null}},"items":[id(7)],"is_negative":false,"is_synthetic":false,"blanket_impl":null}}),
    );
    put(
        5,
        "Widget",
        "public",
        json!({"use":{"source":"private::Widget","name":"Widget","id":id(2),"is_glob":false}}),
    );
    put(
        6,
        "Other",
        "public",
        json!({"use":{"source":"private::Widget","name":"Other","id":id(2),"is_glob":false}}),
    );
    let function = json!({"function":{"sig":{"inputs":[["self",{"borrowed_ref":{"lifetime":"'a","is_mutable":false,"type":{"generic":"Self"}}}]],"output":{"primitive":"usize"},"is_c_variadic":false},"generics":{"params":[{"name":"'a","kind":{"lifetime":{"outlives":[]}}}],"where_predicates":[]},"header":{"is_const":false,"is_unsafe":false,"is_async":false,"abi":"Rust"},"has_body":true}});
    put(7, "call", "public", function.clone());
    put(
        8,
        "value",
        "public",
        json!({"struct_field":{"generic":"T"}}),
    );
    put(
        9,
        "Invisible",
        "public",
        json!({"struct":{"kind":"unit","generics":generics,"impls":[]}}),
    );
    put(10, "call", "default", function.clone());
    put(
        13,
        "",
        "default",
        json!({"impl":{"is_unsafe":false,"generics":generics,"provided_trait_methods":[],"trait":{"path":"Render","id":id(3),"args":null},"for":{"resolved_path":{"path":"Widget","id":id(2),"args":null}},"items":[id(14)],"is_negative":false,"is_synthetic":false,"blanket_impl":null}}),
    );
    put(14, "call", "default", function);
    put(
        15,
        "Render",
        "public",
        json!({"use":{"source":"private::Render","name":"Render","id":id(3),"is_glob":false}}),
    );
    put(
        16,
        "Choice",
        "public",
        json!({"enum":{"generics":generics,"has_stripped_variants":false,"variants":[id(17)],"impls":[]}}),
    );
    put(
        17,
        "Named",
        "default",
        json!({"variant":{"kind":{"struct":{"fields":[id(18)],"has_stripped_fields":false}},"discriminant":null}}),
    );
    put(
        18,
        "number",
        "default",
        json!({"struct_field":{"primitive":"usize"}}),
    );
    items.get_mut(&id(4).to_string()).unwrap()["name"] = Value::Null;
    items.get_mut(&id(13).to_string()).unwrap()["name"] = Value::Null;
    items.get_mut(&id(13).to_string()).unwrap()["docs"] = json!("Implementation documentation.");
    items.get_mut(&id(2).to_string()).unwrap()["docs"] = json!(
        "See [`Self`], [Render], and [the trait](crate::private::Render).\n\n```rust,no_run\n# let hidden = 1;\nassert_eq!(hidden, 1);\n```"
    );
    items.get_mut(&id(2).to_string()).unwrap()["links"] =
        json!({"Self":id(2),"Render":id(3),"crate::private::Render":id(3)});
    serde_json::to_vec(&json!({"root":id(0),"crate_version":"1.2.3","includes_private":true,"index":items,"paths":{},"external_crates":{},"target":{"triple":"x86_64-unknown-linux-gnu","target_features":[]},"format_version":61})).unwrap()
}

fn extract(bytes: &[u8]) -> RustExtraction {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = ResolvedRepositoryPaths {
        id: "repo".into(),
        path: root.clone(),
    };
    let target = ResolvedTargetPath {
        id: "api".into(),
        path: root.join("lib.rs"),
    };
    let package = ResolvedPackagePaths {
        id: "rust-sample".into(),
        repository_index: 0,
        path: root.clone(),
        metadata_path: root.join("Cargo.toml"),
        overview: None,
        targets: vec![target.clone()],
    };
    extract_rustdoc(&repository, &package, &target, bytes)
}

fn data(item: &Item) -> &RustItemData {
    let Some(ItemLanguageData::Rust(data)) = &item.language_data else {
        panic!("Rust item")
    };
    data
}

#[test]
fn reconciles_private_reexports_and_keeps_distinct_implementation_members() {
    let result = extract(&fixture(0));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.version.as_deref(), Some("1.2.3"));
    assert!(
        !result
            .items
            .values()
            .any(|item| item.name == "Invisible" || item.name == "private")
    );
    let (id, widget) = result
        .items
        .iter()
        .find(|(_, item)| item.name == "Widget")
        .unwrap();
    assert_eq!(widget.qualified_name, "sample::Other");
    assert!(
        widget
            .aliases
            .iter()
            .any(|alias| alias.qualified_name == "sample::Widget")
    );
    assert!(id.contains("sample%3A%3Aprivate%3A%3AWidget"));
    assert_eq!(data(widget).implementations.len(), 2);
    let methods: Vec<_> = result
        .items
        .iter()
        .filter(|(_, item)| {
            item.name == "call"
                && data(item)
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.item == *id)
        })
        .collect();
    assert_eq!(methods.len(), 2);
    assert_ne!(methods[0].0, methods[1].0);
    assert_eq!(methods[0].1.qualified_name, methods[1].1.qualified_name);
    assert!(
        data(widget)
            .implementations
            .iter()
            .all(|implementation| implementation.members.len() == 1)
    );
    let implementation = data(widget)
        .implementations
        .iter()
        .find(|implementation| implementation.documentation_item.is_some())
        .unwrap();
    let documentation = &result.items[&implementation.documentation_item.as_ref().unwrap().item];
    assert_eq!(
        data(documentation).declaration,
        RustDeclarationKind::Implementation
    );
    assert!(documentation.documentation.is_some());
    let named = result
        .items
        .values()
        .find(|item| item.name == "Named")
        .unwrap();
    assert_eq!(data(named).declaration, RustDeclarationKind::Variant);
    assert!(
        named
            .children
            .iter()
            .any(|id| result.items[id].name == "number")
    );
}

#[test]
fn compiler_numeric_identifiers_do_not_enter_portable_items() {
    let a = extract(&fixture(0));
    let b = extract(&fixture(1000));
    assert_eq!(a, b);
}

#[test]
fn preserves_generics_receivers_and_compiler_link_targets() {
    let result = extract(&fixture(0));
    let widget = result
        .items
        .values()
        .find(|item| item.name == "Widget")
        .unwrap();
    let Signature::LanguageSpecific { syntax, .. } = &widget.signatures[0].signature else {
        panic!("structured Rust signature")
    };
    assert_eq!(signatures::spelling(syntax), "pub struct Widget<T = usize>");
    let method = result
        .items
        .values()
        .find(|item| {
            item.name == "call"
                && data(item).implementation.as_deref() == Some("impl sample::private::Widget")
        })
        .unwrap();
    let Signature::LanguageSpecific { syntax, .. } = &method.signatures[0].signature else {
        panic!("structured Rust signature")
    };
    assert_eq!(
        signatures::spelling(syntax),
        "pub fn call<'a>(&'a self) -> usize"
    );
    let document = &widget.documentation.as_ref().unwrap().document;
    let Block::Paragraph { inlines, .. } = &document.blocks[0] else {
        panic!("paragraph")
    };
    let links: Vec<_> = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(links.len(), 3, "{inlines:?}");
    assert!(
        links
            .iter()
            .all(|target| target.starts_with("@ref rust-sample::sid1:rust:"))
    );
    assert_eq!(links[1], links[2]);
    assert_eq!(
        document.span.end,
        widget
            .documentation
            .as_ref()
            .unwrap()
            .raw_source
            .as_ref()
            .unwrap()
            .len()
    );
    let Block::CodeBlock {
        source,
        language,
        source_segments,
        ..
    } = &document.blocks[1]
    else {
        panic!("code block")
    };
    assert_eq!(language.as_deref(), Some("rust"));
    assert_eq!(source, "assert_eq!(hidden, 1);\n", "{source_segments:?}");
    assert_eq!(
        *source,
        source_segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
    );
    let raw = widget
        .documentation
        .as_ref()
        .unwrap()
        .raw_source
        .as_ref()
        .unwrap();
    for segment in source_segments {
        assert_eq!(segment.text, raw[segment.span.start..segment.span.end]);
    }
}

#[test]
fn merges_split_inherent_blocks_without_losing_members_or_prose() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    fixture["index"]["2"]["inner"]["struct"]["impls"]
        .as_array_mut()
        .unwrap()
        .push(json!(20));
    let mut implementation = fixture["index"]["4"].clone();
    implementation["id"] = json!(20);
    implementation["docs"] = json!("Second block.\n\n## Details\n\n[Details](#details)");
    implementation["inner"]["impl"]["items"] = json!([21]);
    fixture["index"]["20"] = implementation;
    fixture["index"]["4"]["docs"] = json!("First block.");
    let mut method = fixture["index"]["7"].clone();
    method["id"] = json!(21);
    method["name"] = json!("other");
    fixture["index"]["21"] = method;
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let widget = result
        .items
        .values()
        .find(|item| item.name == "Widget")
        .unwrap();
    let implementation = data(widget)
        .implementations
        .iter()
        .find(|implementation| implementation.trait_name.is_none())
        .unwrap();
    assert_eq!(implementation.members.len(), 2);
    let item = &result.items[&implementation.documentation_item.as_ref().unwrap().item];
    let docs = item.documentation.as_ref().unwrap();
    assert!(
        docs.raw_source
            .as_ref()
            .unwrap()
            .contains("First block.\n\nSecond block.")
    );
    assert!(docs.source_location.is_none());
    let Block::Heading { attributes, .. } = &docs.document.blocks[2] else {
        panic!("heading")
    };
    assert_eq!(attributes.identifier.as_ref().unwrap().value, "details");
}

#[test]
fn hidden_export_targets_do_not_enter_the_public_graph() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    fixture["index"]["2"]["attrs"] = json!([{"other":"#[doc(hidden)]"}]);
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(!result.items.values().any(|item| item.name == "Widget"));
    assert!(!result.items.values().any(|item| item.name == "value"));
}

#[test]
fn dependency_links_use_external_docs_without_importing_dependency_items() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    fixture["index"]["0"]["docs"] = json!("See [`Vec`].");
    fixture["index"]["0"]["links"] = json!({"Vec":900});
    fixture["paths"]["900"] = json!({"crate_id":1,"path":["alloc","vec","Vec"],"kind":"struct"});
    fixture["external_crates"]["1"] = json!({"name":"alloc","html_root_url":"https://doc.rust-lang.org/alloc/","path":"liballoc.rlib"});
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(!result.items.values().any(|item| item.name == "Vec"));
    let root = result
        .items
        .values()
        .find(|item| item.name == "sample")
        .unwrap();
    let Block::Paragraph { inlines, .. } = &root.documentation.as_ref().unwrap().document.blocks[0]
    else {
        panic!("paragraph")
    };
    assert!(inlines.iter().any(|inline| matches!(inline, Inline::Link { target, .. } if target == "https://doc.rust-lang.org/alloc/vec/struct.Vec.html")));
}

#[test]
fn external_members_link_to_their_owning_type_page() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    fixture["index"]["0"]["docs"] = json!("See [`None`], [`f64::MAX`], and [`str::parse`].");
    fixture["index"]["0"]["links"] = json!({"`None`":901,"`f64::MAX`":903,"`str::parse`":905});
    fixture["paths"]["900"] = json!({"crate_id":1,"path":["core","option","Option"],"kind":"enum"});
    fixture["paths"]["901"] =
        json!({"crate_id":1,"path":["core","option","Option","None"],"kind":"variant"});
    fixture["paths"]["902"] = json!({"crate_id":2,"path":["std","f64"],"kind":"primitive"});
    fixture["paths"]["903"] = json!({"crate_id":1,"path":["std","f64","MAX"],"kind":"assoc_const"});
    fixture["paths"]["904"] = json!({"crate_id":2,"path":["std","str"],"kind":"primitive"});
    fixture["paths"]["905"] = json!({"crate_id":1,"path":["std","str","parse"],"kind":"function"});
    fixture["external_crates"]["1"] =
        json!({"name":"core","html_root_url":null,"path":"libcore.rlib"});
    fixture["external_crates"]["2"] =
        json!({"name":"std","html_root_url":null,"path":"libstd.rlib"});
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let root = result
        .items
        .values()
        .find(|item| item.name == "sample")
        .unwrap();
    let Block::Paragraph { inlines, .. } = &root.documentation.as_ref().unwrap().document.blocks[0]
    else {
        panic!("paragraph")
    };
    let links: Vec<_> = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        links,
        [
            "https://doc.rust-lang.org/core/option/enum.Option.html#variant.None",
            "https://doc.rust-lang.org/std/primitive.f64.html#associatedconstant.MAX",
            "https://doc.rust-lang.org/std/primitive.str.html#method.parse"
        ]
    );
}

#[test]
fn rejects_unknown_json_before_interpreting_its_graph() {
    let result = extract(br#"{"format_version":62}"#);
    assert!(result.items.is_empty());
    assert_eq!(result.diagnostics[0].severity, Severity::Error);
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::RustExtraction);
}

#[test]
fn compiler_character_columns_become_exact_utf8_byte_ranges() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let path = root.join("lib.rs");
    let text = "\tpub fn héllo() {}\n";
    fs::write(&path, text).unwrap();
    let repository = ResolvedRepositoryPaths {
        id: "repo".into(),
        path: root.clone(),
    };
    let target = ResolvedTargetPath {
        id: "api".into(),
        path: path.clone(),
    };
    let package = ResolvedPackagePaths {
        id: "rust-sample".into(),
        repository_index: 0,
        path: root.clone(),
        metadata_path: root.join("Cargo.toml"),
        overview: None,
        targets: vec![target.clone()],
    };
    let krate: rd::Crate = serde_json::from_slice(&fixture(0)).unwrap();
    let mut function = krate.index[&rd::Id(7)].clone();
    function.span = Some(rd::Span {
        filename: path,
        begin: (1, 2),
        end: (1, 19),
    });
    let source = source_location(&repository, &package, &target, &function).unwrap();
    let span = source.span.unwrap();
    assert_eq!(&text[span.start..span.end], "pub fn héllo() {}");
    assert_eq!(span.start, 1);
    assert_eq!(span.end, 19);
    function.span = None;
    assert!(source_location(&repository, &package, &target, &function).is_none());
}

#[test]
fn macro_hygiene_paths_and_ambiguous_bound_syntax_are_normalized() {
    let mut krate: rd::Crate = serde_json::from_slice(&fixture(0)).unwrap();
    krate.paths.insert(
        rd::Id(900),
        rd::ItemSummary {
            crate_id: 1,
            path: vec!["core".into(), "fmt".into(), "Formatter".into()],
            kind: rd::ItemKind::Struct,
        },
    );
    let references = BTreeMap::new();
    let signatures = Signatures {
        krate: &krate,
        references: &references,
    };
    let path = rd::Path {
        path: "$crate::fmt::Formatter".into(),
        id: rd::Id(900),
        args: None,
    };
    assert_eq!(
        signatures::spelling(&signatures.path(&path)),
        "core::fmt::Formatter"
    );
    let bound = |name: &str| rd::PolyTrait {
        trait_: rd::Path {
            path: name.into(),
            id: rd::Id(3),
            args: None,
        },
        generic_params: vec![],
    };
    let type_ = rd::Type::BorrowedRef {
        lifetime: None,
        is_mutable: false,
        type_: Box::new(rd::Type::DynTrait(rd::DynTrait {
            traits: vec![bound("A"), bound("B")],
            lifetime: None,
        })),
    };
    assert_eq!(
        signatures::spelling(&signatures.type_(&type_)),
        "&(dyn A + B)"
    );
}

#[test]
fn blanket_implementation_headers_retain_the_generic_receiver() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    let implementation = &mut fixture["index"]["13"]["inner"]["impl"];
    implementation["blanket_impl"] = json!({"generic":"T"});
    implementation["generics"]["params"] =
        json!([{"name":"T","kind":{"type":{"bounds":[],"default":null,"is_synthetic":false}}}]);
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let widget = result
        .items
        .values()
        .find(|item| item.name == "Widget")
        .unwrap();
    let implementation = data(widget)
        .implementations
        .iter()
        .find(|implementation| implementation.is_blanket)
        .unwrap();
    assert_eq!(
        signatures::spelling(&implementation.signature),
        "impl<T> Render for T"
    );
    assert_eq!(
        implementation.identity,
        "impl<T> sample::private::Render for T"
    );
}

#[test]
fn trait_implementation_summaries_link_public_local_types_without_claiming_members() {
    let mut fixture: Value = serde_json::from_slice(&fixture(0)).unwrap();
    let mut private_impl = fixture["index"]["13"].clone();
    private_impl["id"] = json!(30);
    private_impl["inner"]["impl"]["for"] =
        json!({"resolved_path":{"path":"Invisible","id":9,"args":null}});
    private_impl["inner"]["impl"]["items"] = json!([]);
    fixture["index"]["30"] = private_impl;
    let mut hidden_impl = fixture["index"]["13"].clone();
    hidden_impl["id"] = json!(31);
    hidden_impl["attrs"] = json!([{"other":"#[doc(hidden)]"}]);
    fixture["index"]["31"] = hidden_impl;
    fixture["index"]["3"]["inner"]["trait"]["implementations"] = json!([13, 30, 31]);
    let result = extract(&serde_json::to_vec(&fixture).unwrap());
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let (trait_id, trait_) = result
        .items
        .iter()
        .find(|(_, item)| item.name == "Render")
        .unwrap();
    let (widget_id, widget) = result
        .items
        .iter()
        .find(|(_, item)| item.name == "Widget")
        .unwrap();
    let implementations = &data(trait_).implementations;
    assert_eq!(implementations.len(), 1);
    let implementation = &implementations[0];
    assert_eq!(
        signatures::spelling(&implementation.signature),
        "impl Render for Widget"
    );
    assert_eq!(
        implementation.trait_target.as_ref().unwrap().item,
        *trait_id
    );
    assert!(implementation.members.is_empty());
    assert!(implementation.documentation_item.is_none());
    assert!(
        serde_json::to_string(&implementation.signature)
            .unwrap()
            .contains(widget_id)
    );
    let concrete = data(widget)
        .implementations
        .iter()
        .find(|implementation| {
            implementation
                .trait_target
                .as_ref()
                .is_some_and(|reference| reference.item == *trait_id)
        })
        .unwrap();
    assert!(!concrete.members.is_empty());
    assert!(concrete.documentation_item.is_some());
}
