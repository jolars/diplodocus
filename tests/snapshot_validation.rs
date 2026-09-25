mod support;

use base64::Engine;
use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::WORKSPACE_SCHEMA_VERSION;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::snapshots::{
    RECORD_ENCODING_VERSION, STORAGE_SCHEMA_VERSION, Snapshot, SnapshotError,
};
use diplodocus::validation::resolve_workspace;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

fn snapshot() -> Snapshot {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    Snapshot::from_sources(&sources, &resolved).unwrap()
}

fn canonical_fingerprint(mut value: Value) -> String {
    value.sort_all_objects();
    fingerprint_bytes(&serde_json::to_vec(&value).unwrap()).value
}

// Recompute every digest so these tests exercise validation beyond corruption detection.
fn rewrite(path: &std::path::Path, export: &mut Value) {
    let db = Connection::open(path).unwrap();
    db.execute("DELETE FROM records", []).unwrap();
    for record in export["records"].as_array_mut().unwrap() {
        let key = json!({"kind": record["kind"], "owner": record["owner"], "id": record["id"]});
        record["fingerprint"] = canonical_fingerprint(json!([
            "diplodocus/snapshot-record-v1",
            key,
            record["content"]
        ]))
        .into();
        db.execute(
            "INSERT INTO records VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record["kind"].as_str().unwrap(),
                record["owner"].as_str().unwrap(),
                record["id"].as_str().unwrap(),
                serde_json::to_string(&record["content"]).unwrap(),
                record["fingerprint"].as_str().unwrap(),
            ],
        )
        .unwrap();
    }
    db.execute("DELETE FROM assets", []).unwrap();
    for asset in export["assets"].as_array().unwrap() {
        db.execute(
            "INSERT INTO assets VALUES (?1, ?2, ?3)",
            params![
                asset["digest"].as_str().unwrap(),
                asset["media_type"].as_str().unwrap(),
                base64::engine::general_purpose::STANDARD
                    .decode(asset["bytes_base64"].as_str().unwrap())
                    .unwrap(),
            ],
        )
        .unwrap();
    }
    let mut logical = export.clone();
    for asset in logical["assets"].as_array_mut().unwrap() {
        asset.as_object_mut().unwrap().remove("bytes_base64");
    }
    let digest = canonical_fingerprint(logical);
    db.execute(
        "UPDATE manifest SET content_fingerprint=?1, producer=?2",
        params![digest, export["producer"].as_str().unwrap()],
    )
    .unwrap();
}

fn record<'a>(export: &'a mut Value, kind: &str) -> &'a mut Value {
    &mut export["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["kind"] == kind)
        .unwrap()["content"]
}

fn changed(
    snapshot: &Snapshot,
    change: impl FnOnce(&mut Value),
) -> Result<Snapshot, SnapshotError> {
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let mut export = serde_json::from_str(&snapshot.canonical_export().unwrap()).unwrap();
    change(&mut export);
    rewrite(&path, &mut export);
    let before = std::fs::read(&path).unwrap();
    let result = Snapshot::load(&path);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(
        !matches!(
            result,
            Err(SnapshotError::Invalid(
                "record fingerprint" | "snapshot fingerprint"
            ))
        ),
        "fingerprint rejection masked record validation: {result:?}"
    );
    result
}

#[test]
fn rejects_missing_or_malformed_presentation_even_with_valid_fingerprints() {
    let snapshot = snapshot();
    for case in [
        "missing", "owner", "id", "shape", "field", "unknown", "type",
    ] {
        let result = changed(&snapshot, |export| {
            let records = export["records"].as_array_mut().unwrap();
            if case == "missing" {
                records.retain(|r| r["kind"] != "presentation");
                return;
            }
            let record = records
                .iter_mut()
                .find(|r| r["kind"] == "presentation")
                .unwrap();
            match case {
                "owner" => record["owner"] = "project".into(),
                "id" => record["id"] = "defaults".into(),
                "shape" => record["content"] = json!([]),
                "field" => {
                    record["content"].as_object_mut().unwrap().remove("title");
                }
                "unknown" => record["content"]["theme_path"] = "/tmp/theme".into(),
                "type" => record["content"]["title"] = true.into(),
                _ => unreachable!(),
            }
        });
        assert!(result.is_err(), "accepted {case}");
    }
}

#[test]
fn loading_and_republishing_preserve_the_original_producer_version() {
    let snapshot = snapshot();
    let loaded = changed(&snapshot, |export| export["producer"] = "0.0.1".into()).unwrap();
    assert_eq!(loaded.producer(), "0.0.1");
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    loaded.publish(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap().producer(), "0.0.1");
}

#[test]
fn rejects_nested_references_even_with_valid_fingerprints() {
    let snapshot = snapshot();
    let missing = json!({"package": "pyfoo", "item": "missing"});
    let source = json!({"repository": "missing", "path": "source.py", "span": null});
    let evidence = json!({"source": source, "role": "definition", "parsers": []});
    let expression = json!({"kind": "name", "name": "Missing", "target": missing});
    let declaration = json!({"activity": {"kind": "declaration"}, "source": {"kind": "repository", "repository": "missing", "path": "source.py"}, "span": null, "tools": {}});
    let mut accepted = Vec::new();
    for case in [
        "signature-target",
        "signature-source",
        "alias-source",
        "python-family",
        "python-overload",
        "python-constructor",
        "python-field",
        "python-base",
        "python-type-alias",
        "python-decorator",
        "python-export-source",
        "r-method",
        "r-generic",
        "r-registration",
        "item-provenance",
        "document-provenance",
        "workspace-provenance",
        "extraction-target",
        "extraction-repository",
        "generated-collection",
        "execution-environment",
        "diagnostic-source",
    ] {
        let result = changed(&snapshot, |export| match case {
            "signature-target" | "signature-source" => {
                record(export, "item")["signatures"] = json!([{
                    "signature": {"kind": "callable", "parameters": [{"name": "x", "kind": {"kind": "positional-only"}, "annotation": {"kind": "apply", "constructor": {"kind": "literal", "text": "list"}, "arguments": [expression]}, "default": null}], "returns": null},
                    "sources": if case == "signature-source" { json!([evidence]) } else { json!([]) },
                }]);
                if case == "signature-source" {
                    record(export, "item")["signatures"][0]["signature"]["parameters"] = json!([]);
                }
            }
            "alias-source" => {
                record(export, "item")["aliases"] = json!([{"qualified_name": "alias", "kind": "python-reexport", "sources": [evidence]}])
            }
            "python-family"
            | "python-overload"
            | "python-constructor"
            | "python-field"
            | "python-base"
            | "python-type-alias"
            | "python-decorator"
            | "python-export-source" => {
                let mut data = json!({"visibility": "public", "declaration": {"kind": "constant"}, "decorators": []});
                data["declaration"] = match case {
                    "python-family" => {
                        json!({"kind": "callable", "binding": "function", "is_async": false, "role": {"kind": "family", "overloads": [missing]}})
                    }
                    "python-overload" => {
                        json!({"kind": "callable", "binding": "function", "is_async": false, "role": {"kind": "overload", "family": missing}})
                    }
                    "python-constructor" => {
                        json!({"kind": "class", "bases": [], "constructor": {"kind": "explicit", "item": missing}})
                    }
                    "python-field" => {
                        json!({"kind": "class", "bases": [], "constructor": {"kind": "dataclass", "fields": [missing], "init": true, "frozen": false}})
                    }
                    "python-base" => {
                        json!({"kind": "class", "bases": [expression], "constructor": {"kind": "unspecified"}})
                    }
                    "python-type-alias" => json!({"kind": "type-alias", "target": expression}),
                    "python-export-source" => {
                        json!({"kind": "module", "source": "implementation", "exports": {"kind": "explicit", "names": [], "sources": [evidence]}})
                    }
                    _ => json!({"kind": "constant"}),
                };
                if case == "python-decorator" {
                    data["decorators"] =
                        json!([{"expression": expression, "semantics": "unknown", "sources": []}]);
                }
                record(export, "item")["language_data"] =
                    json!({"language": "python", "data": data});
            }
            "r-method" | "r-generic" | "r-registration" => {
                let declaration = match case {
                    "r-method" => {
                        json!({"kind": "s3-generic", "dispatch_name": "f", "dispatch_object": null, "methods": [missing]})
                    }
                    "r-generic" => {
                        json!({"kind": "s3-method", "generic": {"kind": "workspace", "item": missing}, "class": "a", "registration": []})
                    }
                    _ => {
                        json!({"kind": "s3-method", "generic": {"kind": "external", "package": "stats", "name": "predict"}, "class": "a", "registration": [evidence]})
                    }
                };
                record(export, "item")["language_data"] = json!({"language": "r", "data": {"exported": true, "declaration": declaration}});
            }
            "item-provenance" => record(export, "item")["provenance"] = json!([declaration]),
            "document-provenance" => {
                record(export, "page")["document"]["provenance"] = json!([declaration])
            }
            "workspace-provenance" => {
                record(export, "workspace")["provenance"] = json!([declaration])
            }
            "extraction-target" | "extraction-repository" => {
                let provenance = record(export, "workspace")["provenance"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|p| p["activity"]["kind"] == "extraction")
                    .unwrap();
                if case == "extraction-target" {
                    provenance["activity"]["target"]["target"] = "missing".into();
                } else {
                    provenance["activity"]["inputs"]["missing"] = json!({});
                }
            }
            "generated-collection" => {
                record(export, "item")["provenance"] = json!([{"activity": {"kind": "generated-markdown", "collection": "missing", "cell": 0, "output": 0}, "source": null, "span": null, "tools": {}}])
            }
            "execution-environment" => {
                record(export, "workspace")["provenance"] = json!([{"activity": {"kind": "execution", "mode": "execute", "engine": "jupyter", "kernel": {"name": "python3", "language": null, "language_version": null, "version": null}, "origin": "executed", "declared_environment_inputs": [{"source": source, "fingerprint": {"algorithm": "sha256", "value": "0".repeat(64)}}]}, "source": null, "span": null, "tools": {}}])
            }
            "diagnostic-source" => {
                record(export, "workspace")["diagnostics"] = json!([{"code": "unsupported-authored-syntax", "severity": "warning", "message": "warning", "source": {"kind": "repository", "repository": "missing", "path": "source.py"}, "span": null}])
            }
            _ => unreachable!(),
        });
        if result.is_ok() {
            accepted.push(case);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted dangling references: {accepted:?}"
    );
}

#[test]
fn rejects_malformed_records_paths_and_keys() {
    let snapshot = snapshot();
    let mut accepted = Vec::new();
    for case in [
        "missing-workspace",
        "missing-document",
        "missing-repository",
        "missing-package",
        "missing-collection",
        "missing-target",
        "unknown-kind",
        "empty-id",
        "wrong-owner",
        "missing-field",
        "unknown-field",
        "wrong-shape",
        "ir-version",
        "absolute-path",
        "parent-path",
        "windows-path",
        "target-path",
        "slug",
        "mount",
    ] {
        let result = changed(&snapshot, |export| match case {
            "missing-workspace" | "missing-document" | "missing-repository" | "missing-package"
            | "missing-collection" | "missing-target" => {
                let kind = case.strip_prefix("missing-").unwrap();
                export["records"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|r| r["kind"] != kind);
            }
            "unknown-kind" | "empty-id" | "wrong-owner" => {
                let row = export["records"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|r| r["kind"] == "item")
                    .unwrap();
                match case {
                    "unknown-kind" => row["kind"] = "unknown".into(),
                    "empty-id" => row["id"] = "".into(),
                    _ => row["owner"] = "missing".into(),
                }
            }
            "missing-field" => {
                record(export, "package")
                    .as_object_mut()
                    .unwrap()
                    .remove("name");
            }
            "unknown-field" => record(export, "package")["unknown"] = true.into(),
            "wrong-shape" => *record(export, "package") = json!([]),
            "ir-version" => record(export, "workspace")["schema_version"] = 2.into(),
            "absolute-path" => record(export, "package")["path"] = "/tmp/source".into(),
            "parent-path" => record(export, "package")["metadata_path"] = "../metadata".into(),
            "windows-path" => record(export, "package")["path"] = "C:\\source".into(),
            "target-path" => record(export, "target")["path"] = "a/../../source".into(),
            "slug" => record(export, "package")["slug"] = "../escape".into(),
            "mount" => record(export, "collection")["mount"] = "../escape".into(),
            _ => unreachable!(),
        });
        if result.is_ok() {
            accepted.push(case);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted invalid records: {accepted:?}"
    );
}

#[test]
fn rejects_invalid_cell_option_indices_without_panicking() {
    let snapshot = snapshot();
    let result = changed(&snapshot, |export| {
        record(export, "page")["document"]["document"]["blocks"] = json!([{
            "type": "code-cell", "language": "python", "identifier": null, "classes": [], "labels": [],
            "source": "", "source_segments": [], "options": [],
            "resolved_options": [{"key": "label", "resolution": {"type": "resolved", "declaration": 0}}],
            "code_span": null, "span": {"start": 0, "end": 0},
        }]);
    });
    assert!(result.is_err());
}

#[test]
fn loading_is_read_only_and_never_creates_a_missing_database() {
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    assert!(Snapshot::load(&path).is_err());
    assert!(!path.exists());
    let snapshot = snapshot();
    snapshot.publish(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(
        Snapshot::load(&path).unwrap().workspace(),
        snapshot.workspace()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 1);
}

#[test]
fn each_version_is_rejected_before_decoding_records_without_migration() {
    let snapshot = snapshot();
    for (field, version) in [
        ("storage_version", 0),
        ("storage_version", STORAGE_SCHEMA_VERSION - 1),
        ("storage_version", STORAGE_SCHEMA_VERSION + 1),
        ("ir_version", 0),
        ("ir_version", WORKSPACE_SCHEMA_VERSION + 1),
        ("encoding_version", 0),
        ("encoding_version", RECORD_ENCODING_VERSION + 1),
    ] {
        let target = tempfile::tempdir().unwrap();
        let path = target.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!(
            "UPDATE manifest SET {field} = {version}; DROP TABLE records; DROP TABLE assets;"
        ))
        .unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(
            matches!(Snapshot::load(&path), Err(SnapshotError::Version)),
            "{field} = {version}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 1);
    }
}

#[test]
fn accepts_valid_nested_and_external_references_without_source_files() {
    let snapshot = snapshot();
    assert_eq!(
        changed(&snapshot, |_| {})
            .unwrap()
            .canonical_export()
            .unwrap(),
        snapshot.canonical_export().unwrap(),
    );
    changed(&snapshot, |export| {
        record(export, "item")["signatures"] = json!([{
            "signature": {"kind": "value", "annotation": {"kind": "name", "name": "fit", "target": {"package": "pyfoo", "item": "sid1:python:function:foo.model.fit"}}, "value": null},
            "sources": [{"source": {"repository": "python", "path": "absent.py", "span": null}, "role": "definition", "parsers": []}],
        }]);
        record(export, "item")["language_data"] = json!({"language": "r", "data": {"exported": true, "declaration": {"kind": "s3-method", "generic": {"kind": "external", "package": "stats", "name": "predict"}, "class": "a", "registration": []}}});
    }).unwrap();
}

#[test]
fn validates_asset_bytes_and_references_independently_of_record_fingerprints() {
    let snapshot = snapshot();
    for case in ["missing", "corrupt", "algorithm", "reference", "path"] {
        let result = changed(&snapshot, |export| match case {
            "missing" => export["assets"] = json!([]),
            "corrupt" => {
                export["assets"][0]["bytes_base64"] = "AA==".into();
                export["assets"][0]["byte_size"] = 1.into();
            }
            _ => {
                let reference = export["records"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .filter(|r| r["kind"] == "document")
                    .flat_map(|r| r["content"]["references"].as_array_mut().unwrap())
                    .find(|r| r["target"]["kind"] == "asset")
                    .unwrap();
                let asset = &mut reference["target"]["asset"];
                match case {
                    "algorithm" => asset["fingerprint"]["algorithm"] = "unknown".into(),
                    "reference" => asset["fingerprint"]["value"] = "0".repeat(64).into(),
                    "path" => asset["path"] = "unrelated/asset".into(),
                    _ => unreachable!(),
                }
            }
        });
        let expected = match case {
            "missing" | "reference" => "missing referenced asset",
            "corrupt" => "asset fingerprint",
            "algorithm" => "asset digest metadata",
            "path" => "asset path",
            _ => unreachable!(),
        };
        assert!(
            matches!(result, Err(SnapshotError::Invalid(message)) if message == expected),
            "{case}: {result:?}"
        );
    }
}

#[test]
fn rejects_invalid_image_contents_even_with_recomputed_asset_digests() {
    let snapshot = snapshot();
    assert_eq!(snapshot.assets().len(), 1);
    let valid_svg = b"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>";
    for (case, media, bytes) in [
        ("valid control", "image/svg+xml", valid_svg.as_slice()),
        ("truncated SVG", "image/svg+xml", b"<svg><circle".as_slice()),
        (
            "truncated PNG",
            "image/png",
            b"\x89PNG\r\n\x1a\n".as_slice(),
        ),
        (
            "truncated JPEG",
            "image/jpeg",
            b"\xff\xd8\xff\xe0".as_slice(),
        ),
        ("wrong media", "image/png", valid_svg.as_slice()),
        ("unsupported media", "image/gif", valid_svg.as_slice()),
        (
            "active SVG",
            "image/svg+xml",
            b"<svg><script>alert(1)</script></svg>".as_slice(),
        ),
    ] {
        let result = changed(&snapshot, |export| {
            let digest = fingerprint_bytes(bytes).value;
            export["assets"][0] = json!({
                "digest": digest,
                "media_type": media,
                "byte_size": bytes.len(),
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
            });
            for reference in export["records"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .filter(|r| r["kind"] == "document")
                .flat_map(|r| r["content"]["references"].as_array_mut().unwrap())
                .filter(|r| r["target"]["kind"] == "asset")
            {
                let asset = &mut reference["target"]["asset"];
                asset["fingerprint"]["value"] = digest.clone().into();
                asset["path"] = format!("content-assets/sha256/{digest}").into();
            }
        });
        if case == "valid control" {
            let loaded = result.unwrap();
            assert_eq!(
                loaded.assets()[&fingerprint_bytes(bytes).value].bytes,
                bytes
            );
        } else {
            assert!(
                matches!(result, Err(SnapshotError::Invalid("asset media"))),
                "{case}: {result:?}"
            );
        }
    }
}

#[test]
fn round_trips_page_links_with_empty_fragments() {
    let root = support::acceptance_workspace();
    root.write("core/docs/link.md", "[Home](index.md#)\n");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    assert_eq!(
        Snapshot::load(path).unwrap().documents(),
        snapshot.documents()
    );
}
