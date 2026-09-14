use diplodocus::documents::{AuthoredFormat, parse_authored_document};
use diplodocus::ir::{
    Block, CellOutput, OutputRepresentation, ParameterKind, SchemaVersion, Signature,
    SignatureExpression, SourceLocation, WORKSPACE_SCHEMA_VERSION, Workspace,
};
use serde_json::json;

mod support;

#[test]
fn schema_version_is_explicit_and_rejects_unknown_versions() {
    let workspace = Workspace::default();
    let mut value = serde_json::to_value(&workspace).unwrap();
    assert_eq!(value["schema_version"], WORKSPACE_SCHEMA_VERSION);
    assert_eq!(
        serde_json::from_value::<Workspace>(value.clone()).unwrap(),
        workspace
    );
    value["schema_version"] = json!(2);
    assert!(serde_json::from_value::<Workspace>(value.clone()).is_err());
    value.as_object_mut().unwrap().remove("schema_version");
    assert!(serde_json::from_value::<Workspace>(value).is_err());
    assert_eq!(serde_json::to_string(&SchemaVersion).unwrap(), "1");
    for invalid in [json!(0), json!(-1), json!("1"), json!(null)] {
        assert!(serde_json::from_value::<SchemaVersion>(invalid).is_err());
    }
}

fn sample() -> serde_json::Value {
    let mut document = parse_authored_document(
        "# Guide\n\n```{python}\nprint('hello')\n```\n",
        AuthoredFormat::Qmd,
    )
    .document;
    let Block::CodeCell(cell) = &mut document.blocks[1] else {
        panic!("cell")
    };
    let execution = json!({
        "activity": {
            "kind": "execution", "mode": "execute", "engine": "jupyter",
            "kernel": {"name": "python3", "language": "python", "language_version": "3.13", "version": "6.0"},
            "origin": "cache",
            "declared_environment_inputs": [{
                "source": {"repository": "core", "path": "uv.lock", "span": null},
                "fingerprint": {"algorithm": "sha256", "value": "environment-digest"}
            }]
        },
        "source": {"kind": "repository", "repository": "core", "path": "docs/guide.qmd"},
        "span": {"start": 9, "end": 40}, "tools": {"jupyter-engine": "0.1.0", "jupyter-protocol": "2.0.2", "python": "3.13"}
    });
    cell.outputs = serde_json::from_value(json!([
        {"kind": {"type": "stream", "stream": "stdout"},
         "representations": [{"kind": "plain-text", "media_type": "text/plain", "text": "# plain stdout\n<script>literal</script>"}],
         "provenance": [execution]},
        {"kind": {"type": "display"}, "representations": [
            {"kind": "markdown-blocks", "media_type": "text/markdown", "blocks": [{"type": "paragraph", "inlines": [{"type": "text", "value": "Result", "span": {"start": 0, "end": 6}}], "span": {"start": 0, "end": 6}}]},
            {"kind": "asset", "media_type": "image/png", "asset": {"path": "assets/figure-digest.png", "fingerprint": {"algorithm": "sha256", "value": "figure-digest"}}},
            {"kind": "sanitized-html", "media_type": "text/html", "html": "<b>Result</b>", "policy": "html-v1", "sanitizer": {"name": "html-sanitizer", "version": "1.0"}}
        ], "provenance": []},
        {"kind": {"type": "error", "name": "ValueError", "message": "invalid input", "traceback": ["ValueError: invalid input"]}, "representations": [], "provenance": []}
    ])).unwrap();
    let packages = json!({"pyfoo": {
        "slug": "python", "name": "Foo for Python", "ecosystem": "python", "version": "2.1.0",
        "repository": "core", "path": null, "metadata_path": "pyproject.toml",
        "kind": "package", "visibility": "public",
        "extraction_targets": {"api": {"extractor": "python", "path": "src", "role": "public-api"}},
        "items": {"foo.fit": {
            "kind": "function", "name": "fit", "qualified_name": "foo.fit",
            "signatures": [{"signature": {"kind": "callable", "parameters": [
                {"name": "x", "kind": {"kind": "positional-only"},
                 "annotation": {"kind": "apply", "constructor": {"kind": "name", "name": "list", "target": null}, "arguments": [{"kind": "name", "name": "float", "target": null}]},
                 "default": null},
                {"name": "strict", "kind": {"kind": "keyword-only"},
                 "annotation": {"kind": "name", "name": "bool", "target": null},
                 "default": {"kind": "literal", "text": "True"}}
            ], "returns": {"kind": "name", "name": "Model", "target": {"package": "pyfoo", "item": "foo.Model"}}}, "sources": [{"source": {"repository": "core", "path": "src/foo.pyi", "span": {"start": 0, "end": 70}}, "role": "signature", "parsers": ["ruff"]}]}],
            "documentation": {"document": {"span": {"start": 0, "end": 0}, "frontmatter": null, "blocks": []}, "source_format": {"kind": "extracted", "name": "numpy-docstring"}, "source_location": {"repository": "core", "path": "src/foo.py", "span": {"start": 20, "end": 50}}, "raw_source": "Fit a model.", "provenance": []},
            "source_location": {"repository": "core", "path": "src/foo.py", "span": {"start": 0, "end": 100}},
            "children": [], "provenance": [{"activity": {
                "kind": "extraction", "target": {"package": "pyfoo", "target": "api"}, "mode": "static",
                "capabilities": ["python.signatures"],
                "parsers": {"ruff": {"version": "0.0.12", "role": "python-syntax", "settings": {"mode": "stub"}}},
                "inputs": {"core": {"src/foo.pyi": {"kind": "python-stub", "fingerprint": {"algorithm": "sha256", "value": "stub-digest"}, "parsers": ["ruff"]}}}
            }, "source": null, "span": null, "tools": {"python-extractor": "0.1.0", "ruff": "0.0.12"}}]
        }, "foo.Model": {
            "kind": "class", "name": "Model", "qualified_name": "foo.Model", "signatures": [], "documentation": null, "source_location": null, "children": [], "provenance": []
        }}
    }});
    json!({
        "schema_version": 1, "name": "Foo",
        "repositories": {"core": {
            "canonical_url": "https://example.com/foo.git",
            "source_link_template": "https://example.com/foo/blob/{revision}/{path}#L{line}",
            "revision": "abc123", "dirty": false,
            "declared_input_fingerprint": {"algorithm": "sha256", "value": "repository-digest"}
        }},
        "packages": packages,
        "content_collections": {"guide": {
            "owner": {"kind": "project"}, "repository": "core", "path": "docs", "mount": "guide", "format": "qmd",
            "execution": {"mode": "execute", "engine": "jupyter", "kernel": "python3", "declared_environment_inputs": ["uv.lock"]}
        }},
        "pages": {"guide/index": {
            "owner": {"kind": "project"}, "kind": {"kind": "authored", "collection": "guide"}, "title": "Guide",
            "document": {"document": document, "source_format": {"kind": "authored", "format": "qmd"}, "source_location": {"repository": "core", "path": "docs/guide.qmd", "span": null}, "raw_source": null, "provenance": []}
        }},
        "concepts": {"fitting": {"kind": "equivalent", "members": [{"package": "pyfoo", "item": "foo.fit"}], "documentation": null}},
        "relationships": [{
            "from": {"kind": "workspace", "package": "pyfoo"},
            "to": {"kind": "external", "ecosystem": "rust", "name": "foo-core"},
            "kind": "binds", "version_constraint": "^1.9",
            "provenance": [{"activity": {"kind": "declaration"}, "source": {"kind": "configuration", "path": "diplodocus.toml"}, "span": {"start": 10, "end": 30}, "tools": {}}]
        }],
        "diagnostics": [{"code": "unsupported-authored-syntax", "severity": "warning", "message": "Unsupported source construct.", "related_entity": {"kind": "item", "package": "pyfoo", "id": "foo.fit"}, "source": {"kind": "repository", "repository": "core", "path": "src/foo.py"}, "span": {"start": 10, "end": 20}}],
        "provenance": [{"activity": {"kind": "declaration"}, "source": null, "span": null, "tools": {"diplodocus": "0.1.0", "panache": "0.29.0"}}]
    })
}

#[test]
fn all_workspace_entities_round_trip_with_a_stable_golden() {
    let value = sample();
    let workspace: Workspace = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&workspace).unwrap(), value);
    support::assert_json_golden(&workspace, "ir/workspace.json");
    let item = &workspace.packages["pyfoo"].items["foo.fit"];
    let signature = &item.signatures[0].signature;
    assert_eq!(
        item.signatures[0].sources[0].source.path.as_str(),
        "src/foo.pyi"
    );
    assert_eq!(
        item.documentation
            .as_ref()
            .unwrap()
            .source_location
            .as_ref()
            .unwrap()
            .path
            .as_str(),
        "src/foo.py"
    );
    let Signature::Callable {
        parameters,
        returns,
    } = signature
    else {
        panic!("callable")
    };
    assert_eq!(parameters[0].kind, ParameterKind::PositionalOnly);
    assert!(matches!(
        parameters[0].annotation,
        Some(SignatureExpression::Apply { .. })
    ));
    assert!(matches!(
        returns,
        Some(SignatureExpression::Name {
            target: Some(_),
            ..
        })
    ));
    let Block::CodeCell(cell) = &workspace.pages["guide/index"].document.document.blocks[1] else {
        panic!("cell")
    };
    assert!(
        matches!(&cell.outputs[0].representations[0], OutputRepresentation::PlainText { text, .. } if text.contains("<script>"))
    );
}

#[test]
fn deserialized_html_is_only_an_unvalidated_candidate() {
    let value = json!({
        "kind": "sanitized-html", "media_type": "text/html",
        "html": "<script>untrusted()</script>", "policy": "forged-policy",
        "sanitizer": {"name": "claimed-sanitizer", "version": "1"}
    });
    let representation: OutputRepresentation = serde_json::from_value(value.clone()).unwrap();
    let OutputRepresentation::HtmlCandidate { html, .. } = &representation else {
        panic!("unvalidated candidate")
    };
    assert_eq!(html.as_untrusted_str(), "<script>untrusted()</script>");
    assert_eq!(serde_json::to_value(representation).unwrap(), value);
}

#[test]
fn unordered_collections_serialize_in_canonical_order() {
    let mut value = sample();
    value["repositories"]["aaa"] = value["repositories"]["core"].clone();
    let mut diagnostic = value["diagnostics"][0].clone();
    diagnostic["span"]["start"] = json!(0);
    value["diagnostics"]
        .as_array_mut()
        .unwrap()
        .push(diagnostic);
    let first: Workspace = serde_json::from_value(value.clone()).unwrap();
    value["diagnostics"].as_array_mut().unwrap().reverse();
    let mut second: Workspace = serde_json::from_value(value).unwrap();
    second.repositories = first.repositories.clone().into_iter().rev().collect();
    second.packages.get_mut("pyfoo").unwrap().items = first.packages["pyfoo"]
        .items
        .clone()
        .into_iter()
        .rev()
        .collect();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    assert_eq!(first.diagnostics.first().unwrap().span.unwrap().start, 0);
}

#[test]
fn paths_reject_checkout_paths_and_retain_explicit_relative_roots() {
    for path in [
        "/home/user/checkout/file.py",
        "C:/checkout/file.py",
        "a\\b",
        "../file.py",
        "a/../b",
        "a//b",
        "",
        ".",
    ] {
        let value = json!({"repository": "core", "path": path, "span": null});
        assert!(
            serde_json::from_value::<SourceLocation>(value).is_err(),
            "{path}"
        );
        let mut value = sample();
        value["packages"]["pyfoo"]["path"] = json!(path);
        assert!(
            serde_json::from_value::<Workspace>(value).is_err(),
            "{path}"
        );
    }
    let mut value = sample();
    value["repositories"]["core"]["checkout_path"] = json!("/home/user/checkout");
    assert!(serde_json::from_value::<Workspace>(value).is_err());
    let mut value = sample();
    value["content_collections"]["guide"]["execution"]["declared_environment_inputs"] =
        json!(["/tmp/uv.lock"]);
    assert!(serde_json::from_value::<Workspace>(value).is_err());
}

#[test]
fn empty_outputs_preserve_existing_document_serialization() {
    let document = parse_authored_document("```{r}\n1 + 1\n```\n", AuthoredFormat::Qmd).document;
    let value = serde_json::to_value(&document).unwrap();
    assert!(value["blocks"][0].get("outputs").is_none());
    assert_eq!(
        serde_json::from_value::<diplodocus::ir::Document>(value).unwrap(),
        document
    );
}

#[test]
fn additional_signature_and_output_variants_have_stable_tags() {
    for value in [
        json!({"kind": "value", "annotation": null, "value": {"kind": "literal", "text": "42"}}),
        json!({"kind": "language-specific", "language": "r", "syntax": {"kind": "language-specific", "language": "r", "name": "formula", "children": [{"kind": "sequence", "items": [{"kind": "name", "name": "x", "target": null}]}], "source": "~x"}}),
    ] {
        let signature: Signature = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(signature).unwrap(), value);
    }
    for kind in [
        "positional-only",
        "positional-or-keyword",
        "keyword-only",
        "variadic-positional",
        "variadic-keyword",
    ] {
        let value = json!({"kind": kind});
        let parameter: ParameterKind = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parameter).unwrap(), value);
    }
    let value = json!({"kind": {"type": "stream", "stream": "stderr"}, "representations": [], "provenance": []});
    let output: CellOutput = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(output).unwrap(), value);
}
