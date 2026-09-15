mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use diplodocus::configuration::{WorkspaceConfiguration, parse_configuration};
use diplodocus::diagnostics::{DiagnosticCode, DiagnosticPath};
use diplodocus::ir::{SourceSpan, TargetReference};
use diplodocus::paths::{PathResolutionErrorKind, resolve_workspace_paths};
use diplodocus::provenance::{
    BuiltinExtractor, DeclaredSourceInputs, ExecutionObservation, ExtractionObservation,
    PANACHE_VERSION, builtin_tools, collect_static_provenance, fingerprint_bytes,
    observe_repository,
};
use support::TestWorkspace;

const CONFIG: &str = r#"
[project]
name = "Evidence"

[[repository]]
id = "repo"
path = "repo"
url = "https://example.org/source"

[[package]]
id = "pkg"
name = "Package"
slug = "pkg"
ecosystem = "python"
repository = "repo"
path = "pkg"
metadata_path = "pyproject.toml"
targets = [{ id = "api", extractor = "python", path = "src", role = "public-api" }]

[[content]]
id = "guide"
owner = "project"
repository = "repo"
path = "docs"
mount = "guide"
format = "qmd"
[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared_environment_inputs = ["uv.lock", "requirements.txt"]
"#;

fn workspace(reverse: bool) -> (TestWorkspace, WorkspaceConfiguration, PathBuf) {
    let workspace = TestWorkspace::new();
    let mut files = vec![
        ("repo/pkg/pyproject.toml", "[project]\nname = 'pkg'\n"),
        ("repo/pkg/src/api.py", "def api(): pass\n"),
        ("repo/pkg/src/api.pyi", "def api() -> None: ...\n"),
        ("repo/docs/index.qmd", "# Guide\n"),
        ("repo/uv.lock", "locked packages\n"),
        ("repo/requirements.txt", "example==1.0\n"),
    ];
    if reverse {
        files.reverse();
    }
    for (path, bytes) in files {
        workspace.write(path, bytes);
    }
    let config_path = workspace.path().join("diplodocus.toml");
    (workspace, parse_configuration(CONFIG).unwrap(), config_path)
}

fn selection(reverse: bool) -> DeclaredSourceInputs {
    let mut files = vec![PathBuf::from("src/api.py"), PathBuf::from("src/api.pyi")];
    if reverse {
        files.reverse();
    }
    DeclaredSourceInputs {
        extraction: BTreeMap::from([(
            TargetReference {
                package: "pkg".into(),
                target: "api".into(),
            },
            files,
        )]),
        content: BTreeMap::from([("guide".into(), vec!["index.qmd".into()])]),
    }
}

#[test]
fn raw_fingerprints_match_sha256_vectors_without_normalization() {
    let empty = fingerprint_bytes(b"");
    assert_eq!(empty.algorithm, "sha256");
    assert_eq!(
        empty.value,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        fingerprint_bytes(b"abc").value,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(fingerprint_bytes(b"a\n"), fingerprint_bytes(b"a\r\n"));
}

#[test]
fn relocated_workspaces_have_identical_portable_evidence() {
    let (first, config, path) = workspace(false);
    let (second, mut other_config, other_path) = workspace(true);
    other_config.content[0]
        .execution
        .declared_environment_inputs
        .reverse();
    let left = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    let right = collect_static_provenance(&other_path, &other_config, &selection(true)).unwrap();
    assert_eq!(left, right);
    let bytes = serde_json::to_string_pretty(&left).unwrap();
    assert_eq!(bytes, serde_json::to_string_pretty(&right).unwrap());
    for root in [
        first.path(),
        second.path(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    ] {
        assert!(!bytes.contains(root.to_str().unwrap()));
    }
    assert!(!bytes.contains("locked packages"));
    assert!(!bytes.contains("example==1.0"));
    assert_eq!(left.repositories["repo"].revision, None);
    assert_eq!(left.repositories["repo"].dirty, None);
    assert!(
        left.repositories["repo"]
            .declared_input_fingerprint
            .is_some()
    );
    assert_eq!(left.inputs["repo"].len(), 6);
    support::assert_json_golden(&left, "provenance/static.json");
}

#[test]
fn only_selected_content_and_declared_environment_affect_fingerprints() {
    let (workspace, config, path) = workspace(false);
    let original = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    workspace.write("repo/pkg/src/ignored.py", "not selected");
    workspace.write("repo/docs/ignored.qmd", "not selected");
    let irrelevant = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    assert_eq!(original, irrelevant);
    for input in [
        "repo/pkg/src/api.py",
        "repo/pkg/pyproject.toml",
        "repo/docs/index.qmd",
        "repo/uv.lock",
    ] {
        let bytes = fs::read(workspace.path().join(input)).unwrap();
        workspace.write(input, b"changed");
        let changed = collect_static_provenance(&path, &config, &selection(false)).unwrap();
        assert_ne!(
            original.repositories["repo"].declared_input_fingerprint,
            changed.repositories["repo"].declared_input_fingerprint,
            "{input}"
        );
        workspace.write(input, bytes);
    }
}

#[test]
fn incomplete_selection_is_unknown_and_explicit_empty_selection_is_complete() {
    let (_workspace, config, path) = workspace(false);
    let unknown =
        collect_static_provenance(&path, &config, &DeclaredSourceInputs::default()).unwrap();
    assert_eq!(
        unknown.repositories["repo"].declared_input_fingerprint,
        None
    );
    assert_eq!(unknown.inputs["repo"].len(), 3);
    assert_eq!(unknown.declared_environment_inputs["guide"].len(), 2);
    let mut inputs = selection(false);
    inputs.extraction.values_mut().for_each(Vec::clear);
    inputs.content.values_mut().for_each(Vec::clear);
    let empty = collect_static_provenance(&path, &config, &inputs).unwrap();
    assert!(
        empty.repositories["repo"]
            .declared_input_fingerprint
            .is_some()
    );
    assert_eq!(empty.inputs["repo"].len(), 3);
}

#[test]
fn source_conversion_rechecks_containment_and_retains_proven_ranges() {
    let (workspace, config, path) = workspace(false);
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let repo = &resolved.repositories[0];
    let span = Some(SourceSpan { start: 0, end: 3 });
    let source = repo
        .source_location(repo.path.join("pkg/src/../src/api.py"), span)
        .unwrap();
    assert_eq!(source.repository, "repo");
    assert_eq!(source.path.as_str(), "pkg/src/api.py");
    assert_eq!(source.span, span);
    workspace.write("outside.py", "outside");
    for input in [
        workspace.path().join("outside.py"),
        repo.path.join("../outside.py"),
        repo.path.join("pkg/../../../repo/pkg/src/api.py"),
    ] {
        let error = repo.source_location(input, None).unwrap_err();
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::OutsideBoundary { .. }
        ));
        let diagnostic = error.to_diagnostic(DiagnosticPath::try_from("diplodocus.toml").unwrap());
        assert_eq!(diagnostic.code, DiagnosticCode::SourcePathOutsideBoundary);
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains(workspace.path().to_str().unwrap())
        );
    }
    assert!(repo.source_location(&repo.path, None).is_err());
    assert!(
        repo.source_location(repo.path.join("absent.py"), None)
            .is_err()
    );
}

#[test]
fn selection_rejects_unknown_owners_and_package_or_collection_escapes() {
    let (_workspace, config, path) = workspace(false);
    let mut inputs = selection(false);
    inputs.content.insert("unknown".into(), Vec::new());
    assert!(collect_static_provenance(&path, &config, &inputs).is_err());
    let mut inputs = selection(false);
    *inputs.extraction.values_mut().next().unwrap() = vec!["../uv.lock".into()];
    assert!(collect_static_provenance(&path, &config, &inputs).is_err());
    let mut inputs = selection(false);
    inputs
        .content
        .insert("guide".into(), vec!["../uv.lock".into()]);
    assert!(collect_static_provenance(&path, &config, &inputs).is_err());
}

#[test]
fn declared_revisions_win_without_changing_content_fingerprints() {
    let (_workspace, mut config, path) = workspace(false);
    let original = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    config.repositories[0].revision = Some("release-v2".into());
    let declared = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    assert_eq!(
        declared.repositories["repo"].revision.as_deref(),
        Some("release-v2")
    );
    assert_eq!(
        declared.revisions["repo"].declared.as_deref(),
        Some("release-v2")
    );
    assert_eq!(declared.revisions["repo"].observed, None);
    assert_eq!(
        original.repositories["repo"].declared_input_fingerprint,
        declared.repositories["repo"].declared_input_fingerprint
    );
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Provenance Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.org")
        .env("GIT_COMMITTER_NAME", "Provenance Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.org")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn observes_git_head_and_clean_dirty_and_unborn_states_without_writes() {
    let (workspace, config, path) = workspace(false);
    let repo_path = workspace.path().join("repo");
    git(&repo_path, &["init", "--quiet"]);
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let repo = &resolved.repositories[0];
    let unborn = observe_repository(repo, None);
    assert_eq!(unborn.observed, None);
    assert_eq!(unborn.dirty, Some(true));
    git(&repo_path, &["add", "."]);
    git(
        &repo_path,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let expected = git(&repo_path, &["rev-parse", "HEAD"]);
    let index_before = fs::read(repo_path.join(".git/index")).unwrap();
    let clean = observe_repository(repo, Some("declared-tag"));
    assert_eq!(clean.observed.as_deref(), Some(expected.as_str()));
    assert_eq!(clean.declared.as_deref(), Some("declared-tag"));
    assert_eq!(clean.dirty, Some(false));
    assert_eq!(
        fs::read(repo_path.join(".git/index")).unwrap(),
        index_before
    );
    workspace.write("repo/pkg/src/api.py", "changed");
    assert_eq!(observe_repository(repo, None).dirty, Some(true));
    git(&repo_path, &["checkout", "--", "pkg/src/api.py"]);
    workspace.write("repo/untracked", "new");
    assert_eq!(observe_repository(repo, None).dirty, Some(true));
}

#[test]
fn missing_promisor_objects_never_trigger_transport() {
    let (workspace, config, path) = workspace(false);
    let root = workspace.path().join("repo");
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let head = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["config", "extensions.partialClone", "origin"]);
    git(&root, &["config", "remote.origin.promisor", "true"]);
    git(
        &root,
        &["config", "remote.origin.partialCloneFilter", "blob:none"],
    );
    git(
        &root,
        &["config", "remote.origin.url", "ssh://example.invalid/repo"],
    );
    // The fake transport records invocation and exits before any network access.
    git(
        &root,
        &["config", "core.sshCommand", "touch lazy-fetch-ran; false"],
    );
    let object = root.join(".git/objects").join(&head[..2]).join(&head[2..]);
    fs::rename(&object, root.join(".git/missing-head")).unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let observation = observe_repository(&resolved.repositories[0], Some("declared"));
    assert!(
        !root.join("lazy-fetch-ran").exists(),
        "executed lazy fetch transport"
    );
    assert_eq!(observation.declared.as_deref(), Some("declared"));
    assert_eq!(observation.observed, None);
    assert_eq!(observation.dirty, None);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert!(!object.exists());
}

#[test]
fn repository_content_filters_are_never_executed() {
    for filter in ["clean", "process"] {
        for configuration in ["local", "included", "worktree"] {
            let (workspace, config, path) = workspace(false);
            let root = workspace.path().join("repo");
            workspace.write("repo/.gitattributes", "*.py filter=provenance-test\n");
            git(&root, &["init", "--quiet"]);
            git(&root, &["add", "."]);
            git(
                &root,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--quiet",
                    "-m",
                    "fixture",
                ],
            );
            let head = git(&root, &["rev-parse", "HEAD"]);
            let key = format!("filter.provenance-test.{filter}");
            let command = if filter == "clean" {
                "touch filter-ran; cat"
            } else {
                "touch filter-ran; exit 1"
            };
            if configuration == "included" {
                git(
                    &root,
                    &["config", "--file", ".git/filter-config", &key, command],
                );
                git(&root, &["config", "include.path", "filter-config"]);
            } else if configuration == "worktree" {
                git(&root, &["config", "extensions.worktreeConfig", "true"]);
                git(&root, &["config", "--worktree", &key, command]);
            } else {
                git(&root, &["config", &key, command]);
            }
            workspace.write("repo/pkg/src/api.py", "changed content\n");
            let index = fs::read(root.join(".git/index")).unwrap();
            let resolved = resolve_workspace_paths(&path, &config).unwrap();
            let observation = observe_repository(&resolved.repositories[0], Some("declared"));
            assert!(
                !root.join("filter-ran").exists(),
                "executed {filter}, configuration={configuration}"
            );
            assert_eq!(observation.dirty, None);
            assert_eq!(observation.observed.as_deref(), Some(head.as_str()));
            assert_eq!(observation.declared.as_deref(), Some("declared"));
            assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        }
    }
}

#[test]
fn submodule_content_filters_are_never_executed() {
    let (workspace, config, path) = workspace(false);
    let root = workspace.path().join("repo");
    let nested = root.join("dependency");
    workspace.write("repo/dependency/file.py", "original\n");
    workspace.write(
        "repo/dependency/.gitattributes",
        "*.py filter=provenance-test\n",
    );
    git(&nested, &["init", "--quiet"]);
    git(&nested, &["add", "."]);
    git(
        &nested,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "nested fixture",
        ],
    );
    workspace.write(
        "repo/.gitmodules",
        "[submodule \"dependency\"]\npath = dependency\nurl = ./dependency\n",
    );
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    git(
        &nested,
        &[
            "config",
            "filter.provenance-test.clean",
            "touch filter-ran; cat",
        ],
    );
    workspace.write("repo/dependency/file.py", "changed content\n");
    let index = fs::read(root.join(".git/index")).unwrap();
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let observation = observe_repository(&resolved.repositories[0], None);
    assert!(
        !nested.join("filter-ran").exists(),
        "executed submodule filter"
    );
    assert_eq!(observation.dirty, None);
    assert!(observation.observed.is_some());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}

#[test]
fn a_nested_non_git_root_does_not_inherit_its_parents_revision() {
    let (workspace, config, path) = workspace(false);
    git(workspace.path(), &["init", "--quiet"]);
    git(workspace.path(), &["add", "."]);
    git(
        workspace.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "parent",
        ],
    );
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let observation = observe_repository(&resolved.repositories[0], None);
    assert_eq!(observation.observed, None);
    assert_eq!(observation.dirty, None);
}

#[test]
fn available_component_versions_match_the_build_dependencies() {
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert_eq!(
        manifest["dependencies"]["panache-parser"].as_str(),
        Some(format!("={PANACHE_VERSION}").as_str())
    );
    let tools = builtin_tools();
    assert_eq!(tools["diplodocus"], env!("CARGO_PKG_VERSION"));
    assert_eq!(tools["panache-parser"], PANACHE_VERSION);
    assert_eq!(
        tools.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["diplodocus", "panache-parser"])
    );
}

#[cfg(unix)]
#[test]
fn symlink_paths_preserve_declared_environment_names_and_reject_escapes() {
    use std::os::unix::fs::symlink;
    let (workspace, mut config, path) = workspace(false);
    symlink("uv.lock", workspace.path().join("repo/env-link")).unwrap();
    config.content[0].execution.declared_environment_inputs = vec!["./env-link".into()];
    let evidence = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    assert_eq!(
        evidence.declared_environment_inputs["guide"][0]
            .source
            .path
            .as_str(),
        "env-link"
    );
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let repo = &resolved.repositories[0];
    assert_eq!(
        repo.source_location(repo.path.join("env-link"), None)
            .unwrap()
            .path
            .as_str(),
        "uv.lock"
    );
    workspace.write("outside", "secret");
    fs::remove_file(workspace.path().join("repo/env-link")).unwrap();
    symlink("../outside", workspace.path().join("repo/env-link")).unwrap();
    assert!(
        repo.source_location(repo.path.join("env-link"), None)
            .is_err()
    );
    assert!(collect_static_provenance(&path, &config, &selection(false)).is_err());
}

#[test]
fn duplicate_normalized_environment_inputs_are_rejected() {
    let (_workspace, mut config, path) = workspace(false);
    config.content[0].execution.declared_environment_inputs =
        vec!["uv.lock".into(), "./uv.lock".into()];
    assert!(collect_static_provenance(&path, &config, &selection(false)).is_err());
}

#[test]
fn producers_supply_extraction_and_execution_observations_explicitly() {
    use diplodocus::ir::{
        ExecutionEngine, ExecutionMode, ExecutionOrigin, ExtractionInput, ExtractionMode,
        KernelProvenance, ParserProvenance, ProvenanceActivity,
    };
    let parser = ParserProvenance {
        version: "0.0.12".into(),
        role: "python-syntax".into(),
        settings: BTreeMap::from([("target_version".into(), "py310".into())]),
    };
    let extraction = ExtractionObservation {
        target: TargetReference {
            package: "pkg".into(),
            target: "api".into(),
        },
        extractor: BuiltinExtractor::Python,
        capabilities: BTreeSet::from(["provenance.source".into()]),
        parsers: BTreeMap::from([("ruff_python_parser".into(), parser.clone())]),
        inputs: BTreeMap::from([(
            "repo".into(),
            BTreeMap::from([(
                DiagnosticPath::try_from("pkg/src/api.py").unwrap(),
                ExtractionInput {
                    kind: "python-source".into(),
                    fingerprint: fingerprint_bytes(b""),
                    parsers: BTreeSet::from(["ruff_python_parser".into()]),
                },
            )]),
        )]),
    }
    .into_provenance(None, None);
    assert_eq!(extraction.tools["python"], env!("CARGO_PKG_VERSION"));
    assert_eq!(extraction.tools["ruff_python_parser"], "0.0.12");
    let ProvenanceActivity::Extraction { mode, parsers, .. } = &extraction.activity else {
        panic!("extraction")
    };
    assert_eq!(*mode, ExtractionMode::Static);
    assert_eq!(parsers["ruff_python_parser"], parser);
    let execution = ExecutionObservation {
        engine: ExecutionEngine::Jupyter,
        kernel: KernelProvenance {
            name: "python3".into(),
            language: None,
            language_version: None,
            version: None,
        },
        origin: ExecutionOrigin::Executed,
        tools: BTreeMap::new(),
        declared_environment_inputs: Vec::new(),
    }
    .into_provenance(None, None);
    let ProvenanceActivity::Execution { mode, kernel, .. } = &execution.activity else {
        panic!("execution")
    };
    assert_eq!(*mode, ExecutionMode::Execute);
    assert_eq!(kernel.language_version, None);
    assert_eq!(kernel.version, None);
    assert!(execution.tools.is_empty());
    support::assert_json_golden(&(extraction, execution), "provenance/producers.json");
}

#[cfg(unix)]
#[test]
fn symlink_parent_traversal_names_the_actual_source_and_rejects_ambiguous_environment_paths() {
    use std::os::unix::fs::symlink;
    let (workspace, mut config, path) = workspace(false);
    workspace.write("repo/deep/nested/placeholder", "");
    workspace.write("repo/deep/input.lock", "actual");
    workspace.write("repo/input.lock", "lexical");
    symlink("deep/nested", workspace.path().join("repo/link")).unwrap();
    config.content[0].execution.declared_environment_inputs = vec!["link/../input.lock".into()];
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let repo = &resolved.repositories[0];
    let source = repo
        .source_location(repo.path.join("link/../input.lock"), None)
        .unwrap();
    assert_eq!(source.path.as_str(), "deep/input.lock");
    assert!(collect_static_provenance(&path, &config, &selection(false)).is_err());
}

#[cfg(unix)]
#[test]
fn nonportable_source_names_are_rejected_without_lossy_conversion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let (workspace, config, path) = workspace(false);
    let resolved = resolve_workspace_paths(&path, &config).unwrap();
    let repo = &resolved.repositories[0];
    for name in [
        OsString::from_vec(vec![0xff]),
        "bad:name".into(),
        "back\\slash".into(),
    ] {
        let file = PathBuf::from("repo").join(name);
        workspace.write(&file, "content");
        let error = repo
            .source_location(workspace.path().join(file), None)
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::InvalidPath { .. }
        ));
    }
}

#[test]
fn git_worktrees_and_corrupt_metadata_have_explicit_observations() {
    let (workspace, config, path) = workspace(false);
    let root = workspace.path().join("repo");
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let checkout = workspace.path().join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            checkout.to_str().unwrap(),
        ],
    );
    let mut config = config;
    config.repositories[0].path = "linked".into();
    config.repositories[0].revision = Some("declared".into());
    let evidence = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    assert_eq!(
        evidence.repositories["repo"].revision.as_deref(),
        Some("declared")
    );
    assert_eq!(
        evidence.revisions["repo"].observed.as_deref(),
        Some(git(&root, &["rev-parse", "HEAD"]).as_str())
    );
    assert_eq!(evidence.repositories["repo"].dirty, Some(false));
    workspace.write("linked/.git", "not valid metadata\n");
    let corrupt = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    assert_eq!(
        corrupt.repositories["repo"].revision.as_deref(),
        Some("declared")
    );
    assert_eq!(corrupt.repositories["repo"].dirty, None);
    assert_eq!(corrupt.revisions["repo"].observed, None);
    assert_eq!(evidence.inputs, corrupt.inputs);
}

#[cfg(unix)]
#[test]
fn selected_symlink_parent_paths_label_the_bytes_actually_hashed() {
    use std::os::unix::fs::symlink;
    let (workspace, config, path) = workspace(false);
    workspace.write("repo/pkg/src/nested/placeholder", "");
    workspace.write("repo/pkg/api.py", "wrong file");
    symlink("src/nested", workspace.path().join("repo/pkg/link")).unwrap();
    let mut inputs = selection(false);
    *inputs.extraction.values_mut().next().unwrap() = vec!["link/../api.py".into()];
    let evidence = collect_static_provenance(&path, &config, &inputs).unwrap();
    assert_eq!(
        evidence.inputs["repo"][&DiagnosticPath::try_from("pkg/src/api.py").unwrap()],
        fingerprint_bytes(b"def api(): pass\n")
    );
    assert!(
        !evidence.inputs["repo"].contains_key(&DiagnosticPath::try_from("pkg/api.py").unwrap())
    );
}

#[test]
fn a_rename_changes_the_manifest_without_changing_file_content_hashes() {
    let (workspace, config, path) = workspace(false);
    let original = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    fs::rename(
        workspace.path().join("repo/docs/index.qmd"),
        workspace.path().join("repo/docs/renamed.qmd"),
    )
    .unwrap();
    let mut inputs = selection(false);
    inputs
        .content
        .insert("guide".into(), vec!["renamed.qmd".into()]);
    let changed = collect_static_provenance(&path, &config, &inputs).unwrap();
    assert_eq!(
        original.inputs["repo"][&DiagnosticPath::try_from("docs/index.qmd").unwrap()],
        changed.inputs["repo"][&DiagnosticPath::try_from("docs/renamed.qmd").unwrap()]
    );
    assert_ne!(
        original.repositories["repo"].declared_input_fingerprint,
        changed.repositories["repo"].declared_input_fingerprint
    );
}

#[test]
fn execution_producer_canonicalizes_environment_order_and_retains_context() {
    use diplodocus::diagnostics::DiagnosticSource;
    use diplodocus::ir::{ExecutionEngine, ExecutionOrigin, KernelProvenance};
    let (_workspace, config, path) = workspace(false);
    let evidence = collect_static_provenance(&path, &config, &selection(false)).unwrap();
    let observation = ExecutionObservation {
        engine: ExecutionEngine::Jupyter,
        kernel: KernelProvenance {
            name: "python3".into(),
            language: None,
            language_version: None,
            version: None,
        },
        origin: ExecutionOrigin::Cache,
        tools: BTreeMap::new(),
        declared_environment_inputs: evidence.declared_environment_inputs["guide"].clone(),
    };
    let mut reordered = observation.clone();
    reordered.declared_environment_inputs.reverse();
    let source = Some(DiagnosticSource::Repository {
        repository: "repo".into(),
        path: DiagnosticPath::try_from("docs/index.qmd").unwrap(),
    });
    let span = Some(SourceSpan { start: 1, end: 7 });
    let first = observation.into_provenance(source.clone(), span);
    let second = reordered.into_provenance(source.clone(), span);
    assert_eq!(first, second);
    assert_eq!(first.source, source);
    assert_eq!(first.span, span);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}
