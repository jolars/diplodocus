mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn snapshot() -> Snapshot {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    Snapshot::from_sources(&sources, &resolved).unwrap()
}

#[test]
fn portable_snapshot_round_trip_needs_no_sources_or_sidecars() {
    let snapshot = snapshot();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    snapshot.publish(&path).unwrap();
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.workspace(), snapshot.workspace());
    assert_eq!(loaded.documents(), snapshot.documents());
    assert_eq!(loaded.assets(), snapshot.assets());
    assert_eq!(
        loaded.canonical_export().unwrap(),
        snapshot.canonical_export().unwrap()
    );
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 1);
    let db = rusqlite::Connection::open(&path).unwrap();
    let items: i64 = db
        .query_row(
            "SELECT count(*) FROM records WHERE kind = 'item'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        items as usize,
        loaded
            .workspace()
            .packages
            .values()
            .map(|p| p.items.len())
            .sum::<usize>()
    );
}

#[test]
fn rejects_unsupported_versions_missing_records_and_corrupt_assets() {
    for sql in [
        "UPDATE manifest SET storage_version = 1",
        "UPDATE manifest SET storage_version = 3",
        "UPDATE manifest SET ir_version = 2",
        "UPDATE manifest SET encoding_version = 2",
        "DELETE FROM records WHERE kind = 'workspace'",
        "DELETE FROM records WHERE kind = 'package'",
        "DELETE FROM records WHERE kind = 'document'",
        "DELETE FROM assets",
        "UPDATE assets SET bytes = X'00'",
        "UPDATE records SET fingerprint = 'bad' WHERE kind = 'page'",
        "UPDATE records SET content = '{}' WHERE kind = 'page'",
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("snapshot.sqlite");
        snapshot().publish(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        assert!(Snapshot::load(&path).is_err(), "accepted {sql}");
    }
}

#[test]
fn refresh_replaces_records_and_failed_publication_preserves_destination() {
    let root = support::acceptance_workspace();
    let config = root.path().join("workspace/diplodocus.toml");
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    let refresh = || {
        let sources = assemble_workspace(&config).unwrap();
        let resolved = resolve_workspace(&sources).unwrap();
        Snapshot::from_sources(&sources, &resolved)
            .unwrap()
            .publish(&path)
            .unwrap();
    };
    refresh();
    let before = Snapshot::load(&path).unwrap();
    root.write(
        "core/docs/temporary.md",
        "# Temporary\n\n[Download](temporary.txt)\n",
    );
    root.write("core/docs/temporary.txt", "temporary asset");
    refresh();
    assert_eq!(
        Snapshot::load(&path).unwrap().workspace().pages.len(),
        before.workspace().pages.len() + 1
    );
    assert_eq!(
        Snapshot::load(&path).unwrap().assets().len(),
        before.assets().len() + 1
    );
    root.remove("core/docs/temporary.md");
    root.remove("core/docs/temporary.txt");
    refresh();
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        before.canonical_export().unwrap()
    );
    let directory = target.path().join("occupied");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("sentinel"), "keep").unwrap();
    assert!(before.publish(&directory).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.join("sentinel")).unwrap(),
        "keep"
    );
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 2);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn executed_snapshot_restores_all_safe_alternatives_without_staging() {
    use diplodocus::assembly::WorkspaceExecution;
    use diplodocus::execution::{ExecutionDeadlines, ValidatedRepresentationRef};
    use diplodocus::validation::resolve_executed_workspace;
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Snapshots'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount='guide'\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n");
    root.write(
        "diplodocus.toml",
        format!(
            "{}\n[presentation]\ntitle='Executed results'\ndescription='Portable execution'\n",
            root.read("diplodocus.toml")
        ),
    );
    root.write(
        "guide/picture.svg",
        "<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>",
    );
    root.write("guide/index.qmd", "# Results {#results}\n\n```{python}\n#| include: false\nfrom IPython.display import display\ndisplay({'text/markdown': '![Nested](picture.svg)', 'text/html': '<p>Safe <strong>HTML</strong><img src=\"picture.svg\"></p>'}, raw=True)\n```\n\n```{python}\nprint('<script>literal</script>')\n```\n\n```{python}\ndisplay({'text/html': '<p><strong>Visible HTML</strong><img src=\"picture.svg\"></p>'}, raw=True)\n```\n\n```{python}\ndisplay({'text/markdown': '*visible Markdown*\\n\\n![Visible](picture.svg)'}, raw=True)\n```\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let executed = sources
        .execute(WorkspaceExecution {
            staging_parent: stage.path(),
            deadlines: ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        })
        .await
        .unwrap();
    let resolved = resolve_executed_workspace(&executed).unwrap();
    let snapshot = Snapshot::from_executed(&executed, &resolved).unwrap();
    let id = snapshot.workspace().pages.keys().next().unwrap().clone();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    executed.discard().unwrap();
    drop(root);
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 1);
    assert_eq!(
        loaded.presentation().title.as_deref(),
        Some("Executed results")
    );
    assert_eq!(
        loaded.presentation().description.as_deref(),
        Some("Portable execution")
    );
    assert_eq!(
        loaded.canonical_export().unwrap(),
        snapshot.canonical_export().unwrap()
    );
    assert_eq!(loaded.assets().len(), 1);
    let result = loaded.executed_page(&id).unwrap();
    assert_eq!(result.record().cells.len(), 4);
    let output = &result.record().cells[0].outputs[0];
    let mut markdown = false;
    let mut html = false;
    for index in 0..output.output.representations.len() {
        match result.representation(0, output.slot, index).unwrap() {
            ValidatedRepresentationRef::Markdown(value) => {
                assert_eq!(value.image_bindings().len(), 1);
                markdown = true;
            }
            ValidatedRepresentationRef::Html(value) => {
                assert_eq!(value.referenced_assets().count(), 1);
                html = true;
            }
            _ => {}
        }
    }
    assert!(markdown && html);
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    assert!(html.contains("<strong>Visible HTML</strong>"));
    assert!(html.contains("<em>visible Markdown</em>"));
    assert!(html.contains("&lt;script&gt;literal&lt;/script&gt;"));
    assert!(!html.contains("<script>literal</script>"));
    let mut export: serde_json::Value =
        serde_json::from_str(&snapshot.canonical_export().unwrap()).unwrap();
    replace_record(&path, &mut export, "execution", |page| {
        let hidden = page["slots"][0]["representations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["kind"] == "html")
            .unwrap();
        hidden["content"]["markup"] =
            "<img src=\"https://untrusted.test/image\" onerror=\"alert(1)\">".into();
    });
    assert!(Snapshot::load(&path).is_err());
}

#[test]
fn rejects_wal_databases_without_consulting_or_creating_sidecars() {
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot().publish(&path).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    drop(db);
    assert!(Snapshot::load(&path).is_err());
}

fn replace_record(
    path: &std::path::Path,
    export: &mut serde_json::Value,
    kind: &str,
    change: impl FnOnce(&mut serde_json::Value),
) {
    let record = export["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["kind"] == kind)
        .unwrap();
    change(&mut record["content"]);
    let key =
        serde_json::json!({"kind": record["kind"], "owner": record["owner"], "id": record["id"]});
    let value = serde_json::json!(["diplodocus/snapshot-record-v1", key, record["content"]]);
    let digest =
        diplodocus::provenance::fingerprint_bytes(&serde_json::to_vec(&value).unwrap()).value;
    record["fingerprint"] = digest.clone().into();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE records SET content=?1, fingerprint=?2 WHERE kind=?3 AND owner=?4 AND id=?5",
        rusqlite::params![
            serde_json::to_string(&record["content"]).unwrap(),
            digest,
            record["kind"].as_str().unwrap(),
            record["owner"].as_str().unwrap(),
            record["id"].as_str().unwrap()
        ],
    )
    .unwrap();
    let mut manifest = export.clone();
    for asset in manifest["assets"].as_array_mut().unwrap() {
        asset.as_object_mut().unwrap().remove("bytes_base64");
    }
    let digest =
        diplodocus::provenance::fingerprint_bytes(&serde_json::to_vec(&manifest).unwrap()).value;
    db.execute("UPDATE manifest SET content_fingerprint=?1", [digest])
        .unwrap();
}

#[test]
fn record_validation_rejects_forgery_even_with_recomputed_fingerprints() {
    for case in ["page-owner", "document-anchors", "external-script"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("snapshot.sqlite");
        let snapshot = snapshot();
        snapshot.publish(&path).unwrap();
        let mut export: serde_json::Value =
            serde_json::from_str(&snapshot.canonical_export().unwrap()).unwrap();
        match case {
            "page-owner" => replace_record(&path, &mut export, "page", |page| {
                page["owner"] = serde_json::json!({"kind":"package", "package":"missing"});
            }),
            "document-anchors" => replace_record(&path, &mut export, "document", |document| {
                document["anchors"] = serde_json::json!(["invented"]);
            }),
            "external-script" => replace_record(&path, &mut export, "document", |document| {
                let references = document["references"].as_array_mut().unwrap();
                if references.is_empty() {
                    references.push(serde_json::json!({"kind":"link", "spelling":"javascript:alert(1)", "target":{"kind":"external", "url":"javascript:alert(1)"}}));
                } else {
                    references[0]["target"] =
                        serde_json::json!({"kind":"external", "url":"javascript:alert(1)"});
                }
            }),
            _ => unreachable!(),
        }
        assert!(Snapshot::load(path).is_err(), "accepted {case}");
    }
}
