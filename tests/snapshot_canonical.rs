mod support;

use base64::Engine;
use diplodocus::assembly::assemble_workspace;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use rusqlite::{Connection, params};
use serde_json::Value;

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    Snapshot::from_sources(&sources, &resolved).unwrap()
}

// Emit descending object keys directly so this exercises both serde_json map backends.
fn reverse_object_keys(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by(|a, b| b.0.cmp(a.0));
            let fields = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(key).unwrap(),
                        reverse_object_keys(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{{\n{fields}\n}}")
        }
        Value::Array(array) => format!(
            "[{}]",
            array
                .iter()
                .map(reverse_object_keys)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

#[test]
fn export_and_fingerprints_ignore_sqlite_layout_and_json_object_order() {
    let root = support::acceptance_workspace();
    root.write("core/docs/layout.bin", b"\0\xfflayout\n");
    root.write("core/docs/layout.md", "[Download](layout.bin)\n");
    let snapshot = snapshot(&root);
    assert_eq!(snapshot.assets().len(), 2);
    let expected = snapshot.canonical_export().unwrap();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let mut logical: Value = serde_json::from_str(&expected).unwrap();
    for asset in logical["assets"].as_array_mut().unwrap() {
        asset.as_object_mut().unwrap().remove("bytes_base64");
    }
    logical.sort_all_objects();
    let digest: String = db
        .query_row(
            "SELECT content_fingerprint FROM manifest WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        fingerprint_bytes(&serde_json::to_vec(&logical).unwrap()).value,
        digest
    );
    let page_size: u32 = db
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .unwrap();
    db.pragma_update(
        None,
        "page_size",
        if page_size == 8192 { 4096 } else { 8192 },
    )
    .unwrap();
    db.execute_batch("VACUUM; PRAGMA user_version = 123;")
        .unwrap();
    let export: Value = serde_json::from_str(&expected).unwrap();
    db.execute("DELETE FROM records", []).unwrap();
    for record in export["records"].as_array().unwrap().iter().rev() {
        db.execute(
            "INSERT INTO records (content, kind, owner, id, fingerprint) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                reverse_object_keys(&record["content"]),
                record["kind"].as_str().unwrap(),
                record["owner"].as_str().unwrap(),
                record["id"].as_str().unwrap(),
                record["fingerprint"].as_str().unwrap(),
            ],
        )
        .unwrap();
    }
    db.execute("DELETE FROM assets", []).unwrap();
    for (digest, asset) in snapshot.assets().iter().rev() {
        db.execute(
            "INSERT INTO assets VALUES (?1, ?2, ?3)",
            params![digest, asset.media_type, asset.bytes],
        )
        .unwrap();
    }
    drop(db);
    drop(root);
    let after = std::fs::read(&path).unwrap();
    assert_ne!(&before[16..18], &after[16..18], "SQLite page size changed");
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        expected
    );
}

#[test]
fn export_retains_exact_binary_assets_and_sorts_semantic_record_keys() {
    let root = support::acceptance_workspace();
    let binary = b"\0\xff\x01\xfe\n";
    root.write("core/docs/canonical.bin", binary);
    root.write(
        "core/docs/canonical.md",
        "# Binary\n\n[Download](canonical.bin)\n",
    );
    let snapshot = snapshot(&root);
    let text = snapshot.canonical_export().unwrap();
    assert!(text.ends_with('\n'));
    assert!(!text.ends_with("\n\n"));
    assert!(!text.contains(root.path().to_str().unwrap()));
    let export: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<_> = export["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            (
                record["kind"].as_str().unwrap(),
                record["owner"].as_str().unwrap(),
                record["id"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    let mut previous = None;
    let mut found_binary = false;
    for asset in export["assets"].as_array().unwrap() {
        let digest = asset["digest"].as_str().unwrap();
        assert!(previous.is_none_or(|previous| previous < digest));
        previous = Some(digest);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(asset["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes, snapshot.assets()[digest].bytes);
        assert_eq!(bytes.len() as u64, asset["byte_size"].as_u64().unwrap());
        assert_eq!(fingerprint_bytes(&bytes).value, digest);
        if bytes == binary {
            found_binary = true;
            assert_eq!(asset["bytes_base64"], "AP8B/go=");
            assert_eq!(asset["media_type"], "application/octet-stream");
        }
    }
    assert!(found_binary);
}
