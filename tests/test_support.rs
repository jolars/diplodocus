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
fn acceptance_fixture_has_gfm_and_qmd_authored_content() {
    for source in [
        "core/docs/index.md",
        "core/docs/getting-started/workspace.md",
        "python/docs/guide.qmd",
        "python/docs/models/fitting.qmd",
    ] {
        assert!(
            fixture_path(format!("acceptance/{source}")).is_file(),
            "authored fixture should exist: {source}"
        );
    }

    assert!(fixture_path("acceptance/core/docs/assets/workspace.svg").is_file());

    let configuration = load_fixture("acceptance/workspace/polydoc.toml");
    assert!(configuration.contains(
        "id = \"guide\"\nowner = \"project\"\nrepository = \"core\"\npath = \"docs\"\nmount = \"guide\"\nformat = \"gfm\""
    ));
    assert!(configuration.contains(
        "id = \"python-guide\"\nowner = \"pyfoo\"\nrepository = \"python\"\npath = \"docs\"\nmount = \"guide\"\nformat = \"qmd\""
    ));

    let project_index = load_fixture("acceptance/core/docs/index.md");
    assert!(project_index.contains("[`pyfoo::foo.fit`]"));
    assert!(project_index.contains("| Package | Version |"));
    assert!(project_index.contains("> [!NOTE]"));
    assert!(project_index.contains("```python"));
    assert!(project_index.contains("<component name=\"unsupported\" />"));

    let nested_project_page = load_fixture("acceptance/core/docs/getting-started/workspace.md");
    assert!(nested_project_page.contains("![Workspace layout](../assets/workspace.svg)"));

    let package_guide = load_fixture("acceptance/python/docs/guide.qmd");
    assert!(package_guide.contains("::: {.callout-tip #stateful}"));
    assert!(package_guide.contains("::: {.unsupported-directive}"));

    let nested_package_page = load_fixture("acceptance/python/docs/models/fitting.qmd");
    assert!(nested_package_page.contains("[`foo.FooModel.fit`]"));
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
fn acceptance_python_distribution_has_metadata_and_static_source_variants() {
    let python = fixture_path("acceptance/python");

    for source in [
        "pyproject.toml",
        "python/foo/__init__.py",
        "python/foo/__init__.pyi",
        "python/foo/model.py",
        "python/foo/model.pyi",
        "python/foo/experimental.py",
        "python/foo/_native.pyi",
        "python/foo/py.typed",
    ] {
        assert!(
            python.join(source).is_file(),
            "Python fixture source should exist: {source}"
        );
    }
    assert!(
        !python.join("python/foo/_native.py").exists(),
        "the native extension should be represented only by its stub"
    );

    let metadata = load_fixture("acceptance/python/pyproject.toml");
    for field in [
        "[project]",
        "name = \"foo-python\"",
        "version = \"1.9.0\"",
        "dependencies = [",
    ] {
        assert!(metadata.contains(field), "missing Python metadata: {field}");
    }
}

#[test]
fn acceptance_python_distribution_declares_its_public_surface() {
    let package = load_fixture("acceptance/python/python/foo/__init__.py");

    for public_name in [
        "DEFAULT_TOLERANCE",
        "SUPPORTED_SOLVERS",
        "FitDiagnostics",
        "FooModel",
        "NativeWorkspace",
        "fit",
        "mean_squared_error",
        "native_mean",
    ] {
        assert!(
            package.contains(public_name),
            "missing public Python name: {public_name}"
        );
    }
    assert!(package.contains("from .model import"));
    assert!(package.contains("from ._native import"));
    assert!(package.contains("__all__ = ["));

    let implementation = load_fixture("acceptance/python/python/foo/model.py");
    for construct in [
        "class FitDiagnostics:",
        "class FooModel:",
        "def fit(",
        "def mean_squared_error(",
        "def predict(",
        "@property",
        "DEFAULT_TOLERANCE:",
        "SUPPORTED_SOLVERS:",
    ] {
        assert!(
            implementation.contains(construct),
            "missing public Python construct: {construct}"
        );
    }

    let dynamic_exports = load_fixture("acceptance/python/python/foo/experimental.py");
    assert!(dynamic_exports.contains("def experimental_rank("));
    assert!(dynamic_exports.contains("__all__ = _exported_names()"));

    let native_stub = load_fixture("acceptance/python/python/foo/_native.pyi");
    assert!(native_stub.contains("class NativeWorkspace:"));
    assert!(native_stub.contains("def native_mean("));
}

#[test]
fn acceptance_python_distribution_has_overloads_and_numpy_docstrings() {
    let stubs = load_fixture("acceptance/python/python/foo/model.pyi");
    assert!(stubs.matches("@overload").count() >= 4);
    assert!(stubs.contains("class FooModel:"));
    assert!(stubs.contains("def fit("));
    assert!(stubs.contains("def predict("));

    let implementation = load_fixture("acceptance/python/python/foo/model.py");
    for section in [
        "Parameters\n    ----------",
        "Returns\n    -------",
        "Raises\n    ------",
        "Notes\n    -----",
        "References\n    ----------",
        "Examples\n    --------",
    ] {
        assert!(
            implementation.contains(section),
            "missing NumPy docstring section: {section}"
        );
    }
}

#[test]
fn acceptance_r_package_has_metadata_namespace_and_sources() {
    let package = fixture_path("acceptance/r");

    for source in [
        "DESCRIPTION",
        "LICENSE",
        "NAMESPACE",
        "R/fit.R",
        "R/metrics.R",
        "R/experimental.R",
        "man/fit.Rd",
        "man/foo_model.Rd",
        "man/mean_squared_error.Rd",
        "man/experimental_summary.Rd",
    ] {
        assert!(
            package.join(source).is_file(),
            "R fixture source should exist: {source}"
        );
    }

    let description = load_fixture("acceptance/r/DESCRIPTION");
    for field in [
        "Package: foo",
        "Version: 1.8.0",
        "Depends:",
        "R (>= 4.3)",
        "Imports:",
        "stats",
        "Suggests:",
        "testthat (>= 3.2.0)",
    ] {
        assert!(description.contains(field), "missing R metadata: {field}");
    }
}

#[test]
fn acceptance_r_package_declares_exports_and_s3_dispatch() {
    let namespace = load_fixture("acceptance/r/NAMESPACE");
    for directive in [
        "export(fit)",
        "export(foo_model)",
        "export(mean_squared_error)",
        "export(experimental_summary)",
        "S3method(fit,default)",
        "S3method(fit,foo_model)",
        "S3method(predict,foo_model)",
        "importFrom(stats,predict)",
    ] {
        assert!(
            namespace.contains(directive),
            "missing R namespace directive: {directive}"
        );
    }

    let implementation = load_fixture("acceptance/r/R/fit.R");
    for construct in [
        "fit <- function(x, ...)",
        "UseMethod(\"fit\")",
        "fit.default <- function(",
        "fit.foo_model <- function(",
        "predict.foo_model <- function(",
        "foo_model <- function(",
    ] {
        assert!(
            implementation.contains(construct),
            "missing public R construct: {construct}"
        );
    }

    let metrics = load_fixture("acceptance/r/R/metrics.R");
    assert!(metrics.contains("mean_squared_error <- function("));
}

#[test]
fn acceptance_r_package_has_structured_rd_and_an_unsupported_construct() {
    let fit_documentation = load_fixture("acceptance/r/man/fit.Rd");
    for construct in [
        r"\alias{fit}",
        r"\alias{fit.default}",
        r"\alias{fit.foo_model}",
        r"\usage{",
        r"\arguments{",
        r"\value{",
        r"\references{",
        r"\examples{",
    ] {
        assert!(
            fit_documentation.contains(construct),
            "missing Rd construct: {construct}"
        );
    }

    let model_documentation = load_fixture("acceptance/r/man/foo_model.Rd");
    assert!(model_documentation.contains(r"\alias{predict.foo_model}"));
    assert!(model_documentation.contains(r"\method{predict}{foo_model}"));

    let dynamic_documentation = load_fixture("acceptance/r/man/experimental_summary.Rd");
    assert!(dynamic_documentation.contains(r"\Sexpr[stage=render,results=text]"));
    assert!(dynamic_documentation.contains("must produce an unsupported-Rd diagnostic"));
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
