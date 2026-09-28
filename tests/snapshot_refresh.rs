mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, CommandError, ExtractOptions};
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::snapshots::{Snapshot, SnapshotError};
use diplodocus::validation::{resolve_item, resolve_workspace};
use rusqlite::{Connection, OpenFlags};

const CONFIG: &str = r#"
[project]
name = "Refresh"
[[repository]]
id = "main"
path = "repo"
[[package]]
id = "api"
name = "API"
slug = "api"
ecosystem = "python"
repository = "main"
path = "."
metadata_path = "pyproject.toml"
targets = [{ id = "api", extractor = "python", path = "foo", role = "public-api" }]
[[content]]
id = "guide"
owner = "api"
repository = "main"
path = "docs"
mount = "guide"
format = "qmd"
[content.execution]
mode = "never"
[[concept]]
id = "retained"
kind = "related"
members = [{ package = "api", item = "foo.keep" }]
"#;

const OBSOLETE_CONFIG: &str = r#"
[[repository]]
id = "old"
path = "old"
[[package]]
id = "old"
name = "Old API"
slug = "old"
ecosystem = "python"
repository = "old"
path = "."
metadata_path = "pyproject.toml"
targets = [{ id = "api", extractor = "python", path = "bar", role = "public-api" }]
[[content]]
id = "old-guide"
owner = "old"
repository = "old"
path = "docs"
mount = "guide"
format = "qmd"
[content.execution]
mode = "never"
[[concept]]
id = "obsolete"
kind = "equivalent"
members = [{ package = "api", item = "foo.remove" }, { package = "old", item = "bar.remove" }]
[[relationship]]
from = "api"
to = "old"
kind = "binds"
provenance = "explicit"
"#;

const SOURCE: &str = "def keep(x):\n    \"\"\"Keep this function.\"\"\"\n    return x\n\ndef remove():\n    \"\"\"Remove this function.\"\"\"\n    pass\n";
const INDEX: &str =
    "# Guide\n\n[`api::foo.keep`]\n\n[Shared](shared.bin) [Changing](changing.bin)\n";
const REMOVED_PAGE: &str =
    "# Removed\n\n[`api::foo.remove`]\n\n[Shared](shared.bin) [Orphan](orphan.bin)\n";

fn workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", format!("{CONFIG}{OBSOLETE_CONFIG}"));
    root.write(
        "repo/pyproject.toml",
        "[project]\nname='foo'\nversion='1.0.0'\n",
    );
    root.write("repo/foo/__init__.py", SOURCE);
    root.write(
        "old/pyproject.toml",
        "[project]\nname='bar'\nversion='1.0.0'\n",
    );
    root.write(
        "old/bar/__init__.py",
        "def remove():\n    \"\"\"Old API.\"\"\"\n    pass\n",
    );
    root.write("old/docs/index.qmd", "# Old guide\n\n[`old::bar.remove`]\n");
    root.write("repo/docs/index.qmd", INDEX);
    root.write("repo/docs/removed.qmd", REMOVED_PAGE);
    for (name, bytes) in [
        ("shared", "shared"),
        ("changing", "before"),
        ("orphan", "orphan"),
    ] {
        root.write(format!("repo/docs/{name}.bin"), bytes);
    }
    root
}

fn extract(root: &support::TestWorkspace, path: &Path) -> Result<(), CommandError> {
    commands::extract(ExtractOptions {
        config: root.path().join("diplodocus.toml"),
        output: Some(path.to_owned()),
    })
}

fn database(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn assert_only_snapshot(path: &Path) {
    let entries: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries, [path]);
    let db = database(path);
    let mut statement = db
        .prepare("SELECT name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(names, ["assets", "manifest", "records"]);
    assert_eq!(
        db.query_row("SELECT count(*) FROM manifest", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

fn refresh(root: &support::TestWorkspace, path: &Path) -> Snapshot {
    extract(root, path).unwrap();
    assert_only_snapshot(path);
    let loaded = Snapshot::load(path).unwrap();
    // A fresh assembly supplies the oracle independently of the previous database.
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let expected = Snapshot::from_sources(&sources, &resolved).unwrap();
    assert_eq!(
        loaded.canonical_export().unwrap(),
        expected.canonical_export().unwrap()
    );
    loaded
}

type RecordKey = (String, String, String);

fn records(path: &Path) -> BTreeMap<RecordKey, (String, String)> {
    database(path)
        .prepare("SELECT kind, owner, id, content, fingerprint FROM records")
        .unwrap()
        .query_map([], |row| {
            Ok((
                (row.get(0)?, row.get(1)?, row.get(2)?),
                (row.get(3)?, row.get(4)?),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_assets(snapshot: &Snapshot, contents: &[&str]) {
    let expected: BTreeSet<_> = contents
        .iter()
        .map(|bytes| fingerprint_bytes(bytes.as_bytes()).value)
        .collect();
    assert_eq!(
        snapshot.assets().keys().cloned().collect::<BTreeSet<_>>(),
        expected
    );
    for bytes in contents {
        assert_eq!(
            snapshot.assets()[&fingerprint_bytes(bytes.as_bytes()).value].bytes,
            bytes.as_bytes()
        );
    }
}

#[test]
fn repeated_refreshes_retain_ids_and_replace_deleted_entities_and_unused_assets() {
    let root = workspace();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    let before = refresh(&root, &path);
    let before_records = records(&path);
    let keep = resolve_item(before.workspace(), None, "api::foo.keep").unwrap();
    let remove = resolve_item(before.workspace(), None, "api::foo.remove").unwrap();
    let (page_id, _) = before
        .workspace()
        .pages
        .iter()
        .find(|(_, page)| page.title == "Guide")
        .unwrap();
    assert_assets(&before, &["shared", "before", "orphan"]);
    assert_eq!(before.workspace().relationships.len(), 1);

    for _ in 0..3 {
        refresh(&root, &path);
        assert_eq!(records(&path), before_records);
    }

    root.write(
        "diplodocus.toml",
        CONFIG
            .replace("slug = \"api\"", "slug = \"new-api\"")
            .replace("mount = \"guide\"", "mount = \"manual\""),
    );
    root.write(
        "repo/foo/__init__.py",
        "def keep(x, y=1):\n    \"\"\"Updated documentation.\"\"\"\n    return x + y\n",
    );
    root.write(
        "repo/docs/index.qmd",
        INDEX.replace("# Guide", "# Revised guide"),
    );
    root.write("repo/docs/changing.bin", "after");
    root.remove("repo/docs/removed.qmd");
    let after = refresh(&root, &path);
    let after_records = records(&path);
    assert_eq!(
        resolve_item(after.workspace(), None, "api::foo.keep").unwrap(),
        keep
    );
    assert_eq!(after.workspace().pages[page_id].title, "Revised guide");
    assert!(
        !after.workspace().packages["api"]
            .items
            .contains_key(&remove.item)
    );
    assert!(after.workspace().relationships.is_empty());
    assert_assets(&after, &["shared", "after"]);
    assert!(root.path().join("repo/docs/orphan.bin").exists());
    assert!(root.path().join("old/bar/__init__.py").exists());

    let removed_kinds: BTreeSet<_> = before_records
        .keys()
        .filter(|key| !after_records.contains_key(*key))
        .map(|key| key.0.as_str())
        .collect();
    assert_eq!(
        removed_kinds,
        BTreeSet::from([
            "repository",
            "package",
            "target",
            "item",
            "collection",
            "page",
            "concept",
            "document"
        ])
    );
    assert!(
        after_records
            .keys()
            .all(|key| before_records.contains_key(key))
    );
    for key in [
        ("item".into(), keep.package.clone(), keep.item.clone()),
        ("page".into(), "".into(), page_id.clone()),
        ("package".into(), "".into(), "api".into()),
        ("collection".into(), "".into(), "guide".into()),
    ] {
        assert_ne!(
            before_records[&key], after_records[&key],
            "retained entity was not updated: {key:?}"
        );
    }
    for _ in 0..3 {
        refresh(&root, &path);
        assert_eq!(records(&path), after_records);
    }

    root.write(
        "repo/docs/index.qmd",
        "# Revised guide\n\n[`api::foo.keep`]\n",
    );
    assert_assets(&refresh(&root, &path), &[]);
    root.write("diplodocus.toml", format!("{CONFIG}{OBSOLETE_CONFIG}"));
    root.write("repo/foo/__init__.py", SOURCE);
    root.write("repo/docs/index.qmd", INDEX);
    root.write("repo/docs/removed.qmd", REMOVED_PAGE);
    root.write("repo/docs/changing.bin", "before");
    let restored = refresh(&root, &path);
    assert_eq!(
        restored.canonical_export().unwrap(),
        before.canonical_export().unwrap()
    );
    assert_eq!(records(&path), before_records);
}

#[test]
fn refresh_discards_manual_database_edits_and_history() {
    let root = workspace();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    let original = refresh(&root, &path).canonical_export().unwrap();
    let original_records = records(&path);
    for sql in [
        "UPDATE records SET content = '{}' WHERE kind = 'page'",
        "DELETE FROM records WHERE kind = 'item'",
        "UPDATE assets SET bytes = X'00'",
        "INSERT INTO assets VALUES (printf('%064d', 0), 'application/octet-stream', X'01')",
        "UPDATE manifest SET storage_version = 0",
        "CREATE TABLE manual_notes (note TEXT); INSERT INTO manual_notes VALUES ('keep me'); CREATE TABLE snapshot_history AS SELECT * FROM records",
    ] {
        let before = fs::read(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(sql).unwrap();
        db.close().unwrap();
        assert_ne!(
            fs::read(&path).unwrap(),
            before,
            "mutation did not change database: {sql}"
        );
        assert_eq!(
            refresh(&root, &path).canonical_export().unwrap(),
            original,
            "{sql}"
        );
        assert_eq!(records(&path), original_records);
    }
}

#[test]
fn failed_refreshes_preserve_the_previous_snapshot_and_recover_from_current_sources() {
    let root = workspace();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    let before = refresh(&root, &path).canonical_export().unwrap();
    let bytes = fs::read(&path).unwrap();

    root.write("repo/docs/index.qmd", "[`missing-item`]\n");
    assert!(matches!(
        extract(&root, &path),
        Err(CommandError::Resolution(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        before
    );
    assert_only_snapshot(&path);

    root.write("repo/docs/index.qmd", "# Ready for publication\n");
    let journal = target.path().join("documentation.sqlite-journal");
    fs::write(&journal, "existing recovery state").unwrap();
    for _ in 0..2 {
        assert!(matches!(
            extract(&root, &path),
            Err(CommandError::Snapshot(SnapshotError::Io(_)))
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(&journal).unwrap(), b"existing recovery state");
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 2);
    }
    fs::remove_file(journal).unwrap();
    root.write("repo/docs/index.qmd", "# Latest source\n");
    let recovered = refresh(&root, &path);
    assert!(
        recovered
            .workspace()
            .pages
            .values()
            .any(|page| page.title == "Latest source")
    );
    assert_ne!(recovered.canonical_export().unwrap(), before);
}

#[test]
fn replacement_failure_removes_staging_and_allows_retry() {
    let root = workspace();
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("sentinel"), "keep").unwrap();
    for _ in 0..2 {
        assert!(matches!(
            extract(&root, &path),
            Err(CommandError::Snapshot(SnapshotError::Io(_)))
        ));
        assert_eq!(fs::read_to_string(path.join("sentinel")).unwrap(), "keep");
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    }
    fs::remove_file(path.join("sentinel")).unwrap();
    fs::remove_dir(&path).unwrap();
    refresh(&root, &path);
}

#[cfg(target_os = "linux")]
#[test]
fn refresh_replaces_generated_figures_and_removes_obsolete_execution_records() {
    let root = workspace();
    root.write(
        "diplodocus.toml",
        CONFIG.replace(
            "mode = \"never\"",
            "mode = \"execute\"\nengine = \"jupyter\"\nkernel = \"python3\"",
        ),
    );
    root.remove("repo/docs/removed.qmd");
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    let mut retained_page = None;
    let mut previous_digest = None;
    for radius in [3, 5] {
        root.write(
            "repo/docs/index.qmd",
            format!("# Figure\n\n```{{python}}\nfrom IPython.display import SVG, display\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='{radius}'/></svg>\"))\n```\n"),
        );
        extract(&root, &path).unwrap();
        assert_only_snapshot(&path);
        let loaded = Snapshot::load(&path).unwrap();
        assert_eq!(loaded.workspace().pages.len(), 1);
        let page = loaded.workspace().pages.keys().next().unwrap().clone();
        assert_eq!(retained_page.get_or_insert(page.clone()), &page);
        assert!(loaded.executed_page(&page).is_some());
        assert_eq!(loaded.assets().len(), 1);
        let digest = loaded.assets().keys().next().unwrap().clone();
        if let Some(previous) = previous_digest.replace(digest) {
            assert!(!loaded.assets().contains_key(&previous));
        }
        assert_eq!(
            records(&path)
                .keys()
                .filter(|key| key.0 == "execution")
                .count(),
            1
        );
    }

    root.write(
        "repo/docs/index.qmd",
        "# No figure\n\n```{python}\npass\n```\n",
    );
    extract(&root, &path).unwrap();
    assert_only_snapshot(&path);
    let loaded = Snapshot::load(&path).unwrap();
    let page = retained_page.unwrap();
    assert!(loaded.executed_page(&page).is_some());
    assert_assets(&loaded, &[]);

    root.write("diplodocus.toml", CONFIG);
    let disabled = refresh(&root, &path);
    assert!(disabled.workspace().pages.contains_key(&page));
    assert!(disabled.executed_page(&page).is_none());
    assert!(!records(&path).keys().any(|key| key.0 == "execution"));
    assert_assets(&disabled, &[]);
}
