//! Shared integration-test helpers; each test target uses a different subset.

#![allow(dead_code)]

use std::collections::BTreeMap;
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

pub fn prepared_acceptance_workspace() -> TestWorkspace {
    let workspace = acceptance_workspace();
    prepare_acceptance_python(&workspace);
    workspace
}

#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct ApiDiscovery {
    pub items: BTreeMap<String, diplodocus::ir::Item>,
    pub metadata: Option<diplodocus::ir::PackageMetadata>,
    pub diagnostics: Vec<diplodocus::diagnostics::Diagnostic>,
    extraction: Option<diplodocus::ir::Provenance>,
}

impl ApiDiscovery {
    pub fn extraction(&self) -> &diplodocus::ir::Provenance {
        self.extraction.as_ref().expect("successful API discovery")
    }

    pub fn input_count(&self, repository: &str) -> usize {
        let diplodocus::ir::ProvenanceActivity::Extraction { inputs, .. } =
            &self.extraction().activity
        else {
            unreachable!()
        };
        inputs[repository].len()
    }
}

pub fn discover_api(config: &Path, package: &str) -> ApiDiscovery {
    use diplodocus::assembly::{AssemblyError, assemble_workspace};
    use diplodocus::ir::ProvenanceActivity;

    match assemble_workspace(config) {
        Ok(sources) => {
            let workspace = sources.workspace();
            let extraction = workspace.provenance.iter().find(|record| {
                matches!(&record.activity, ProvenanceActivity::Extraction { target, .. }
                    if target.package == package)
            });
            ApiDiscovery {
                items: workspace.packages[package].items.clone(),
                metadata: workspace.packages[package].metadata.clone(),
                diagnostics: workspace.diagnostics.iter().cloned().collect(),
                extraction: extraction.cloned(),
            }
        }
        Err(AssemblyError::Path(error)) => ApiDiscovery {
            diagnostics: vec![error.to_diagnostic("diplodocus.yaml".try_into().unwrap())],
            ..ApiDiscovery::default()
        },
        Err(AssemblyError::Diagnostics(diagnostics)) => ApiDiscovery {
            diagnostics,
            ..ApiDiscovery::default()
        },
        Err(error) => panic!("{error:?}"),
    }
}

pub fn discover_r_api(workspace: &TestWorkspace) -> ApiDiscovery {
    let directory = workspace.path().join("core");
    let mut config =
        diplodocus::configuration::load_configuration(directory.join("diplodocus.yaml")).unwrap();
    config.packages.retain(|package| package.id == "rfoo");
    config.packages[0].incremental.cache = diplodocus::configuration::IncrementalCache::Off;
    config.content.clear();
    config.concepts.clear();
    config.relationships.clear();
    let path = directory.join("r-extraction.yaml");
    workspace.write(&path, serde_saphyr::to_string(&config).unwrap());
    discover_api(&path, "rfoo")
}

fn prepare_acceptance_python(workspace: &TestWorkspace) {
    // Relocated workspaces copy the same prepared producer artifact. Compiling
    // twice can embed different build paths in otherwise equivalent binaries.
    static NATIVE: std::sync::OnceLock<(String, Vec<u8>)> = std::sync::OnceLock::new();
    let (name, bytes) = NATIVE.get_or_init(|| {
        let output = std::process::Command::new("python")
            .args([
                "-c",
                r#"
import os, pathlib, subprocess, sys, sysconfig
root = pathlib.Path(sys.argv[1])
destination = root / 'python/python/foo' / ('_native' + sysconfig.get_config_var('EXT_SUFFIX'))
arguments = [os.environ.get('CC', 'cc'), '-shared', '-fPIC',
             '-I' + sysconfig.get_path('include'),
             str(root / 'python/native/native.c'), '-o', str(destination)]
if sys.platform == 'darwin':
    arguments.extend(['-undefined', 'dynamic_lookup'])
subprocess.run(arguments, check=True)
sys.path.insert(0, str(root / 'python/python'))
from foo import NativeWorkspace, native_mean
assert NativeWorkspace(3).dimension == 3
assert native_mean([1.0, 3.0]) == 2.0
print(destination.name)
"#,
            ])
            .arg(workspace.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let name = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        let bytes = fs::read(workspace.path().join("python/python/foo").join(&name)).unwrap();
        (name, bytes)
    });
    workspace.write(Path::new("python/python/foo").join(name), bytes);
}

pub fn prepared_python_overloads_workspace() -> TestWorkspace {
    let workspace = TestWorkspace::from_fixture("python-overloads");
    workspace.write(
        "python/demo/native.py",
        "def native_value(value):\n    return value\n",
    );
    workspace
}

pub fn shared_r_workspace() -> TestWorkspace {
    let workspace = TestWorkspace::new();
    workspace.write("core/diplodocus.yaml", "project:\n  name: \"Shared R help\"\nrepositories:\n  - id: \"r\"\n    path: \"../r\"\npackages:\n  - id: \"rfoo\"\n    name: \"Shared help\"\n    slug: \"r\"\n    ecosystem: \"r\"\n    repository: \"r\"\n    path: \".\"\n    metadata-path: \"DESCRIPTION\"\n    targets:\n      - id: \"api\"\n        extractor: \"r\"\n        path: \".\"\n        role: \"public-api\"\n");
    workspace.write(
        "r/DESCRIPTION",
        "Package: sharedhelp\nVersion: 1.0.0\nTitle: Shared Help Fixtures\n\
        Description: Fixtures for shared help topics.\nLicense: MIT\n\
        Author: Fixture Authors\nMaintainer: Fixture Authors <fixtures@example.com>\n",
    );
    let fixture = fixture_path("r-shared-topics");
    for directory in ["R", "man"] {
        for file in files_under(&fixture.join(directory)) {
            workspace.write(
                Path::new("r").join(directory).join(&file),
                fs::read(fixture.join(directory).join(&file)).unwrap(),
            );
        }
    }
    workspace.write("r/NAMESPACE", fs::read(fixture.join("NAMESPACE")).unwrap());
    workspace
}

pub fn add_r_datasets(workspace: &TestWorkspace) {
    let fixture = fixture_path("r-datasets/man");
    for file in files_under(&fixture) {
        workspace.write(
            Path::new("r/man").join(&file),
            fs::read(fixture.join(file)).unwrap(),
        );
    }
}

pub fn add_r_rd_content(workspace: &TestWorkspace) {
    let fixture = fixture_path("r-rd-content");
    for directory in ["R", "man"] {
        for file in files_under(&fixture.join(directory)) {
            workspace.write(
                Path::new("r").join(directory).join(&file),
                fs::read(fixture.join(directory).join(file)).unwrap(),
            );
        }
    }
    workspace.write(
        "r/NAMESPACE",
        format!(
            "{}\n{}",
            workspace.read("r/NAMESPACE"),
            fs::read_to_string(fixture.join("NAMESPACE")).unwrap()
        ),
    );
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
    let mut value = serde_json::to_value(actual).expect("serialize golden observation");
    let serialized = serde_json::to_string(&value).unwrap();
    assert!(!serialized.contains(env!("CARGO_MANIFEST_DIR")));
    normalize_golden_tool_versions(&mut value);
    Assert::new()
        .action_env("SNAPSHOTS")
        .eq(Data::json(value), golden(relative).is_json());
}

/// Keep golden comparisons independent of tool releases, retaining their names
/// and parser settings. Dedicated provenance tests verify exact dependency pins.
pub fn normalize_golden_tool_versions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(tools) = object
                .get_mut("tools")
                .and_then(|value| value.as_object_mut())
            {
                for version in tools.values_mut().filter(|value| value.is_string()) {
                    *version = serde_json::json!("[TOOL_VERSION]");
                }
            }
            if let Some(parsers) = object
                .get_mut("parsers")
                .and_then(|value| value.as_object_mut())
            {
                for parser in parsers.values_mut() {
                    if let Some(version) = parser.get_mut("version") {
                        *version = serde_json::json!("[TOOL_VERSION]");
                    }
                }
            }
            for child in object.values_mut() {
                normalize_golden_tool_versions(child);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                normalize_golden_tool_versions(child);
            }
        }
        _ => {}
    }
}

pub fn normalize_build_tool_versions(tools: &mut BTreeMap<String, String>, names: &[&str]) {
    for name in names {
        let version = tools.get_mut(*name).expect("build tool should be recorded");
        // Verify the observation before making the golden independent of releases.
        assert_eq!(version, env!("CARGO_PKG_VERSION"), "version of {name}");
        *version = "[DIPLODOCUS_VERSION]".into();
    }
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

pub fn configuration_error_span(error: &serde_saphyr::Error) -> Option<diplodocus::ir::SourceSpan> {
    let span = error.location()?.span();
    let start = usize::try_from(span.byte_offset()?).ok()?;
    let length = usize::try_from(span.byte_len()?).ok()?;
    Some(diplodocus::ir::SourceSpan {
        start,
        end: start + length,
    })
}

pub fn merge_configurations(source: &str, additions: &str) -> String {
    let mut configuration: serde_json::Value = serde_saphyr::from_str(source).unwrap();
    let additions: serde_json::Map<String, serde_json::Value> =
        serde_saphyr::from_str(additions).unwrap();
    for (key, value) in additions {
        if let (Some(existing), Some(entries)) = (
            configuration
                .get_mut(&key)
                .and_then(serde_json::Value::as_array_mut),
            value.as_array(),
        ) {
            existing.extend(entries.iter().cloned());
        } else {
            configuration.as_object_mut().unwrap().insert(key, value);
        }
    }
    serde_saphyr::to_string(&configuration).unwrap()
}
