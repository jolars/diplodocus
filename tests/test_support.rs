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
    assert_eq!(configuration.matches("mode = \"never\"").count(), 2);

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
fn acceptance_fixture_has_executable_python_and_r_qmd_pages() {
    let cases = [
        (
            "python/execution/stateful.qmd",
            "python",
            "python3",
            "pyproject.toml",
            [
                "values = [2, 4, 6]",
                "print(f\"Python total: {total}\")",
                "file=sys.stderr",
                "Markdown(",
                "SVG(",
            ],
        ),
        (
            "r/execution/stateful.qmd",
            "r",
            "ir",
            "DESCRIPTION",
            [
                "values <- c(2, 4, 6)",
                "cat(sprintf(",
                "message(",
                "display_markdown(",
                "display_svg(",
            ],
        ),
    ];

    let configuration = load_fixture("acceptance/workspace/polydoc.toml");
    for (source, language, kernel, environment_input, constructs) in cases {
        assert!(
            fixture_path(format!("acceptance/{source}")).is_file(),
            "executable fixture should exist: {source}"
        );
        assert!(configuration.contains(&format!(
            "repository = \"{language}\"\npath = \"execution\"\nmount = \"execution\"\nformat = \"qmd\"\n\n[content.execution]\nmode = \"execute\"\nengine = \"jupyter\"\nkernel = \"{kernel}\"\ndeclared_environment_inputs = [\"{environment_input}\"]"
        )));

        let page = load_fixture(format!("acceptance/{source}"));
        assert_eq!(page.matches("```{").count(), 5);
        assert_eq!(page.matches("#| label:").count(), 5);
        assert!(page.contains("#| echo:"));
        assert!(page.contains("#| fig-alt:"));
        assert!(page.contains("#| error: true"));
        assert!(page.contains(if language == "python" {
            "raise RuntimeError("
        } else {
            "stop("
        }));
        for construct in constructs {
            assert!(
                page.contains(construct),
                "missing `{construct}` in {source}"
            );
        }
    }
}

#[test]
fn acceptance_fixture_has_execution_authority_variants() {
    for source in [
        "core/docs/execution/display-only.md",
        "python/safety/default-never.qmd",
        "python/safety/metadata-cannot-authorize.qmd",
        "python/execution/generated-markdown.qmd",
    ] {
        assert!(
            fixture_path(format!("acceptance/{source}")).is_file(),
            "execution-authority fixture should exist: {source}"
        );
    }

    let configuration = load_fixture("acceptance/workspace/polydoc.toml");
    let collection_start = configuration
        .find("id = \"python-default-never\"")
        .expect("default-never QMD collection should be configured");
    let collection_tail = &configuration[collection_start..];
    let collection_end = collection_tail
        .find("\n[[content]]")
        .unwrap_or(collection_tail.len());
    let defaulted_collection = &collection_tail[..collection_end];
    assert!(defaulted_collection.contains("format = \"qmd\""));
    assert!(
        !defaulted_collection.contains("[content.execution]"),
        "the focused QMD collection must exercise the default execution mode"
    );

    let gfm = load_fixture("acceptance/core/docs/execution/display-only.md");
    assert!(gfm.contains("```python"));
    assert!(gfm.contains("GFM fences must stay display-only"));

    let default_never = load_fixture("acceptance/python/safety/default-never.qmd");
    assert!(default_never.contains("#| label: default-never"));
    assert!(default_never.contains("QMD execution must default to never"));

    let metadata = load_fixture("acceptance/python/safety/metadata-cannot-authorize.qmd");
    assert!(metadata.contains("execute: true"));
    assert!(metadata.contains("jupyter: python3"));
    assert!(metadata.contains("Document metadata must not authorize execution"));

    let generated = load_fixture("acceptance/python/execution/generated-markdown.qmd");
    assert!(generated.contains("#| label: generated-markdown"));
    assert!(generated.contains("Markdown("));
    assert!(generated.contains("\"```{python}\\n\""));
    assert!(generated.contains("Generated Markdown must stay inert"));
}

#[test]
fn acceptance_fixture_has_output_safety_variants() {
    let cases = [
        (
            "python/execution/markdown-looking-stdout.qmd",
            [
                "#| label: markdown-looking-stdout",
                "print(",
                "# Not a heading",
            ],
        ),
        (
            "python/execution/unsafe-html.qmd",
            ["#| label: unsafe-html", "HTML(", "<script>"],
        ),
        (
            "python/execution/asset-boundary-escape.qmd",
            [
                "#| label: asset-boundary-escape",
                "Markdown(",
                "../../core/docs/assets/workspace.svg",
            ],
        ),
    ];

    for (source, constructs) in cases {
        assert!(
            fixture_path(format!("acceptance/{source}")).is_file(),
            "output-safety fixture should exist: {source}"
        );
        let page = load_fixture(format!("acceptance/{source}"));
        assert_eq!(
            page.lines().filter(|line| *line == "```{python}").count(),
            1
        );
        for construct in constructs {
            assert!(
                page.contains(construct),
                "missing `{construct}` in {source}"
            );
        }
    }
}

#[test]
fn acceptance_configuration_declares_cross_language_callable_concepts() {
    let configuration = load_fixture("acceptance/workspace/polydoc.toml");

    for concept in [
        "id = \"fit\"\nkind = \"equivalent\"\nmembers = [\n  { package = \"pyfoo\", item = \"foo.fit\" },\n  { package = \"rfoo\", item = \"fit\" },\n]",
        "id = \"foo-model.fit\"\nkind = \"analogous\"\nmembers = [\n  { package = \"pyfoo\", item = \"foo.FooModel.fit\" },\n  { package = \"rfoo\", item = \"fit.foo_model\" },\n]",
    ] {
        assert!(configuration.contains(concept));
    }

    let python_stubs = load_fixture("acceptance/python/python/foo/model.pyi");
    assert!(python_stubs.contains("def fit("));
    assert!(python_stubs.matches("@overload").count() >= 4);
    assert!(python_stubs.contains("class FooModel:"));

    let r_source = load_fixture("acceptance/r/R/fit.R");
    assert!(r_source.contains("UseMethod(\"fit\")"));
    assert!(r_source.contains("fit.foo_model <- function("));
}

#[test]
fn acceptance_fixture_has_visibility_and_relationship_variants() {
    let visibility = load_fixture("acceptance/workspace/variants/visibility.toml");
    assert_eq!(visibility.matches("[[package]]").count(), 3);
    assert_eq!(visibility.matches("visibility = \"public\"").count(), 1);
    assert_eq!(visibility.matches("visibility = \"internal\"").count(), 1);
    assert_eq!(visibility.matches("visibility = \"hidden\"").count(), 1);
    assert!(visibility.contains("kind = \"package\""));
    assert_eq!(visibility.matches("kind = \"component\"").count(), 2);
    assert!(!visibility.contains("[[relationship]]"));

    let cases = [
        (
            "relationship-compatible.toml",
            "to = \"rfoo\"",
            "version_constraint = \"^1.8\"",
        ),
        (
            "relationship-incompatible.toml",
            "to = \"rfoo\"",
            "version_constraint = \"^2.0\"",
        ),
        (
            "relationship-external.toml",
            "to = \"cargo:foo-core\"",
            "version_constraint = \"^1.9\"",
        ),
    ];

    for (variant, endpoint, constraint) in cases {
        let configuration = load_fixture(format!("acceptance/workspace/variants/{variant}"));
        assert_eq!(configuration.matches("[[relationship]]").count(), 1);
        assert!(configuration.contains(endpoint));
        assert!(configuration.contains(constraint));
        assert!(configuration.contains("provenance = \"explicit\""));
    }

    let incompatible = load_fixture("acceptance/workspace/variants/relationship-incompatible.toml");
    assert!(incompatible.contains("Expected diagnostic: incompatible-package-relationship"));

    let external = load_fixture("acceptance/workspace/variants/relationship-external.toml");
    assert!(!external.contains("id = \"cargo:foo-core\""));
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
fn acceptance_matrix_maps_every_fixture_to_each_behavior_dimension() {
    let matrix = load_fixture("acceptance/MATRIX.md");

    for heading in [
        "Fixture construct",
        "Expected IR",
        "Diagnostic",
        "Execution",
        "Provenance",
        "URL",
        "Navigation",
        "Link",
        "Concept",
        "Search",
    ] {
        assert!(
            matrix.contains(&format!("| {heading} ")),
            "acceptance matrix should have a `{heading}` column"
        );
    }

    let behavior_tables = matrix
        .split("## MVP completion coverage")
        .next()
        .expect("acceptance matrix should contain behavior tables");
    for (line_number, row) in behavior_tables
        .lines()
        .enumerate()
        .filter(|(_, line)| line.starts_with("| "))
    {
        assert_eq!(
            row.trim_matches('|').split('|').count(),
            10,
            "acceptance matrix row {} should map all behavior dimensions",
            line_number + 1
        );
    }

    let acceptance = fixture_path("acceptance");
    let mut pending = vec![acceptance.clone()];
    let mut fixture_files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("acceptance fixture should be readable") {
            let path = entry.expect("fixture entry should be readable").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                fixture_files.push(path);
            }
        }
    }
    fixture_files.sort();

    for path in fixture_files {
        let relative = path
            .strip_prefix(&acceptance)
            .expect("fixture path should be under the acceptance root")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "MATRIX.md"
            || relative
                .split('/')
                .any(|component| component.starts_with('.'))
        {
            continue;
        }
        assert!(
            matrix.contains(&format!("`{relative}`")),
            "acceptance matrix should map fixture `{relative}`"
        );
    }

    for expected_diagnostic in [
        "python-dynamic-export",
        "unsupported-rd",
        "unsupported-authored-syntax",
        "document-execution-not-authorized",
        "unsafe-kernel-html",
        "generated-asset-outside-boundary",
        "incompatible-package-relationship",
    ] {
        assert!(
            matrix.contains(&format!("`{expected_diagnostic}`")),
            "acceptance matrix should name the `{expected_diagnostic}` diagnostic"
        );
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
