mod support;

use std::fs;
use std::panic::catch_unwind;
use std::path::Path;

use support::{
    TestWorkspace, assert_matches_golden, assert_output_tree, fixture_path, load_fixture,
};

#[test]
fn acceptance_fixture_has_sibling_workspace_roots() {
    let acceptance = fixture_path("acceptance");

    for root in ["workspace", "core", "python", "r"] {
        let path = acceptance.join(root);
        assert!(
            path.is_dir(),
            "fixture root should exist: {}",
            path.display()
        );
        assert_eq!(path.parent(), Some(acceptance.as_path()));
    }
}

#[test]
fn acceptance_configuration_covers_the_design_model() {
    let configuration_path = fixture_path("acceptance/workspace/polydoc.toml");
    let configuration = fs::read_to_string(&configuration_path)
        .expect("acceptance configuration should be readable");

    for table in [
        "[project]",
        "[[repository]]",
        "[[package]]",
        "[[content]]",
        "[[relationship]]",
        "[[concept]]",
    ] {
        assert!(configuration.contains(table), "missing {table} entry");
    }
    assert!(configuration.contains("targets = ["));

    let workspace = configuration_path
        .parent()
        .expect("acceptance configuration should have a parent");
    let workspace = fs::canonicalize(workspace).expect("workspace should be canonicalizable");
    for repository in ["../core", "../python", "../r"] {
        assert!(configuration.contains(&format!("path = \"{repository}\"")));
        let root = fs::canonicalize(workspace.join(Path::new(repository)))
            .expect("repository root should be canonicalizable");
        assert_eq!(root.parent(), workspace.parent());
    }
}

#[test]
fn temporary_workspaces_can_be_seeded_from_fixtures() {
    let workspace = TestWorkspace::from_fixture("support");
    assert_eq!(workspace.read("input.txt"), "fixture input\n");
    assert_eq!(load_fixture("support/input.txt"), "fixture input\n");
}

#[test]
fn golden_files_are_checked_through_one_helper() {
    assert_matches_golden("golden value\n", "support/value.txt");
}

#[test]
fn output_tree_comparison_accepts_equal_trees() {
    let expected = TestWorkspace::from_fixture("output-tree");
    let actual = TestWorkspace::from_fixture("output-tree");
    assert_output_tree(expected.path(), actual.path());
}

#[test]
fn output_tree_comparison_rejects_missing_extra_and_changed_files() {
    let expected = TestWorkspace::from_fixture("output-tree");

    let missing = TestWorkspace::from_fixture("output-tree");
    missing.remove("assets/site.css");
    assert!(catch_unwind(|| assert_output_tree(expected.path(), missing.path())).is_err());

    let extra = TestWorkspace::from_fixture("output-tree");
    extra.write("extra.txt", "extra\n");
    assert!(catch_unwind(|| assert_output_tree(expected.path(), extra.path())).is_err());

    let changed = TestWorkspace::from_fixture("output-tree");
    changed.write("index.html", "changed\n");
    assert!(catch_unwind(|| assert_output_tree(expected.path(), changed.path())).is_err());
}
