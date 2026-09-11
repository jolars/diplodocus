mod support;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use diplodocus::configuration::{WorkspaceConfiguration, load_configuration, parse_configuration};
use diplodocus::paths::{PathResolutionErrorKind, PathType, resolve_workspace_paths};

use support::TestWorkspace;

const CONFIG: &str = r#"
[project]
name = "Paths"

[[repository]]
id = "source"
path = "../repo"

[[package]]
id = "pkg"
name = "Package"
slug = "pkg"
ecosystem = "python"
repository = "source"
path = "packages/pkg"
metadata_path = "pyproject.toml"
targets = [
  { id = "directory", extractor = "python", path = "src", role = "public-api" },
  { id = "file", extractor = "python", path = "src/module.py", role = "public-api" },
]

[[content]]
id = "guide"
owner = "project"
repository = "source"
path = "docs"
mount = "guide"
format = "qmd"

[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared_environment_inputs = ["uv.lock"]
"#;

fn workspace() -> (TestWorkspace, WorkspaceConfiguration, PathBuf) {
    let workspace = TestWorkspace::new();
    workspace.write("config/diplodocus.toml", CONFIG);
    workspace.write("repo/packages/pkg/pyproject.toml", "[project]");
    workspace.write("repo/packages/pkg/src/module.py", "");
    workspace.write("repo/docs/index.qmd", "# Guide");
    workspace.write("repo/uv.lock", "");
    let config_path = workspace.path().join("config/diplodocus.toml");
    let config = load_configuration(&config_path).unwrap();
    (workspace, config, config_path)
}

fn canonical(workspace: &TestWorkspace, path: &str) -> PathBuf {
    fs::canonicalize(workspace.path().join(path)).unwrap()
}

#[test]
fn resolves_each_base_without_changing_portable_declarations() {
    let (workspace, config, config_path) = workspace();
    let before = toml::to_string(&config).unwrap();
    let resolved = resolve_workspace_paths(&config_path, &config).unwrap();
    assert_eq!(
        resolved.configuration_directory,
        canonical(&workspace, "config")
    );
    assert_eq!(resolved.repositories.len(), 1);
    assert_eq!(resolved.repositories[0].id, "source");
    assert_eq!(resolved.repositories[0].path, canonical(&workspace, "repo"));
    assert_eq!(resolved.packages[0].id, "pkg");
    assert_eq!(resolved.packages[0].repository_index, 0);
    assert_eq!(
        resolved.packages[0].path,
        canonical(&workspace, "repo/packages/pkg")
    );
    assert_eq!(
        resolved.packages[0].metadata_path,
        canonical(&workspace, "repo/packages/pkg/pyproject.toml")
    );
    assert_eq!(resolved.packages[0].targets[0].id, "directory");
    assert_eq!(
        resolved.packages[0].targets[0].path,
        canonical(&workspace, "repo/packages/pkg/src")
    );
    assert_eq!(
        resolved.packages[0].targets[1].path,
        canonical(&workspace, "repo/packages/pkg/src/module.py")
    );
    assert_eq!(resolved.content[0].id, "guide");
    assert_eq!(resolved.content[0].repository_index, 0);
    assert_eq!(resolved.content[0].path, canonical(&workspace, "repo/docs"));
    assert_eq!(
        resolved.content[0].declared_environment_inputs,
        [canonical(&workspace, "repo/uv.lock")]
    );
    assert_eq!(toml::to_string(&config).unwrap(), before);
    assert_eq!(parse_configuration(&before).unwrap(), config);
    assert!(!before.contains(workspace.path().to_str().unwrap()));
}

#[test]
fn resolves_acceptance_sibling_checkouts_in_declaration_order() {
    let workspace = support::acceptance_workspace();
    let path = workspace.path().join("workspace/diplodocus.toml");
    let config = load_configuration(&path).unwrap();
    let resolved = resolve_workspace_paths(path, &config).unwrap();
    assert_eq!(
        resolved
            .repositories
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["core", "python", "r"]
    );
    assert_eq!(resolved.packages[0].repository_index, 1);
    assert_eq!(resolved.packages[1].repository_index, 2);
    assert_eq!(
        resolved.packages[1].targets[0].path,
        canonical(&workspace, "r")
    );
    assert_eq!(
        resolved.content[3].declared_environment_inputs,
        [canonical(&workspace, "python/pyproject.toml")]
    );
    assert_eq!(
        resolved.content[4].declared_environment_inputs,
        [canonical(&workspace, "r/DESCRIPTION")]
    );
}

#[test]
fn resolves_the_authored_only_project_configuration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("diplodocus.toml");
    let config = load_configuration(&path).unwrap();
    let resolved = resolve_workspace_paths(path, &config).unwrap();
    assert!(resolved.packages.is_empty());
    assert_eq!(resolved.content.len(), 2);
}

#[test]
fn empty_inputs_and_targets_remain_empty_without_discovery() {
    let (_workspace, mut config, path) = workspace();
    config.packages[0].targets.clear();
    assert!(
        resolve_workspace_paths(&path, &config).unwrap().packages[0]
            .targets
            .is_empty()
    );
    config.packages.clear();
    assert!(
        resolve_workspace_paths(&path, &config)
            .unwrap()
            .packages
            .is_empty()
    );
    let minimal = parse_configuration("[project]\nname = 'Minimal'\n").unwrap();
    let resolved = resolve_workspace_paths(path, &minimal).unwrap();
    assert!(resolved.repositories.is_empty());
    assert!(resolved.packages.is_empty());
    assert!(resolved.content.is_empty());
}

#[test]
fn only_the_configuration_parent_must_exist() {
    let (_workspace, config, path) = workspace();
    fs::remove_file(&path).unwrap();
    resolve_workspace_paths(&path, &config).unwrap();
    let error = resolve_workspace_paths(path.join("missing.toml"), &config).unwrap_err();
    assert_eq!(error.field, "configuration_directory");
    assert!(matches!(
        error.kind,
        PathResolutionErrorKind::FileSystem { .. }
    ));
}

#[test]
fn bare_relative_configuration_path_uses_current_directory() {
    let config = parse_configuration("[project]\nname = 'Minimal'\n").unwrap();
    let resolved = resolve_workspace_paths("not-created.toml", &config).unwrap();
    assert_eq!(
        resolved.configuration_directory,
        fs::canonicalize(".").unwrap()
    );
}

#[test]
fn explicit_absolute_repository_root_is_allowed() {
    let (workspace, mut config, path) = workspace();
    config.repositories[0].path = canonical(&workspace, "repo");
    resolve_workspace_paths(path, &config).unwrap();
}

#[derive(Debug, Clone, Copy)]
enum Field {
    Repository,
    Package,
    Metadata,
    Target,
    Content,
    Environment,
}

impl Field {
    fn set(self, config: &mut WorkspaceConfiguration, path: impl Into<PathBuf>) {
        let target = match self {
            Self::Repository => &mut config.repositories[0].path,
            Self::Package => &mut config.packages[0].path,
            Self::Metadata => &mut config.packages[0].metadata_path,
            Self::Target => &mut config.packages[0].targets[0].path,
            Self::Content => &mut config.content[0].path,
            Self::Environment => &mut config.content[0].execution.declared_environment_inputs[0],
        };
        *target = path.into();
    }

    fn field(self) -> &'static str {
        match self {
            Self::Repository => "repository[0] (`source`).path",
            Self::Package => "package[0] (`pkg`).path",
            Self::Metadata => "package[0] (`pkg`).metadata_path",
            Self::Target => "package[0] (`pkg`).targets[0] (`directory`).path",
            Self::Content => "content[0] (`guide`).path",
            Self::Environment => "content[0] (`guide`).execution.declared_environment_inputs[0]",
        }
    }

    fn boundary(self) -> &'static str {
        match self {
            Self::Metadata | Self::Target => "repo/packages/pkg",
            _ => "repo",
        }
    }
}

const CHILDREN: [Field; 5] = [
    Field::Package,
    Field::Metadata,
    Field::Target,
    Field::Content,
    Field::Environment,
];
const ALL_FIELDS: [Field; 6] = [
    Field::Repository,
    Field::Package,
    Field::Metadata,
    Field::Target,
    Field::Content,
    Field::Environment,
];

#[test]
fn missing_paths_report_the_configuration_field_and_underlying_error() {
    let (_workspace, original, path) = workspace();
    for field in ALL_FIELDS {
        let mut config = original.clone();
        field.set(&mut config, "missing");
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.configuration_path, path);
        assert_eq!(error.field, field.field());
        assert!(error.to_string().contains(field.field()));
        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(error.source().is_some());
        let PathResolutionErrorKind::FileSystem { source, path } = error.kind else {
            panic!("expected filesystem error for {field:?}");
        };
        assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        assert!(path.ends_with("missing"));
    }
}

#[test]
fn roots_are_directories_and_metadata_and_environment_are_regular_files() {
    let (_workspace, original, path) = workspace();
    for (field, invalid, expected) in [
        (Field::Repository, "diplodocus.toml", PathType::Directory),
        (Field::Package, "uv.lock", PathType::Directory),
        (Field::Content, "uv.lock", PathType::Directory),
        (Field::Metadata, "src", PathType::File),
        (Field::Environment, "docs", PathType::File),
    ] {
        let mut config = original.clone();
        field.set(&mut config, invalid);
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.field, field.field());
        assert!(
            matches!(error.kind, PathResolutionErrorKind::WrongType { expected: actual, .. } if actual == expected)
        );
    }
}

#[test]
fn child_declarations_cannot_reset_their_base_with_absolute_paths() {
    let (workspace, original, path) = workspace();
    for field in CHILDREN {
        let mut config = original.clone();
        field.set(&mut config, canonical(&workspace, field.boundary()));
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.field, field.field());
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::InvalidPath { .. }
        ));
    }
}

#[test]
fn empty_declarations_do_not_implicitly_select_their_base() {
    let (_workspace, original, path) = workspace();
    for field in ALL_FIELDS {
        let mut config = original.clone();
        field.set(&mut config, "");
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.field, field.field());
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::InvalidPath { .. }
        ));
    }
}

#[test]
fn parent_traversal_cannot_leave_any_child_boundary_even_when_it_reenters() {
    let (_workspace, original, path) = workspace();
    for field in CHILDREN {
        for declared in ["..", "../pkg", "../repo"] {
            let mut config = original.clone();
            field.set(&mut config, declared);
            let error = resolve_workspace_paths(&path, &config).unwrap_err();
            assert_eq!(error.field, field.field());
            assert!(matches!(
                error.kind,
                PathResolutionErrorKind::OutsideBoundary { .. }
            ));
        }
    }
}

#[test]
fn contained_parent_segments_are_resolved_on_the_filesystem() {
    let (_workspace, mut config, path) = workspace();
    config.packages[0].path = "packages/./pkg/../pkg".into();
    config.packages[0].metadata_path = "src/../pyproject.toml".into();
    config.packages[0].targets[0].path = "src/..".into();
    config.content[0].path = "docs/../docs".into();
    config.content[0].execution.declared_environment_inputs = vec!["docs/../uv.lock".into()];
    resolve_workspace_paths(path, &config).unwrap();
}

#[test]
fn missing_segments_are_not_erased_by_parent_normalization() {
    let (_workspace, mut config, path) = workspace();
    config.packages[0].metadata_path = "missing/../pyproject.toml".into();
    assert!(matches!(
        resolve_workspace_paths(path, &config).unwrap_err().kind,
        PathResolutionErrorKind::FileSystem { .. }
    ));
}

#[test]
fn directory_syntax_cannot_turn_a_regular_file_into_a_directory() {
    let (_workspace, original, path) = workspace();
    for declared in [
        "pyproject.toml/",
        "pyproject.toml/.",
        "pyproject.toml/../pyproject.toml",
    ] {
        let mut config = original.clone();
        config.packages[0].metadata_path = declared.into();
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.field, Field::Metadata.field());
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::FileSystem { .. }
        ));
    }
}

#[test]
fn configuration_parent_must_be_a_directory() {
    let (_workspace, config, path) = workspace();
    let error = resolve_workspace_paths(path.join("child.toml"), &config).unwrap_err();
    assert_eq!(error.field, "configuration_directory");
    assert!(matches!(
        error.kind,
        PathResolutionErrorKind::WrongType {
            expected: PathType::Directory,
            ..
        }
    ));
}

#[test]
fn repository_references_must_select_exactly_one_root() {
    let (_workspace, original, path) = workspace();
    for content in [false, true] {
        for duplicate in [false, true] {
            let mut config = original.clone();
            if duplicate {
                config.repositories.push(config.repositories[0].clone());
            } else if content {
                config.content[0].repository = "unknown".into();
            } else {
                config.packages[0].repository = "unknown".into();
            }
            if content {
                config.packages.clear();
            }
            let error = resolve_workspace_paths(&path, &config).unwrap_err();
            assert_eq!(
                error.field,
                if content {
                    "content[0] (`guide`).repository"
                } else {
                    "package[0] (`pkg`).repository"
                }
            );
            assert!(
                matches!(error.kind, PathResolutionErrorKind::RepositoryReference { matches, .. } if matches == if duplicate { 2 } else { 0 })
            );
        }
    }
}

#[test]
fn first_error_is_deterministic_in_declaration_order() {
    let (_workspace, mut config, path) = workspace();
    config.repositories[0].path = "missing-repo".into();
    config.packages[0].repository = "unknown".into();
    config.content[0].path = "missing-content".into();
    for _ in 0..3 {
        assert_eq!(
            resolve_workspace_paths(&path, &config).unwrap_err().field,
            Field::Repository.field()
        );
    }
}

#[cfg(unix)]
mod symlinks {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn contained_symlinks_work_for_directories_and_files() {
        let (workspace, mut config, path) = workspace();
        symlink("packages/pkg", workspace.path().join("repo/pkg-link")).unwrap();
        symlink("src", workspace.path().join("repo/packages/pkg/src-link")).unwrap();
        symlink(
            "pyproject.toml",
            workspace.path().join("repo/packages/pkg/metadata-link"),
        )
        .unwrap();
        symlink("docs", workspace.path().join("repo/docs-link")).unwrap();
        symlink("uv.lock", workspace.path().join("repo/env-link")).unwrap();
        config.packages[0].path = "pkg-link".into();
        config.packages[0].metadata_path = "metadata-link".into();
        config.packages[0].targets[0].path = "src-link".into();
        config.content[0].path = "docs-link".into();
        config.content[0].execution.declared_environment_inputs = vec!["env-link".into()];
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(
            resolved.packages[0].path,
            canonical(&workspace, "repo/packages/pkg")
        );
        assert_eq!(
            resolved.packages[0].targets[0].path,
            canonical(&workspace, "repo/packages/pkg/src")
        );
        assert_eq!(
            resolved.content[0].declared_environment_inputs,
            [canonical(&workspace, "repo/uv.lock")]
        );
    }

    #[test]
    fn symlink_parent_segments_follow_actual_directory_structure() {
        let (workspace, mut config, path) = workspace();
        fs::create_dir_all(workspace.path().join("repo/deep/nested")).unwrap();
        symlink("deep/nested", workspace.path().join("repo/link")).unwrap();
        config.content[0].path = "link/..".into();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(resolved.content[0].path, canonical(&workspace, "repo/deep"));
    }

    #[test]
    fn escaping_links_are_rejected_at_every_boundary_including_prefix_traps() {
        let (workspace, original, path) = workspace();
        fs::create_dir_all(workspace.path().join("repo-extra")).unwrap();
        fs::create_dir_all(workspace.path().join("repo/packages/pkg-extra")).unwrap();
        symlink("../repo-extra", workspace.path().join("repo/escape")).unwrap();
        symlink(
            "../pkg-extra",
            workspace.path().join("repo/packages/pkg/escape"),
        )
        .unwrap();
        for field in CHILDREN {
            for declared in [
                "escape",
                "escape/../pkg",
                "escape/../repo",
                "escape/../missing",
            ] {
                let mut config = original.clone();
                field.set(&mut config, declared);
                let error = resolve_workspace_paths(&path, &config).unwrap_err();
                assert_eq!(error.field, field.field());
                let PathResolutionErrorKind::OutsideBoundary { boundary, .. } = error.kind else {
                    panic!("expected boundary error for {field:?}: {error}");
                };
                assert_eq!(boundary, canonical(&workspace, field.boundary()));
            }
        }
    }

    #[test]
    fn symlink_chains_are_judged_by_their_canonical_referent() {
        let (workspace, mut config, path) = workspace();
        fs::create_dir_all(workspace.path().join("outside")).unwrap();
        symlink("../repo/docs", workspace.path().join("outside/back")).unwrap();
        symlink("../outside/back", workspace.path().join("repo/roundtrip")).unwrap();
        config.content[0].path = "roundtrip".into();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(resolved.content[0].path, canonical(&workspace, "repo/docs"));
    }

    #[test]
    fn broken_and_cyclic_links_preserve_filesystem_errors() {
        let (workspace, original, path) = workspace();
        symlink("absent", workspace.path().join("repo/broken")).unwrap();
        symlink("cycle", workspace.path().join("repo/cycle")).unwrap();
        for declared in ["broken", "cycle"] {
            let mut config = original.clone();
            config.content[0].path = declared.into();
            assert!(matches!(
                resolve_workspace_paths(&path, &config).unwrap_err().kind,
                PathResolutionErrorKind::FileSystem { .. }
            ));
        }
    }

    #[test]
    fn declared_repository_symlink_establishes_its_own_boundary() {
        let (workspace, mut config, path) = workspace();
        symlink("repo", workspace.path().join("checkout")).unwrap();
        config.repositories[0].path = "../checkout".into();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(resolved.repositories[0].path, canonical(&workspace, "repo"));
    }

    #[test]
    fn configuration_file_symlink_uses_the_supplied_parent() {
        let (workspace, config, path) = workspace();
        workspace.write("elsewhere/config.toml", CONFIG);
        fs::remove_file(&path).unwrap();
        symlink("../elsewhere/config.toml", &path).unwrap();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(
            resolved.configuration_directory,
            canonical(&workspace, "config")
        );
    }

    #[test]
    fn configuration_parent_and_repository_paths_preserve_symlink_parent_order() {
        let (workspace, mut config, _) = workspace();
        fs::create_dir_all(workspace.path().join("nested/config/child")).unwrap();
        symlink("nested/config/child", workspace.path().join("config-link")).unwrap();
        symlink("repo", workspace.path().join("repo-link")).unwrap();
        let path = workspace.path().join("config-link/../diplodocus.toml");
        config.repositories[0].path = "../../repo-link/packages/..".into();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(
            resolved.configuration_directory,
            canonical(&workspace, "nested/config")
        );
        assert_eq!(resolved.repositories[0].path, canonical(&workspace, "repo"));
    }

    #[test]
    fn absolute_symlinks_inside_the_boundary_are_permitted() {
        let (workspace, mut config, path) = workspace();
        symlink(
            canonical(&workspace, "repo/docs"),
            workspace.path().join("repo/absolute-link"),
        )
        .unwrap();
        config.content[0].path = "absolute-link".into();
        let resolved = resolve_workspace_paths(path, &config).unwrap();
        assert_eq!(resolved.content[0].path, canonical(&workspace, "repo/docs"));
    }

    #[test]
    fn special_files_are_rejected_without_opening_them() {
        use std::os::unix::net::UnixListener;

        let (workspace, original, path) = workspace();
        let _package_socket =
            UnixListener::bind(workspace.path().join("repo/packages/pkg/socket")).unwrap();
        let _repo_socket = UnixListener::bind(workspace.path().join("repo/socket")).unwrap();
        for field in [Field::Metadata, Field::Target, Field::Environment] {
            let mut config = original.clone();
            field.set(&mut config, "socket");
            assert!(matches!(
                resolve_workspace_paths(&path, &config).unwrap_err().kind,
                PathResolutionErrorKind::WrongType { .. }
            ));
        }
    }
}
