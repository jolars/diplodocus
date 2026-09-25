//! Record encoding version 1, shared by storage integrity checks and text export.
//!
//! Typed IR maps and sets supply their semantic ordering. Only JSON object keys
//! are sorted here; arrays and opaque strings retain their exact meaning.

use super::*;

use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ir::WORKSPACE_SCHEMA_VERSION;
use crate::provenance::fingerprint_bytes;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(super) struct Key {
    pub(super) kind: String,
    pub(super) owner: String,
    pub(super) id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct Record {
    #[serde(flatten)]
    pub(super) key: Key,
    pub(super) content: Value,
    pub(super) fingerprint: String,
}

pub(super) fn fingerprint(key: &Key, content: &Value) -> Result<String, SnapshotError> {
    let mut value = json!(["diplodocus/snapshot-record-v1", key, content]);
    // Feature unification can enable serde_json's insertion-ordered map backend.
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value)?;
    Ok(fingerprint_bytes(&bytes).value)
}

pub(super) fn records(snapshot: &Snapshot) -> Result<Vec<Record>, SnapshotError> {
    let mut workspace = serde_json::to_value(&snapshot.workspace)?;
    let object = workspace.as_object_mut().expect("workspace object");
    let mut values = BTreeMap::new();
    for (field, kind) in [
        ("repositories", "repository"),
        ("packages", "package"),
        ("content_collections", "collection"),
        ("pages", "page"),
        ("concepts", "concept"),
    ] {
        let entries = object.remove(field).expect("workspace field");
        for (id, mut value) in entries.as_object().expect("entity map").clone() {
            if kind == "package" {
                for (field, kind) in [("items", "item"), ("extraction_targets", "target")] {
                    let nested = value.as_object_mut().unwrap().remove(field).unwrap();
                    for (child, value) in nested.as_object().unwrap() {
                        values.insert(
                            Key {
                                kind: kind.into(),
                                owner: id.clone(),
                                id: child.clone(),
                            },
                            value.clone(),
                        );
                    }
                }
            }
            values.insert(
                Key {
                    kind: kind.into(),
                    owner: String::new(),
                    id,
                },
                value,
            );
        }
    }
    values.insert(
        Key {
            kind: "presentation".into(),
            owner: String::new(),
            id: String::new(),
        },
        serde_json::to_value(&snapshot.presentation)?,
    );
    values.insert(
        Key {
            kind: "workspace".into(),
            owner: String::new(),
            id: String::new(),
        },
        workspace,
    );
    for document in &snapshot.documents {
        values.insert(
            Key {
                kind: "document".into(),
                owner: String::new(),
                id: serde_json::to_string(&document.document)?,
            },
            serde_json::to_value(document)?,
        );
    }
    for (id, page) in &snapshot.executions {
        values.insert(
            Key {
                kind: "execution".into(),
                owner: String::new(),
                id: id.clone(),
            },
            serde_json::to_value(page)?,
        );
    }
    values
        .into_iter()
        .map(|(key, mut content)| {
            content.sort_all_objects();
            Ok(Record {
                fingerprint: fingerprint(&key, &content)?,
                key,
                content,
            })
        })
        .collect()
}

fn logical_value(snapshot: &Snapshot, records: &[Record], include_bytes: bool) -> Value {
    let assets: Vec<_> = snapshot.assets.iter().map(|(digest, asset)| {
        let mut value = json!({"digest": digest, "media_type": asset.media_type, "byte_size": asset.bytes.len()});
        if include_bytes {
            value["bytes_base64"] = base64::engine::general_purpose::STANDARD.encode(&asset.bytes).into();
        }
        value
    }).collect();
    let mut value = json!({
        "storage_version": STORAGE_SCHEMA_VERSION,
        "ir_version": WORKSPACE_SCHEMA_VERSION,
        "encoding_version": RECORD_ENCODING_VERSION,
        "producer": snapshot.producer,
        "records": records,
        "assets": assets,
    });
    value.sort_all_objects();
    value
}
pub(super) fn content_fingerprint(
    snapshot: &Snapshot,
    records: &[Record],
) -> Result<String, SnapshotError> {
    Ok(fingerprint_bytes(&serde_json::to_vec(&logical_value(
        snapshot, records, false,
    ))?)
    .value)
}
pub(super) fn canonical_export(snapshot: &Snapshot) -> Result<String, SnapshotError> {
    Ok(serde_json::to_string_pretty(&logical_value(snapshot, &records(snapshot)?, true))? + "\n")
}
