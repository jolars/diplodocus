use super::*;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::Repository;

fn fixture() -> Snapshot {
    Snapshot {
        workspace: Workspace {
            name: "Canonical café".into(),
            repositories: BTreeMap::from([(
                "docs".into(),
                Repository {
                    canonical_url: Some("https://example.org/docs".into()),
                    source_link_template: None,
                    revision: Some("v1".into()),
                    dirty: Some(false),
                    declared_input_fingerprint: None,
                },
            )]),
            ..Workspace::default()
        },
        presentation: PresentationDefaults {
            title: Some("Canonical café".into()),
            description: None,
        },
        documents: Vec::new(),
        assets: BTreeMap::new(),
        producer: "canonical-fixture".into(),
        executions: BTreeMap::new(),
        validated_outputs: BTreeMap::new(),
    }
}

#[test]
fn record_encoding_matches_independent_sha256_vector() {
    let key = Key {
        kind: "item".into(),
        owner: "pkg".into(),
        id: "sid1:python:function:café".into(),
    };
    let content = json!({
        "z": [2, 1, {"z": null, "a": true}],
        "a": "café\n\t\u{8}\u{c}\r\0\"\\/\u{2028}",
        "n": u64::MAX,
    });
    // Python's hashlib and sorted compact JSON provide an independent vector.
    assert_eq!(
        fingerprint(&key, &content).unwrap(),
        "81011e6f08479b7605a0859c0212c10fcc81b57be77778b011c147a18a013325"
    );
}

#[test]
fn record_fingerprints_bind_identity_and_preserve_array_and_string_contents() {
    let key = Key {
        kind: "item".into(),
        owner: "pkg".into(),
        id: "identity".into(),
    };
    let content = json!({"children": ["second", "first"], "name": "café"});
    let expected = fingerprint(&key, &content).unwrap();
    for changed in [
        Key {
            kind: "page".into(),
            ..key.clone()
        },
        Key {
            owner: "other".into(),
            ..key.clone()
        },
        Key {
            id: "other".into(),
            ..key.clone()
        },
    ] {
        assert_ne!(fingerprint(&changed, &content).unwrap(), expected);
    }
    for changed in [
        json!({"children": ["first", "second"], "name": "café"}),
        json!({"children": ["second", "first"], "name": "cafe\u{301}"}),
        json!({"children": ["second", "first"], "name": "café", "optional": null}),
    ] {
        assert_ne!(fingerprint(&key, &changed).unwrap(), expected);
    }
    let reordered = serde_json::from_str(
        "{\n  \"name\": \"caf\\u00e9\", \"children\": [\"second\", \"first\"]\n}",
    )
    .unwrap();
    assert_eq!(fingerprint(&key, &reordered).unwrap(), expected);
}

#[test]
fn minimal_export_and_snapshot_digest_match_independent_fixture() {
    let snapshot = fixture();
    snapshot.validate().unwrap();
    assert_eq!(
        canonical_export(&snapshot).unwrap(),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/snapshots/snapshot-encoding/minimal.json"
        ))
    );
    assert_eq!(
        content_fingerprint(&snapshot, &records(&snapshot).unwrap()).unwrap(),
        "a573096448832e533ef78a5f1dcef56a388dd7735eb08955438f6e3403bf504f"
    );
}

#[test]
fn map_and_set_insertion_order_does_not_change_canonical_records() {
    let mut snapshot = fixture();
    let repository = snapshot.workspace.repositories["docs"].clone();
    snapshot
        .workspace
        .repositories
        .insert("another".into(), repository);
    for message in ["Second warning", "First warning"] {
        snapshot.workspace.diagnostics.insert(Diagnostic::new(
            DiagnosticCode::UnsupportedExtractor,
            Severity::Warning,
            message,
        ));
    }
    let before = canonical_export(&snapshot).unwrap();
    snapshot.workspace.repositories = snapshot.workspace.repositories.into_iter().rev().collect();
    snapshot.workspace.diagnostics = snapshot.workspace.diagnostics.into_iter().rev().collect();
    assert_eq!(canonical_export(&snapshot).unwrap(), before);
}

#[test]
fn fingerprints_change_only_for_the_edited_entity() {
    let mut snapshot = fixture();
    let before = records(&snapshot).unwrap();
    let digest = content_fingerprint(&snapshot, &before).unwrap();
    snapshot
        .workspace
        .repositories
        .get_mut("docs")
        .unwrap()
        .revision = Some("v2".into());
    let after = records(&snapshot).unwrap();
    let changed: Vec<_> = before
        .iter()
        .zip(&after)
        .filter_map(|(before, after)| {
            assert_eq!(before.key, after.key);
            (before.fingerprint != after.fingerprint).then_some(after.key.kind.as_str())
        })
        .collect();
    assert_eq!(changed, ["repository"]);
    assert_ne!(content_fingerprint(&snapshot, &after).unwrap(), digest);
    let digest = content_fingerprint(&snapshot, &after).unwrap();
    snapshot.producer = "another-version".into();
    assert_eq!(records(&snapshot).unwrap(), after);
    assert_ne!(content_fingerprint(&snapshot, &after).unwrap(), digest);
}
