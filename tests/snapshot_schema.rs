mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::{ItemLanguageData, SourcedDocument, SourcedSignature};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{DocumentIdentity, resolve_workspace};
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const CONTRACT: &str = include_str!("../docs/design/snapshot-schema.md");

fn example(heading: &str, language: &str) -> &'static str {
    CONTRACT
        .split_once(&format!("### {heading}\n"))
        .unwrap()
        .1
        .split_once(&format!("```{language}\n"))
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0
}

fn with_snapshot(check: impl FnOnce(&Snapshot, &Connection)) {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    check(&snapshot, &db);
}

#[test]
fn every_entity_is_queryable_by_its_documented_semantic_key() {
    with_snapshot(|snapshot, db| {
        let manifest: (u32, u32, u32) = db
            .query_row(
                "SELECT storage_version, ir_version, encoding_version FROM manifest WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(manifest, (2, 1, 1));
        let mut count = 0;
        let mut assert_record = |kind: &str, owner: &str, id: &str, expected: &Value| {
            let content: String = db
                .query_row(
                    "SELECT content FROM records WHERE kind = ?1 AND owner = ?2 AND id = ?3",
                    params![kind, owner, id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&content).unwrap(), *expected);
            count += 1;
        };
        let workspace = snapshot.workspace();
        assert_record("presentation", "", "", &json!(snapshot.presentation()));
        assert_record(
            "workspace",
            "",
            "",
            &json!({
                "schema_version": 1,
                "name": workspace.name,
                "relationships": workspace.relationships,
                "diagnostics": workspace.diagnostics,
                "provenance": workspace.provenance,
            }),
        );
        for (id, repository) in &workspace.repositories {
            assert_record("repository", "", id, &json!(repository));
        }
        for (id, package) in &workspace.packages {
            assert_record(
                "package",
                "",
                id,
                &json!({
                    "slug": package.slug, "name": package.name,
                    "ecosystem": package.ecosystem, "version": package.version,
                    "repository": package.repository, "path": package.path,
                    "metadata_path": package.metadata_path,
                    "kind": package.kind, "visibility": package.visibility,
                }),
            );
            for (target_id, target) in &package.extraction_targets {
                assert_record("target", id, target_id, &json!(target));
            }
            for (item_id, item) in &package.items {
                assert_record("item", id, item_id, &json!(item));
            }
        }
        for (id, collection) in &workspace.content_collections {
            assert_record("collection", "", id, &json!(collection));
        }
        for (id, page) in &workspace.pages {
            assert_record("page", "", id, &json!(page));
        }
        for (id, concept) in &workspace.concepts {
            assert_record("concept", "", id, &json!(concept));
        }
        for document in snapshot.documents() {
            let id = match &document.document {
                DocumentIdentity::Page { page } => {
                    format!(r#"{{"kind":"page","page":{}}}"#, json!(page))
                }
                DocumentIdentity::Item { item } => format!(
                    r#"{{"kind":"item","item":{{"package":{},"item":{}}}}}"#,
                    json!(item.package),
                    json!(item.item)
                ),
                DocumentIdentity::Concept { concept } => {
                    format!(r#"{{"kind":"concept","concept":{}}}"#, json!(concept))
                }
            };
            assert_record("document", "", &id, &json!(document));
        }
        let stored_count: i64 = db
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_count, count);
    });
}

#[test]
fn documented_sql_examples_query_published_records() {
    with_snapshot(|snapshot, db| {
        let mut packages = db.prepare(example("Package metadata", "sql")).unwrap();
        let rows: Vec<(String, String, Option<String>)> = packages
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            snapshot
                .workspace()
                .packages
                .iter()
                .map(|(id, package)| (id.clone(), package.name.clone(), package.version.clone()))
                .collect::<Vec<_>>()
        );

        let key = ["pyfoo", "sid1:python:function:foo.model.fit"];
        let (content, fingerprint): (String, String) = db
            .query_row(example("Item lookup", "sql"), key, |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&content).unwrap(),
            json!(snapshot.workspace().packages[key[0]].items[key[1]])
        );
        assert_eq!(fingerprint.len(), 64);

        let (owner, id, references): (String, String, String) = db
            .query_row(example("Item documentation", "sql"), key, |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!([owner.as_str(), id.as_str()], key);
        let document = snapshot
            .documents()
            .iter()
            .find(|document| {
                matches!(&document.document, DocumentIdentity::Item { item }
                    if item.package == key[0] && item.item == key[1])
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&references).unwrap(),
            json!(document.references)
        );

        let mut documents = db.prepare(example("Resolved documents", "sql")).unwrap();
        let rows: Vec<(String, String)> = documents
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut expected: Vec<_> = snapshot
            .documents()
            .iter()
            .map(|document| {
                (
                    serde_json::to_string(&document.document).unwrap(),
                    json!(document.references).to_string(),
                )
            })
            .collect();
        expected.sort();
        assert_eq!(rows, expected);

        let mut assets = db.prepare(example("Asset lookup", "sql")).unwrap();
        let rows: Vec<(String, String, i64)> = assets
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!rows.is_empty());
        assert_eq!(
            rows,
            snapshot
                .assets()
                .iter()
                .map(|(digest, asset)| (
                    digest.clone(),
                    asset.media_type.clone(),
                    i64::try_from(asset.bytes.len()).unwrap()
                ))
                .collect::<Vec<_>>()
        );
    });
}

fn assert_json_example<T: DeserializeOwned + Serialize>(heading: &str) {
    let value: Value = serde_json::from_str(example(heading, "json")).unwrap();
    let decoded: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
}

#[test]
fn documented_nested_json_preserves_the_typed_ir_shapes() {
    assert_json_example::<diplodocus::configuration::PresentationDefaults>("Presentation defaults");
    assert_json_example::<SourcedDocument>("Sourced document example");
    assert_json_example::<SourcedSignature>("Sourced signature example");
    assert_json_example::<ItemLanguageData>("Python language extension example");
    assert_json_example::<ItemLanguageData>("R language extension example");
}
