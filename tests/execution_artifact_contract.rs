//! Reference data checks, not evidence of a production cache or safety validator.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use diplodocus::configuration::ContentConfiguration;
use diplodocus::documents::{
    MarkdownFragmentOrigin, parse_markdown_fragment, prepare_collection_document,
};
use diplodocus::execution::{OutputVisibility, assets::validate_image_bytes};
use diplodocus::ir::SourceLocation;
use diplodocus::provenance::fingerprint_bytes;
use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/spikes/fixtures/execution-artifact-v1")
}

fn artifact() -> Value {
    serde_json::from_slice(&fs::read(root().join("manifest.json")).unwrap()).unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", fingerprint_bytes(bytes).value)
}

// This independent oracle must continue to match the existing encoding vector.
// It does not replace M6-04's strict decoder or canonical encoder.
fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.as_u64().expect("unsigned integer").to_string(),
        Value::String(value) => {
            let mut encoded = String::from("\"");
            for ch in value.chars() {
                match ch {
                    '"' => encoded.push_str("\\\""),
                    '\\' => encoded.push_str("\\\\"),
                    '\0'..='\u{1f}' => encoded.push_str(&format!("\\u{:04x}", ch as u32)),
                    _ => encoded.push(ch),
                }
            }
            encoded.push('"');
            encoded
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            assert!(keys.iter().all(|key| key.is_ascii()));
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!("{}:{}", canonical(&json!(key)), canonical(&values[key])))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn structured(domain: &str, value: &Value) -> String {
    digest(format!("{domain}\0{}", canonical(value)).as_bytes())
}

fn fields(value: &Value, names: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        names.iter().copied().collect()
    );
}

#[test]
fn canonical_artifact_matches_the_existing_independent_key_oracle() {
    let vector: Value = serde_json::from_str(include_str!(
        "../docs/spikes/fixtures/execution-cache-key-v1.json"
    ))
    .unwrap();
    assert_eq!(canonical(&vector["key_input"]), vector["canonical_utf8"]);
    assert_eq!(
        structured("diplodocus/page-execution-key-v1", &vector["key_input"]),
        vector["page_key"]
    );
    assert_eq!(
        canonical(&vector["encoding_vector"]["input"]),
        vector["encoding_vector"]["canonical_utf8"]
    );
    let manifest = artifact();
    fields(
        &manifest,
        &["schema", "key", "key_input", "result_digest", "result"],
    );
    fields(
        &manifest["result"],
        &["ir_schema", "provenance", "cells", "diagnostics", "assets"],
    );
    assert_eq!(
        fs::read(root().join("manifest.json")).unwrap(),
        canonical(&manifest).into_bytes()
    );
    assert_eq!(
        structured("diplodocus/page-execution-key-v1", &manifest["key_input"]),
        manifest["key"]
    );
    assert_eq!(
        structured("diplodocus/page-execution-result-v1", &manifest["result"]),
        manifest["result_digest"]
    );
    assert_eq!(manifest["result"]["provenance"]["origin"], "executed");
    for key in [
        "page",
        "engine",
        "policies",
        "components",
        "kernel",
        "platform",
        "deadlines_ms",
        "environment_inputs",
    ] {
        assert_eq!(
            manifest["result"]["provenance"][key], manifest["key_input"][key],
            "{key}"
        );
    }
}

#[test]
fn artifact_cells_match_current_preparation_and_exact_source_bytes() {
    let source = fs::read_to_string(root().join("source.qmd")).unwrap();
    let config: ContentConfiguration = toml::from_str("id='guide'\nowner='project'\nrepository='python'\npath='docs'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
    let parsed = prepare_collection_document(&source, &config).unwrap();
    assert!(
        parsed.parsed.diagnostics.is_empty(),
        "{:?}",
        parsed.parsed.diagnostics
    );
    let prepared = parsed.preparation.unwrap();
    let manifest = artifact();
    assert_eq!(
        digest(source.as_bytes()),
        manifest["key_input"]["page"]["source_digest"]
    );
    let cells = manifest["result"]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), prepared.cells.len());
    let mut defaults = serde_json::to_value(&prepared.defaults).unwrap();
    defaults["output"]["value"] = json!(true);
    let defaults = defaults.as_object().unwrap();
    let preparation = &manifest["result"]["provenance"]["preparation"];
    for (name, option) in defaults {
        assert_eq!(preparation["defaults"][name], option["value"]);
        assert_eq!(preparation["default_origins"][name], option["origin"]);
        assert_eq!(
            manifest["key_input"]["options"]["defaults"][name],
            option["value"]
        );
    }
    let mut declarations = Vec::new();
    for (index, (cell, current)) in cells.iter().zip(&prepared.cells).enumerate() {
        fields(
            cell,
            &[
                "ordinal",
                "label",
                "span",
                "source_segments",
                "submitted_source_digest",
                "effective",
                "option_origins",
                "outcome",
                "skip_reason",
                "outputs",
            ],
        );
        assert_eq!(cell["ordinal"], index);
        assert_eq!(
            cell["span"],
            serde_json::to_value(current.cell.span).unwrap(),
            "cell {index}"
        );
        assert_eq!(
            cell["source_segments"],
            serde_json::to_value(&current.cell.source_segments).unwrap()
        );
        assert_eq!(
            cell["effective"],
            manifest["key_input"]["options"]["cells"][index]["effective"]
        );
        let options = &current.options;
        let key_cell = &manifest["key_input"]["options"]["cells"][index];
        fields(
            key_cell,
            &[
                "ordinal",
                "language",
                "eligible",
                "submitted_source_digest",
                "effective",
            ],
        );
        assert_eq!(key_cell["ordinal"], current.ordinal);
        assert_eq!(key_cell["language"], json!(current.cell.language));
        assert_eq!(key_cell["eligible"], options.execution.eval.value);
        assert_eq!(
            key_cell["submitted_source_digest"],
            cell["submitted_source_digest"]
        );
        for declaration in &current.cell.options {
            declarations.push(json!({"cell": index, "kind": "hashpipe", "key": declaration.canonical_key, "raw": &source[declaration.span.start..declaration.span.end], "span": declaration.span}));
        }
        let effective = json!({"eval": options.execution.eval.value, "echo": options.execution.echo.value, "include": options.execution.include.value, "error": options.execution.error.value, "output": match options.execution.output.value { OutputVisibility::Show => json!(true), OutputVisibility::Hide => json!(false), OutputVisibility::AsIs => json!("asis") }, "label": options.label.value, "fig-alt": options.fig_alt.value, "fig-cap": options.fig_cap.value, "fig-subcap": options.fig_subcap.value});
        assert_eq!(cell["effective"], effective);
        assert_eq!(
            cell["option_origins"],
            json!({"eval": options.execution.eval.origin, "echo": options.execution.echo.origin, "output": options.execution.output.origin, "include": options.execution.include.origin, "error": options.execution.error.origin, "label": options.label.origin, "fig-alt": options.fig_alt.origin, "fig-cap": options.fig_cap.origin, "fig-subcap": options.fig_subcap.origin})
        );
        if options.execution.eval.value {
            assert_eq!(
                cell["submitted_source_digest"],
                digest(current.cell.source.as_bytes())
            );
            assert!(cell["skip_reason"].is_null());
        } else {
            assert!(cell["submitted_source_digest"].is_null());
            assert_eq!(cell["skip_reason"], "eval-false");
            assert_eq!(cell["outputs"], json!([]));
        }
    }
    assert_eq!(preparation["declarations"], json!(declarations));
    assert_eq!(cells[1]["effective"]["include"], false);
    assert_eq!(cells[2]["outcome"], "allowed-error");
}

fn references(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(asset) = map.get("asset") {
                fields(asset, &["digest", "media_type", "byte_size"]);
                found.insert(asset["digest"].as_str().unwrap().into());
            }
            for child in map.values() {
                references(child, found);
            }
        }
        Value::Array(values) => {
            for child in values {
                references(child, found);
            }
        }
        Value::String(value) => {
            for suffix in value.split("diplodocus-asset:").skip(1) {
                found.insert(suffix[..71].into());
            }
        }
        _ => {}
    }
}

#[test]
fn all_content_digests_and_the_complete_asset_set_are_fixed() {
    let manifest = artifact();
    let mut referenced = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for cell in manifest["result"]["cells"].as_array().unwrap() {
        for output in cell["outputs"].as_array().unwrap() {
            fields(
                output,
                &[
                    "kind",
                    "stream",
                    "owning_cell",
                    "producing_cell",
                    "updating_cell",
                    "slot",
                    "offered_mime_types",
                    "selected_mime_type",
                    "representations",
                    "diagnostic_indices",
                ],
            );
            for rep in output["representations"].as_array().unwrap() {
                fields(
                    rep,
                    &[
                        "kind",
                        "media_type",
                        "content",
                        "content_digest",
                        "producing_cell",
                        "policy",
                        "producer",
                        "fragment",
                    ],
                );
                let kind = rep["kind"].as_str().unwrap();
                kinds.insert(kind);
                let content = &rep["content"];
                let expected = match kind {
                    "text" if content["type"] == "literal" => {
                        digest(content["text"].as_str().unwrap().as_bytes())
                    }
                    "html-candidate" => digest(content["markup"].as_str().unwrap().as_bytes()),
                    "asset" => content["asset"]["digest"].as_str().unwrap().to_owned(),
                    "markdown" | "text" | "unsupported" => structured(
                        "diplodocus/execution-representation-v1",
                        &json!({"kind": if kind == "text" { "error" } else { kind }, "content": content}),
                    ),
                    _ => panic!("unknown kind"),
                };
                assert_eq!(rep["content_digest"], expected);
                references(content, &mut referenced);
            }
        }
    }
    assert_eq!(
        kinds,
        ["text", "markdown", "html-candidate", "asset", "unsupported"].into()
    );
    let assets = manifest["result"]["assets"].as_array().unwrap();
    assert_eq!(
        referenced,
        assets
            .iter()
            .map(|a| a["digest"].as_str().unwrap().to_owned())
            .collect()
    );
    let mut files = BTreeSet::new();
    for asset in assets {
        fields(asset, &["digest", "media_type", "byte_size"]);
        let hex = asset["digest"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap();
        let bytes = fs::read(root().join("assets/sha256").join(hex)).unwrap();
        assert_eq!(asset["byte_size"], bytes.len());
        assert_eq!(asset["digest"], digest(&bytes));
        validate_image_bytes(asset["media_type"].as_str().unwrap(), &bytes).unwrap();
        files.insert(hex.to_owned());
    }
    assert_eq!(
        files,
        fs::read_dir(root().join("assets/sha256"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    );
    assert_eq!(
        assets.len(),
        2,
        "includes the asset used only by hidden Markdown"
    );
}

#[test]
fn warning_references_and_final_slot_identity_survive_projection() {
    let manifest = artifact();
    let result = &manifest["result"];
    let warnings = result["diagnostics"].as_array().unwrap();
    assert_eq!(warnings.len(), 3);
    for diagnostic in warnings {
        fields(
            diagnostic,
            &[
                "code",
                "severity",
                "arguments",
                "source",
                "cell",
                "slot",
                "fragment",
                "span",
                "related_spans",
            ],
        );
        assert_eq!(diagnostic["severity"], "warning");
    }
    assert_eq!(
        warnings[0]["arguments"],
        json!({"kind":"kernel-message-ignored"})
    );
    assert_eq!(
        warnings[2]["arguments"],
        json!({"kind":"html-rejected", "reason":"element"})
    );
    let outputs = &result["cells"][0]["outputs"];
    assert_eq!(outputs[0]["slot"], 1);
    assert_eq!(outputs[1]["slot"], 2);
    assert_eq!(outputs[1]["owning_cell"], 0);
    assert_eq!(outputs[1]["producing_cell"], 0);
    assert_eq!(outputs[1]["updating_cell"], 1);
    for rep in outputs[1]["representations"].as_array().unwrap() {
        assert_eq!(rep["producing_cell"], 1);
    }
    let unsupported = &result["cells"][2]["outputs"][0];
    assert!(unsupported["selected_mime_type"].is_null());
    assert_eq!(unsupported["diagnostic_indices"], json!([2]));
    fields(
        &unsupported["representations"][0]["content"],
        &["mime_types"],
    );
    assert_eq!(
        unsupported["representations"][0]["content"]["mime_types"],
        unsupported["offered_mime_types"]
    );
    for cell in result["cells"].as_array().unwrap() {
        for output in cell["outputs"].as_array().unwrap() {
            for index in output["diagnostic_indices"].as_array().unwrap() {
                assert!(index.as_u64().unwrap() < warnings.len() as u64);
            }
        }
    }
    // Current discovery warnings are prepended only in the portable projection.
    let current_count = 3;
    let remapped = unsupported["diagnostic_indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_u64().unwrap() + current_count)
        .collect::<Vec<_>>();
    assert_eq!(remapped, [5]);
    let mut projected = unsupported.clone();
    projected["diagnostic_indices"] = json!(remapped);
    assert_eq!(
        projected["representations"][0]["content_digest"],
        structured(
            "diplodocus/execution-representation-v1",
            &json!({"kind":"unsupported", "content":projected["representations"][0]["content"]})
        )
    );
    assert_eq!(unsupported["diagnostic_indices"], json!([2]));
}

#[test]
fn cleared_fragment_warning_keeps_its_original_coordinate_space() {
    let manifest = artifact();
    let source = fs::read_to_string(root().join("cleared-fragment.md")).unwrap();
    let parsed = parse_markdown_fragment(
        &source,
        MarkdownFragmentOrigin {
            collection: "guide".into(),
            cell: 0,
            output: 0,
            source: SourceLocation {
                repository: "python".into(),
                path: "docs/artifact.qmd".try_into().unwrap(),
                span: None,
            },
        },
    );
    assert_eq!(parsed.diagnostics.len(), 1);
    let warning = &manifest["result"]["diagnostics"][1];
    assert!(warning["source"].is_null());
    assert_eq!(
        warning["span"],
        serde_json::to_value(parsed.diagnostics[0].span).unwrap()
    );
    assert_eq!(warning["code"], parsed.diagnostics[0].code.as_str());
    assert_eq!(warning["cell"], 0);
    assert_eq!(warning["slot"], 0);
    assert_eq!(
        warning["fragment"],
        json!({"ordinal":0,"byte_length":source.len()})
    );
    assert_eq!(
        warning["arguments"],
        json!({"kind":"fragment-unsupported", "source_kind": serde_json::to_value(parsed.representation).unwrap()["blocks"][0]["source_kind"]})
    );
    assert!(
        manifest["result"]["cells"][0]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["slot"] != 0)
    );
}

#[test]
fn nested_markdown_uses_typed_images_without_changing_the_inert_parser_tree() {
    let manifest = artifact();
    for (cell, output, rep, filename) in [(0, 1, 1, "fragment.md"), (1, 0, 0, "hidden-fragment.md")]
    {
        let content = &manifest["result"]["cells"][cell]["outputs"][output]["representations"][rep]
            ["content"];
        let source = fs::read_to_string(root().join(filename)).unwrap();
        let parsed = parse_markdown_fragment(
            &source,
            MarkdownFragmentOrigin {
                collection: "guide".into(),
                cell: 1,
                output: 0,
                source: SourceLocation {
                    repository: "python".into(),
                    path: "docs/artifact.qmd".try_into().unwrap(),
                    span: None,
                },
            },
        );
        let mut blocks = serde_json::to_value(parsed.representation).unwrap()["blocks"].clone();
        fn replace_images(value: &mut Value, asset: &Value) {
            match value {
                Value::Object(map) => {
                    if map.get("type") == Some(&json!("image")) {
                        map.remove("target");
                        map.insert("asset".into(), asset.clone());
                    }
                    for child in map.values_mut() {
                        replace_images(child, asset);
                    }
                }
                Value::Array(values) => {
                    for child in values {
                        replace_images(child, asset);
                    }
                }
                _ => {}
            }
        }
        let asset = if cell == 0 {
            &manifest["result"]["cells"][0]["outputs"][1]["representations"][0]["content"]["asset"]
        } else {
            &content["blocks"][0]["inlines"][0]["asset"]
        };
        replace_images(&mut blocks, asset);
        assert_eq!(content["blocks"], blocks);
    }
}
