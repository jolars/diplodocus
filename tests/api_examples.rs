mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::ProvenanceActivity;
use diplodocus::snapshots::Snapshot;
use std::fs;
use std::process::{Command, Output};

fn workspace(language: &str, documentation: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    let (metadata, entry, package_name) = if language == "python" {
        ("pyproject.toml", "src/apiex", "apiex")
    } else {
        ("Project.toml", "src/ApiExamples.jl", "ApiExamples")
    };
    root.write("diplodocus.toml", format!("[project]\nname='API examples'\n[[repository]]\nid='repo'\npath='repo'\n[[package]]\nid='api'\nname='API examples'\nslug='api'\necosystem='{language}'\nrepository='repo'\npath='.'\nmetadata-path='{metadata}'\ntargets=[{{id='api',extractor='{language}',path='{entry}',role='public-api'}}]\n"));
    if language == "python" {
        root.write(
            "repo/pyproject.toml",
            "[project]\nname='apiex'\nversion='0.1.0'\n",
        );
        root.write("repo/src/apiex/__init__.py", format!("__all__ = ['answer', 'other']\ndef answer():\n    \"\"\"An answer.\n\n    Examples\n    --------\n{documentation}\n    \"\"\"\n    return 42\ndef other():\n    \"\"\"Another answer.\n\n    Examples\n    --------\n    >>> 'shared' in globals()\n    False\n    \"\"\"\n    return 24\n"));
    } else {
        root.write("repo/Project.toml", format!("name='{package_name}'\nuuid='556f29ba-a5ad-4973-b428-a6fb30b48c51'\nversion='0.1.0'\n"));
        root.write("repo/src/ApiExamples.jl", format!("module ApiExamples\nexport answer, other\n\"\"\"\nAn answer.\n\n{documentation}\n\"\"\"\nanswer() = 42\n\"\"\"\nAnother answer.\n\n```jldoctest\njulia> isdefined(@__MODULE__, :shared)\nfalse\n```\n\"\"\"\nother() = 24\nend\n"));
    }
    root
}

fn command(root: &support::TestWorkspace, operation: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args([operation, "--config", "diplodocus.toml"])
        .output()
        .unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const PYTHON: &str = "    >>> shared = answer()\n    >>> shared + 1\n    43\n    >>> print('a  b')  # doctest: +NORMALIZE_WHITESPACE\n    a b\n    >>> raise ValueError('expected')\n    Traceback (most recent call last):\n    ...\n    ValueError: expected\n    >>> raise RuntimeError('must stay inert')  # doctest: +SKIP\n";

const JULIA: &str = r#"```jldoctest state; setup = :(nothing), teardown = :(nothing), output = false
julia> shared = answer();

julia> shared + 1
43

julia> error("expected")
ERROR: expected
[...]
```

```jldoctest state
julia> shared
42
```

```jldoctest
answer() + 2
# output
44
```

```jldoctest
julia> isdefined(@__MODULE__, :shared)
false
```"#;

#[test]
fn native_doctests_verify_state_expected_errors_and_portable_outcomes() {
    for (language, documentation) in [("python", PYTHON), ("julia", JULIA)] {
        let root = workspace(language, documentation);
        success(command(&root, "check"));
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        assert!(
            sources.workspace().packages["api"]
                .items
                .values()
                .filter_map(|item| item.documentation.as_ref())
                .flat_map(|d| &d.provenance)
                .all(|p| !matches!(p.activity, ProvenanceActivity::ApiExamples { .. }))
        );
        success(command(&root, "build"));
        let database = root.path().join(".diplodocus/documentation.sqlite");
        let snapshot = Snapshot::load(&database).unwrap();
        let examples: Vec<_> = snapshot.workspace().packages["api"]
            .items
            .values()
            .filter_map(|item| item.documentation.as_ref())
            .flat_map(|d| &d.provenance)
            .filter_map(|p| match &p.activity {
                ProvenanceActivity::ApiExamples {
                    runtime_version,
                    examples,
                    ..
                } => {
                    assert!(!runtime_version.is_empty());
                    Some(examples)
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert!(examples.iter().any(|e| e.actual.contains("43")));
        assert!(
            examples
                .iter()
                .any(|e| e.outcome == "expected-error" && e.actual.contains("expected"))
        );
        if language == "python" {
            assert!(
                examples
                    .iter()
                    .any(|e| e.outcome == "explicit-skip" && e.reason.is_some())
            );
        }
        assert!(
            !snapshot
                .canonical_export()
                .unwrap()
                .contains(root.path().to_str().unwrap())
        );
        let original = fs::read(root.path().join("site/packages/api/index.html")).unwrap();
        fs::remove_dir_all(root.path().join("repo")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", "")
            .args([
                "generate",
                "--input",
                database.to_str().unwrap(),
                "--output",
                "regenerated",
            ])
            .output()
            .unwrap();
        success(output);
        assert_eq!(
            original,
            fs::read(root.path().join("regenerated/packages/api/index.html")).unwrap()
        );
    }
}

#[test]
fn output_mismatches_and_unexpected_errors_preserve_previous_publications() {
    for (language, good, bad) in [
        (
            "python",
            "    >>> answer()\n    42\n",
            "    >>> answer()\n    99\n",
        ),
        (
            "julia",
            "```jldoctest\nanswer()\n# output\n42\n```",
            "```jldoctest\nanswer()\n# output\n99\n```",
        ),
    ] {
        let root = workspace(language, good);
        success(command(&root, "build"));
        let snapshot = root.path().join(".diplodocus/documentation.sqlite");
        let original = fs::read(&snapshot).unwrap();
        let site = fs::read(root.path().join("site/index.html")).unwrap();
        let path = if language == "python" {
            "repo/src/apiex/__init__.py"
        } else {
            "repo/src/ApiExamples.jl"
        };
        root.write(path, root.read(path).replace(good, bad));
        success(command(&root, "check"));
        let output = command(&root, "build");
        assert!(!output.status.success());
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("Expected"), "{message}");
        assert_eq!(original, fs::read(&snapshot).unwrap());
        assert_eq!(site, fs::read(root.path().join("site/index.html")).unwrap());
        let content = root.read(path).replace(bad, good);
        root.write(
            path,
            if language == "python" {
                content.replace("    return 42", "    raise RuntimeError('broken')")
            } else {
                content.replace("answer() = 42", "answer() = error(\"broken\")")
            },
        );
        let output = command(&root, "build");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("broken"));
        assert_eq!(original, fs::read(&snapshot).unwrap());
        assert_eq!(site, fs::read(root.path().join("site/index.html")).unwrap());
    }
}

#[test]
fn unsupported_julia_executable_options_fail_extraction() {
    let root = workspace(
        "julia",
        "```jldoctest; filter = r\"x\"\nanswer()\n# output\n42\n```",
    );
    let output = command(&root, "extract");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unsupported doctest option"));
}

#[test]
fn unavailable_interpreters_and_dependencies_are_errors_instead_of_skips() {
    for (language, documentation) in [("python", PYTHON), ("julia", JULIA)] {
        let root = workspace(language, documentation);
        let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", "")
            .args(["extract", "--config", "diplodocus.toml"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("require the interpreter"));
        assert!(
            !root
                .path()
                .join(".diplodocus/documentation.sqlite")
                .exists()
        );
    }
    let root = workspace("python", PYTHON);
    let path = "repo/src/apiex/__init__.py";
    root.write(
        path,
        format!(
            "import unavailable_documentation_dependency\n{}",
            root.read(path)
        ),
    );
    success(command(&root, "check"));
    let output = command(&root, "extract");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unavailable_documentation_dependency")
    );
}

#[test]
fn julia_function_overviews_include_method_prose_and_resolved_references() {
    let root = workspace(
        "julia",
        "Use [the other function](@ref other).\n\n# Details\n\nSee [details](#details).",
    );
    success(command(&root, "build"));
    let site = root.path().join("site");
    let search: serde_json::Value =
        serde_json::from_slice(&fs::read(site.join("assets/search.json")).unwrap()).unwrap();
    let entry = search
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["title"] == "ApiExamples.answer")
        .unwrap();
    let html = fs::read_to_string(site.join(entry["path"].as_str().unwrap())).unwrap();
    assert!(html.contains("An answer."));
    assert!(html.contains("the other function</a>"));
    assert!(!search.as_array().unwrap().iter().any(|e| {
        e["title"]
            .as_str()
            .is_some_and(|t| t.starts_with("ApiExamples.answer("))
    }));
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let (method_id, _) = snapshot.workspace().packages["api"]
        .items
        .iter()
        .find(|(_, item)| item.kind == diplodocus::ir::ItemKind::Method && item.name == "answer")
        .unwrap();
    let anchor = format!(
        "method-{}-details",
        diplodocus::provenance::fingerprint_bytes(method_id.as_bytes()).value
    );
    assert!(html.contains(&format!("href=\"#{anchor}\"")));
    assert!(html.contains(&format!("id=\"{anchor}\"")));
    assert!(
        entry["text"]
            .as_str()
            .unwrap()
            .contains("the other function")
    );
}

#[test]
fn native_timeouts_and_cancellation_stop_before_publication() {
    use diplodocus::commands::{ExtractOptions, extract_with};
    use diplodocus::execution::ExecutionDeadlines;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for cancel in [false, true] {
        let root = workspace("python", "    >>> while True:\n    ...     pass\n");
        let output = root.path().join("snapshot.sqlite");
        let options = ExtractOptions {
            config: root.path().join("diplodocus.toml"),
            output: Some(output.clone()),
        };
        let deadlines = ExecutionDeadlines {
            cell: 500,
            termination: 100,
            forced_exit: 100,
            ..ExecutionDeadlines::default()
        };
        let result = runtime.block_on(extract_with(
            options,
            deadlines,
            if cancel {
                Box::pin(async { tokio::time::sleep(std::time::Duration::from_millis(100)).await })
            } else {
                Box::pin(std::future::pending())
            },
        ));
        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(if cancel {
                "canceled"
            } else {
                "source diagnostics"
            }),
            "{message}"
        );
        assert!(!output.exists());
    }
}

#[test]
fn python_field_examples_use_module_globals_and_verify_package_identity() {
    let root = workspace("python", PYTHON);
    let path = "repo/src/apiex/__init__.py";
    let source = root
        .read(path)
        .replace("['answer', 'other']", "['answer', 'other', 'Model']");
    root.write(path, format!("{source}\nclass Model:\n    value: int = 42\n    \"\"\"Examples\n    --------\n    >>> Model.value\n    42\n    \"\"\"\n"));
    success(command(&root, "extract"));
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let field = snapshot.workspace().packages["api"]
        .items
        .values()
        .find(|i| i.name == "value")
        .unwrap();
    assert!(field.documentation.as_ref().unwrap().provenance.iter().any(|p|
        matches!(&p.activity, ProvenanceActivity::ApiExamples { examples, .. } if examples[0].actual == "42\n")));
    root.write(
        path,
        format!(
            "__file__ = '/unrelated/checkout/apiex.py'\n{}",
            root.read(path)
        ),
    );
    let output = command(&root, "extract");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not match the supplied checkout")
    );
}
