use super::*;
use crate::configuration::ExecutionMode;
use crate::documents::AuthoredFormat;
use crate::ir::SourceLocation;

fn store(root: &std::path::Path) -> PageAssetStore {
    PageAssetStore::new(
        ExecutionPage {
            source: SourceLocation {
                repository: "repo".into(),
                path: DiagnosticPath::try_from("page.qmd").unwrap(),
                span: None,
            },
            collection: "guide".into(),
            working_directory: None,
            source_fingerprint: fingerprint_bytes(b"source"),
            format: AuthoredFormat::Qmd,
            mode: ExecutionMode::Execute,
            page_veto: false,
            parser_version: "0.29.2".into(),
            qmd_policy: "qmd-mvp-v1".into(),
        },
        root.to_owned(),
        root.join("staging"),
    )
    .unwrap()
}

#[test]
fn injected_digest_collision_rejects_distinct_bytes_and_poisons_retention() {
    let root = tempfile::tempdir().unwrap();
    let mut store = store(root.path());
    store.digest = |_| fingerprint_bytes(b"collision");
    let first = store.stage_bytes("image/svg+xml", b"<svg/>").unwrap();
    let directory = store.directory.as_ref().unwrap().path().to_owned();
    assert_eq!(
        store.stage_bytes("image/svg+xml", b"<svg><rect/></svg>"),
        Err(AssetError::Collision)
    );
    let failure = store.retain(&[first.reference]).unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::AssetCollision);
    assert!(!directory.exists());
}

#[test]
fn inconsistent_media_and_unregistered_digest_files_are_collisions() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = store(root.path());
    let record = owner.stage_bytes("image/svg+xml", b"<svg/>").unwrap();
    owner
        .assets
        .get_mut(&record.reference.fingerprint.value)
        .unwrap()
        .media_type = "image/png".into();
    assert_eq!(
        owner.stage_bytes("image/svg+xml", b"<svg/>"),
        Err(AssetError::Collision)
    );
    drop(owner);

    let mut owner = store(root.path());
    let directory = owner.staging_directory().unwrap();
    let path = directory.join(fingerprint_bytes(b"<svg/>").value);
    fs::write(&path, b"do not overwrite").unwrap();
    assert_eq!(
        owner.stage_bytes("image/svg+xml", b"<svg/>"),
        Err(AssetError::Collision)
    );
    assert_eq!(fs::read(path).unwrap(), b"do not overwrite");
    owner.rollback().unwrap();
}

#[test]
fn retention_rechecks_staged_content_and_removes_the_attempt() {
    for change in ["alter", "missing", "extra"] {
        let root = tempfile::tempdir().unwrap();
        let mut owner = store(root.path());
        let record = owner.stage_bytes("image/svg+xml", b"<svg/>").unwrap();
        let directory = owner.directory.as_ref().unwrap().path().to_owned();
        let path = directory.join(&record.reference.fingerprint.value);
        match change {
            "alter" => fs::write(path, b"<svg> </svg>").unwrap(),
            "missing" => fs::remove_file(path).unwrap(),
            "extra" => fs::write(directory.join("unknown"), b"x").unwrap(),
            _ => unreachable!(),
        }
        assert!(owner.retain(&[record.reference]).is_err(), "{change}");
        assert!(!directory.exists());
    }
}

#[cfg(unix)]
#[test]
fn replaced_staging_boundary_reports_cleanup_without_following_it() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let mut owner = store(root.path());
    owner.stage_bytes("image/svg+xml", b"<svg/>").unwrap();
    let private_name = owner
        .directory
        .as_ref()
        .unwrap()
        .path()
        .file_name()
        .unwrap()
        .to_owned();
    fs::rename(root.path().join("staging"), root.path().join("moved")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join(&private_name)).unwrap();
    let sentinel = outside.path().join(&private_name).join("sentinel");
    fs::write(&sentinel, b"keep").unwrap();
    symlink(outside.path(), root.path().join("staging")).unwrap();
    assert_eq!(owner.rollback(), Err(AssetError::Cleanup));
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn replaced_private_directory_is_never_deleted_as_owned_staging() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = store(root.path());
    owner.stage_bytes("image/svg+xml", b"<svg/>").unwrap();
    let path = owner.directory.as_ref().unwrap().path().to_owned();
    fs::rename(&path, root.path().join("moved")).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("sentinel"), b"keep").unwrap();
    assert_eq!(owner.rollback(), Err(AssetError::Cleanup));
    assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"keep");
}
