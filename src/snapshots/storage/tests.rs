use super::*;

#[test]
fn validation_failure_preserves_the_previous_file_and_cleans_staging() {
    let mut snapshot = Snapshot {
        workspace: Workspace {
            name: "Before refresh".into(),
            ..Workspace::default()
        },
        presentation: PresentationDefaults::default(),
        documents: Vec::new(),
        assets: BTreeMap::new(),
        producer: "test".into(),
        executions: BTreeMap::new(),
        validated_outputs: BTreeMap::new(),
    };
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("documentation.sqlite");
    snapshot.publish(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    let export = snapshot.canonical_export().unwrap();
    snapshot.workspace.name = "Failed refresh".into();
    snapshot.producer.clear();

    // Exercise both input validation and validation of a fully written database.
    // Calling the storage writer directly bypasses the public preflight check.
    for result in [snapshot.publish(&path), publish(&snapshot, &path)] {
        assert!(matches!(
            result,
            Err(SnapshotError::Invalid("missing producer"))
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(load(&path).unwrap().canonical_export().unwrap(), export);
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
    }

    snapshot.producer = "test".into();
    snapshot.publish(&path).unwrap();
    assert_eq!(
        load(&path).unwrap().canonical_export().unwrap(),
        snapshot.canonical_export().unwrap()
    );
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
}
