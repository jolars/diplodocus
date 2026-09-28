use super::*;

#[test]
fn unrelated_databases_and_invalid_sources_are_not_hidden() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("data.md");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch("CREATE TABLE data (value TEXT);")
        .unwrap();
    connection.close().unwrap();
    assert!(!is_snapshot(&database));
    std::fs::write(root.path().join("source.md"), "# Source\n").unwrap();
    assert!(!is_snapshot(&root.path().join("source.md")));
    std::fs::write(root.path().join("broken.md"), b"\xffinvalid source").unwrap();
    assert!(!is_snapshot(&root.path().join("broken.md")));
    assert!(!is_snapshot(&root.path().join("missing.md")));
    assert!(!is_snapshot(root.path()));
    assert!(!is_reserved(&root.path().join("database.sqlite/source.md")));
}

#[test]
fn marked_snapshots_are_excluded_even_when_their_records_are_invalid() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("broken.md");
    let connection = Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "application_id", SNAPSHOT_APPLICATION_ID)
        .unwrap();
    connection.close().unwrap();
    assert!(is_snapshot(&path));
}
