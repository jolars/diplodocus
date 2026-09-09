//! Shared integration-test helpers; each test target uses a different subset.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use snapbox::dir::DirRoot;
use snapbox::{Assert, Data, IntoData};

#[path = "support/acceptance.rs"]
mod acceptance;
#[allow(unused_imports)]
pub use acceptance::*;

pub struct TestWorkspace {
    root: DirRoot,
}

impl Default for TestWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl TestWorkspace {
    pub fn new() -> Self {
        Self {
            root: DirRoot::mutable_temp().expect("temporary workspace should be created"),
        }
    }

    pub fn from_fixture(relative: impl AsRef<Path>) -> Self {
        let fixture = fixture_path(relative);
        let root = DirRoot::mutable_temp()
            .expect("temporary workspace should be created")
            .with_template(&fixture)
            .expect("fixture should be copied into the temporary workspace");
        Self { root }
    }

    pub fn path(&self) -> &Path {
        self.root.path().expect("workspace should have a root")
    }

    pub fn read(&self, relative: impl AsRef<Path>) -> String {
        fs::read_to_string(self.path().join(relative)).expect("workspace file should be readable")
    }

    pub fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workspace directory should be created");
        }
        fs::write(path, contents).expect("workspace file should be written");
    }

    pub fn remove(&self, relative: impl AsRef<Path>) {
        fs::remove_file(self.path().join(relative)).expect("workspace file should be removed");
    }
}

pub fn acceptance_workspace() -> TestWorkspace {
    // Acceptance commands may write caches and output, so every test receives
    // a disposable copy rather than a path into the checked-in corpus.
    TestWorkspace::from_fixture("acceptance")
}

pub fn fixture_path(relative: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

pub fn load_fixture(relative: impl AsRef<Path>) -> String {
    fs::read_to_string(fixture_path(relative)).expect("fixture should be readable")
}

pub fn fixture_files(relative: impl AsRef<Path>) -> Vec<PathBuf> {
    files_under(&fixture_path(relative))
}

pub fn files_under(root: &Path) -> Vec<PathBuf> {
    tree_entries(root)
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .map(|entry| entry.path)
        .collect()
}

pub fn assert_json_golden(actual: &impl serde::Serialize, relative: impl AsRef<Path>) {
    let serialized = serde_json::to_string_pretty(actual).expect("serialize spike observation");
    assert!(!serialized.contains(env!("CARGO_MANIFEST_DIR")));
    assert_matches_golden(serialized, relative);
}

pub fn golden(relative: impl AsRef<Path>) -> Data {
    Data::read_from(&snapshot_path(relative), None).raw()
}

pub fn assert_matches_golden(actual: impl IntoData, relative: impl AsRef<Path>) {
    Assert::new()
        .action_env("SNAPSHOTS")
        .eq(actual, golden(relative));
}

pub fn assert_output_tree(expected: &Path, actual: &Path) {
    let expected_entries = tree_entries(expected);
    let actual_entries = tree_entries(actual);
    assert_eq!(actual_entries, expected_entries, "output tree paths differ");

    for entry in expected_entries {
        if entry.kind == EntryKind::File {
            let expected_contents = fs::read(expected.join(&entry.path))
                .expect("expected output file should be readable");
            let actual_contents =
                fs::read(actual.join(&entry.path)).expect("actual output file should be readable");
            assert_eq!(
                actual_contents,
                expected_contents,
                "output file differs: {}",
                entry.path.display()
            );
        }
    }
}

fn snapshot_path(relative: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(relative)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TreeEntry {
    path: PathBuf,
    kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntryKind {
    Directory,
    File,
    Symlink,
}

fn tree_entries(root: &Path) -> Vec<TreeEntry> {
    fn visit(root: &Path, directory: &Path, entries: &mut Vec<TreeEntry>) {
        let mut children = fs::read_dir(directory)
            .expect("output directory should be readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("output directory entries should be readable");
        children.sort_by_key(fs::DirEntry::file_name);

        for child in children {
            let path = child.path();
            let relative = path
                .strip_prefix(root)
                .expect("output entry should be below the output root")
                .to_path_buf();
            let file_type = child
                .file_type()
                .expect("output entry type should be readable");
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Symlink
            };
            entries.push(TreeEntry {
                path: relative,
                kind,
            });
            if kind == EntryKind::Directory {
                visit(root, &path, entries);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries.sort();
    entries
}
