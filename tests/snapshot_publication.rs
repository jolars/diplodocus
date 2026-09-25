mod support;

use std::fs;
use std::path::{Path, PathBuf};

use diplodocus::assembly::assemble_workspace;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use rusqlite::{Connection, OpenFlags};

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    Snapshot::from_sources(&sources, &resolved).unwrap()
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn entries(path: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn refuses_destination_sidecars_without_changing_the_previous_snapshot() {
    let root = support::acceptance_workspace();
    let before = snapshot(&root);
    root.write("core/docs/new.md", "# New page\n");
    let after = snapshot(&root);
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    before.publish(&path).unwrap();
    let bytes = fs::read(&path).unwrap();

    for suffix in ["-journal", "-wal", "-shm"] {
        let companion = sidecar(&path, suffix);
        fs::write(&companion, "existing SQLite state").unwrap();
        let result = after.publish(&path);
        assert!(result.is_err(), "publication accepted {suffix}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(&companion).unwrap(), b"existing SQLite state");
        assert_eq!(entries(target.path()).len(), 2);
        fs::remove_file(companion).unwrap();
        assert_eq!(
            Snapshot::load(&path).unwrap().canonical_export().unwrap(),
            before.canonical_export().unwrap()
        );
    }

    after.publish(&path).unwrap();
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        after.canonical_export().unwrap()
    );
    assert_eq!(entries(target.path()), [path]);
}

#[cfg(unix)]
#[test]
fn dangling_sidecar_symlinks_prevent_first_publication() {
    use std::os::unix::fs::symlink;

    let root = support::acceptance_workspace();
    let snapshot = snapshot(&root);
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    for suffix in ["-journal", "-wal", "-shm"] {
        let companion = sidecar(&path, suffix);
        symlink("missing", &companion).unwrap();
        assert!(snapshot.publish(&path).is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_link(&companion).unwrap(), Path::new("missing"));
        assert_eq!(entries(target.path()), std::slice::from_ref(&companion));
        fs::remove_file(companion).unwrap();
    }
    snapshot.publish(&path).unwrap();
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        snapshot.canonical_export().unwrap()
    );
}

#[test]
fn refuses_a_destination_with_an_active_sqlite_transaction() {
    let root = support::acceptance_workspace();
    let before = snapshot(&root);
    root.write("core/docs/new.md", "# New page\n");
    let after = snapshot(&root);
    for mode in ["DELETE", "WAL"] {
        let target = tempfile::tempdir().unwrap();
        let path = target.path().join("documentation.sqlite");
        before.publish(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "journal_mode", mode).unwrap();
        db.execute_batch("BEGIN IMMEDIATE; UPDATE manifest SET producer = 'pending';")
            .unwrap();
        let files = entries(target.path());
        assert!(files.len() > 1);
        let bytes: Vec<_> = files.iter().map(|path| fs::read(path).unwrap()).collect();

        assert!(after.publish(&path).is_err(), "publication accepted {mode}");
        assert_eq!(entries(target.path()), files);
        for (path, expected) in files.iter().zip(bytes) {
            assert_eq!(fs::read(path).unwrap(), expected);
        }
        db.execute_batch("ROLLBACK;").unwrap();
        db.pragma_update(None, "journal_mode", "DELETE").unwrap();
        db.close().unwrap();
        assert_eq!(
            Snapshot::load(&path).unwrap().canonical_export().unwrap(),
            before.canonical_export().unwrap()
        );
        after.publish(&path).unwrap();
        assert_eq!(
            Snapshot::load(&path).unwrap().canonical_export().unwrap(),
            after.canonical_export().unwrap()
        );
        assert_eq!(entries(target.path()), [path]);
    }
}

#[test]
fn publication_is_a_complete_standalone_database() {
    let root = support::acceptance_workspace();
    let snapshot = snapshot(&root);
    let expected = snapshot.canonical_export().unwrap();
    assert!(!snapshot.workspace().packages.is_empty());
    assert!(!snapshot.workspace().pages.is_empty());
    assert!(!snapshot.documents().is_empty());
    assert!(!snapshot.assets().is_empty());
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("nested/documentation.sqlite");
    snapshot.publish(&path).unwrap();
    assert_eq!(entries(path.parent().unwrap()), std::slice::from_ref(&path));
    let header = fs::read(&path).unwrap();
    assert_eq!(&header[..16], b"SQLite format 3\0");
    assert_eq!(&header[18..20], [1, 1]);

    let copy = tempfile::tempdir().unwrap();
    let copied = copy.path().join("copied.sqlite");
    fs::copy(&path, &copied).unwrap();
    drop((snapshot, root, target));
    let db = Connection::open_with_flags(&copied, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    db.close().unwrap();
    assert_eq!(
        Snapshot::load(&copied).unwrap().canonical_export().unwrap(),
        expected
    );
    assert_eq!(entries(copy.path()), [copied]);
}

#[cfg(unix)]
#[test]
fn an_open_reader_keeps_the_previous_complete_database_during_replacement() {
    let root = support::acceptance_workspace();
    let before = snapshot(&root);
    root.write("core/docs/new.md", "# New page\n\n[Download](new.txt)\n");
    root.write("core/docs/new.txt", "new asset");
    let after = snapshot(&root);
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    before.publish(&path).unwrap();

    let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    reader.execute_batch("BEGIN;").unwrap();
    let dump = |db: &Connection| {
        let mut values = Vec::new();
        for table in ["manifest", "records", "assets"] {
            let mut statement = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2, 3"))
                .unwrap();
            let count = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..count)
                        .map(|i| row.get::<_, rusqlite::types::Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap();
            values.push(rows.collect::<rusqlite::Result<Vec<_>>>().unwrap());
        }
        values
    };
    let old_records = dump(&reader);
    after.publish(&path).unwrap();
    assert_eq!(dump(&reader), old_records);
    assert_eq!(
        Snapshot::load(&path).unwrap().canonical_export().unwrap(),
        after.canonical_export().unwrap()
    );
    let new_reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_ne!(dump(&new_reader), old_records);
    reader.execute_batch("COMMIT;").unwrap();
    assert_eq!(entries(target.path()), [path]);
}
