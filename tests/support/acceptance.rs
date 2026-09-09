use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::{TestWorkspace, acceptance_workspace, files_under, fixture_path, load_fixture};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceRegistry {
    pub criteria: BTreeMap<String, Vec<String>>,
    pub cases: Vec<AcceptanceCase>,
    pub scenarios: Vec<AcceptanceScenario>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCase {
    pub id: String,
    pub config: String,
    pub overlay: Option<String>,
    pub changes: Vec<String>,
    pub check: Vec<ExpectedDiagnostic>,
    pub build: Vec<ExpectedDiagnostic>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedDiagnostic {
    pub code: String,
    pub severity: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceScenario {
    pub id: String,
    pub cases: Vec<String>,
    pub inputs: Vec<String>,
    pub action: String,
    pub expected: String,
    pub milestone: u8,
}

pub fn acceptance_registry() -> AcceptanceRegistry {
    serde_json::from_str(&load_fixture("acceptance/CASES.json"))
        .expect("the acceptance registry should be valid")
}

pub fn acceptance_case(id: &str) -> TestWorkspace {
    let registry = acceptance_registry();
    let case = registry
        .cases
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("unknown acceptance case: {id}"));
    materialize_case(case)
}

pub fn materialize_case(case: &AcceptanceCase) -> TestWorkspace {
    let workspace = acceptance_workspace();
    if let Some(overlay) = &case.overlay {
        assert!(is_relative_input(overlay));
        let root = fixture_path("acceptance-cases").join(overlay);
        assert_eq!(
            files_under(&root),
            case.changes.iter().map(PathBuf::from).collect::<Vec<_>>(),
            "the overlay must contain exactly its declared changes"
        );
        for relative in &case.changes {
            copy_overlay_file(&workspace, &root, Path::new(relative));
        }
    } else {
        assert!(case.changes.is_empty());
    }
    workspace
}

pub fn is_relative_input(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

pub fn copy_overlay_file(workspace: &TestWorkspace, overlay: &Path, relative: &Path) {
    assert!(is_relative_input(
        relative.to_str().expect("UTF-8 fixture path")
    ));
    let source = overlay
        .join(relative)
        .canonicalize()
        .expect("overlay source should exist");
    assert!(source.starts_with(overlay.canonicalize().unwrap()));
    let destination = workspace.path().join(relative);
    let mut existing = destination.as_path();
    // A dangling symlink must not disguise an out-of-workspace destination.
    while fs::symlink_metadata(existing)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        existing = existing.parent().expect("destination should have a parent");
    }
    assert!(
        existing
            .canonicalize()
            .unwrap()
            .starts_with(workspace.path().canonicalize().unwrap())
    );
    workspace.write(
        relative,
        fs::read(source).expect("overlay should be readable"),
    );
}

pub fn own_documentation_workspace() -> TestWorkspace {
    let workspace = TestWorkspace::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in ["diplodocus.toml", "devenv.lock"] {
        workspace.write(relative, fs::read(root.join(relative)).unwrap());
    }
    for directory in ["docs/guide", "docs/examples"] {
        for relative in files_under(&root.join(directory)) {
            let relative = Path::new(directory).join(relative);
            workspace.write(&relative, fs::read(root.join(&relative)).unwrap());
        }
    }
    workspace
}

pub fn fixture_configuration(workspace: &TestWorkspace, config: &str) -> toml::Value {
    toml::from_str(&workspace.read(config)).expect("fixture configuration should parse")
}

pub fn authored_sources(workspace: &TestWorkspace, config: &str) -> Vec<(PathBuf, String)> {
    let configuration = fixture_configuration(workspace, config);
    let config_dir = workspace
        .path()
        .join(config)
        .parent()
        .unwrap()
        .to_path_buf();
    let repositories = configuration["repository"].as_array().unwrap();
    let mut sources = Vec::new();
    if let Some(collections) = configuration.get("content").and_then(toml::Value::as_array) {
        for collection in collections {
            let repository = repositories
                .iter()
                .find(|repository| repository["id"] == collection["repository"])
                .unwrap();
            let root = config_dir
                .join(repository["path"].as_str().unwrap())
                .join(collection["path"].as_str().unwrap())
                .canonicalize()
                .unwrap();
            let format = collection["format"].as_str().unwrap();
            let extension = match format {
                "gfm" => "md",
                "qmd" => "qmd",
                _ => panic!("unknown fixture profile"),
            };
            for path in files_under(&root) {
                if path.extension().and_then(|extension| extension.to_str()) == Some(extension) {
                    sources.push((
                        root.join(path)
                            .strip_prefix(workspace.path().canonicalize().unwrap())
                            .unwrap()
                            .to_owned(),
                        format.to_owned(),
                    ));
                }
            }
        }
    }
    sources.sort();
    sources
}
