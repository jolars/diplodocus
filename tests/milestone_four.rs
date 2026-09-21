mod support;

use diplodocus::configuration::load_configuration;
use diplodocus::extractors::python::{PythonExtraction, extract_target};
use diplodocus::ir::{ItemLanguageData, ProvenanceActivity, PythonCallableRole, PythonDeclaration};
use diplodocus::paths::resolve_workspace_paths;

fn extract(workspace: &support::TestWorkspace) -> PythonExtraction {
    let path = workspace.path().join("workspace/diplodocus.toml");
    let configuration = load_configuration(&path).unwrap();
    let paths = resolve_workspace_paths(&path, &configuration).unwrap();
    let package = paths.packages.iter().find(|p| p.id == "pyfoo").unwrap();
    extract_target(
        &paths.repositories[package.repository_index],
        package,
        &package.targets[0],
    )
}

fn normalize_versions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(tools) = object.get_mut("tools").and_then(|v| v.as_object_mut()) {
                for name in ["diplodocus", "python"] {
                    if let Some(version) = tools.get_mut(name) {
                        assert_eq!(version, env!("CARGO_PKG_VERSION"));
                        *version = serde_json::json!("[DIPLODOCUS_VERSION]");
                    }
                }
            }
            for child in object.values_mut() {
                normalize_versions(child);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                normalize_versions(child);
            }
        }
        _ => {}
    }
}

#[test]
fn parser_provenance_matches_exact_dependency_pins() {
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    let result = extract(&support::acceptance_workspace());
    let assert_version = |name: &str, version: &str| {
        assert_eq!(
            manifest["dependencies"][name].as_str(),
            Some(format!("={version}").as_str()),
            "provenance must report the pinned version of {name}"
        );
    };
    let assert_tools = |provenance: &diplodocus::ir::Provenance| {
        for (name, version) in &provenance.tools {
            if name != "diplodocus" && name != "python" {
                assert_version(name, version);
            }
        }
    };
    let ProvenanceActivity::Extraction { parsers, .. } = &result.provenance.activity else {
        panic!("extraction");
    };
    for name in [
        "panache-parser",
        "pydocstring",
        "pyproject-toml",
        "ruff_python_parser",
        "ruff_python_ast",
        "ruff_text_size",
    ] {
        assert_version(name, &parsers[name].version);
        assert_version(name, &result.provenance.tools[name]);
    }
    for item in result.items.values() {
        for provenance in &item.provenance {
            assert_tools(provenance);
        }
        if let Some(documentation) = &item.documentation {
            for provenance in &documentation.provenance {
                assert_tools(provenance);
            }
        }
    }
}

#[test]
fn complete_python_extraction_matches_portable_acceptance_ir() {
    let first_workspace = support::acceptance_workspace();
    let second_workspace = support::acceptance_workspace();
    let first = extract(&first_workspace);
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    assert_eq!(first, extract(&second_workspace));
    assert_eq!(first.items.len(), 26);
    assert_eq!(first.metadata.as_ref().unwrap().version, "1.9.0");
    let overloads = first
        .items
        .values()
        .filter(|item| {
            matches!(
                &item.language_data,
                Some(ItemLanguageData::Python(data)) if matches!(data.declaration,
                    PythonDeclaration::Callable { role: PythonCallableRole::Overload { .. }, .. })
            )
        })
        .count();
    assert_eq!(overloads, 4);
    let documented = first
        .items
        .values()
        .filter(|item| item.documentation.is_some())
        .count();
    assert_eq!(documented, 16);
    let fit = first
        .items
        .values()
        .find(|item| item.qualified_name == "foo.model.fit" && item.documentation.is_some())
        .unwrap();
    assert!(
        fit.aliases
            .iter()
            .any(|alias| alias.qualified_name == "foo.fit")
    );
    assert!(fit.signatures.iter().all(|signature| {
        signature
            .sources
            .iter()
            .any(|source| source.source.path.as_str().ends_with(".pyi"))
    }));
    let doc = fit.documentation.as_ref().unwrap();
    assert!(
        doc.source_location
            .as_ref()
            .unwrap()
            .path
            .as_str()
            .ends_with(".py")
    );
    assert!(
        !serde_json::to_string(&doc.document)
            .unwrap()
            .contains("code-cell")
    );
    let ProvenanceActivity::Extraction {
        parsers,
        inputs,
        capabilities,
        ..
    } = &first.provenance.activity
    else {
        panic!("extraction");
    };
    assert_eq!(parsers["pydocstring"].version, "0.4.1");
    assert_eq!(parsers["panache-parser"].version, "0.29.2");
    assert!(capabilities.contains("python.docs.numpy"));
    assert_eq!(inputs["python"].len(), 8);
    assert!(
        inputs["python"][&"python/foo/model.py".try_into().unwrap()]
            .parsers
            .contains("pydocstring")
    );
    let bytes = serde_json::to_string_pretty(&first).unwrap();
    assert!(!bytes.contains(first_workspace.path().to_str().unwrap()));
    assert!(!bytes.contains(second_workspace.path().to_str().unwrap()));
    let mut golden = serde_json::to_value(first).unwrap();
    normalize_versions(&mut golden);
    support::assert_json_golden(&golden, "milestone-four/acceptance.json");
}

#[test]
fn dynamic_exports_keep_the_single_expected_diagnostic() {
    let workspace = support::acceptance_case("python-dynamic-export");
    let first = extract(&workspace);
    assert_eq!(first.diagnostics.len(), 1, "{:?}", first.diagnostics);
    assert_eq!(first.diagnostics[0].code.as_str(), "python-dynamic-export");
    assert_eq!(
        first,
        extract(&support::acceptance_case("python-dynamic-export"))
    );
    support::assert_json_golden(
        &first.diagnostics,
        "milestone-four/dynamic-diagnostics.json",
    );
}

#[test]
fn extraction_does_not_execute_imports_or_build_backend_hooks() {
    let workspace = support::acceptance_workspace();
    let mut source = workspace.read("python/python/foo/__init__.py");
    source.push_str("\nraise RuntimeError('must not import documented package')\n");
    workspace.write("python/python/foo/__init__.py", source);
    let metadata = workspace
        .read("python/pyproject.toml")
        .replace("setuptools.build_meta", "must_not_run");
    workspace.write("python/pyproject.toml", metadata);
    let first = extract(&workspace);
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    assert_eq!(first.items, extract(&support::acceptance_workspace()).items);
    assert!(!workspace.path().join("site").exists());
}

#[test]
fn empty_target_never_claims_docstring_parser_observations() {
    let workspace = support::acceptance_workspace();
    for file in support::files_under(&workspace.path().join("python/python/foo")) {
        if matches!(
            file.extension().and_then(|e| e.to_str()),
            Some("py" | "pyi")
        ) {
            workspace.write(std::path::Path::new("python/python/foo").join(file), "");
        }
    }
    let result = extract(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let ProvenanceActivity::Extraction { parsers, .. } = &result.provenance.activity else {
        panic!("extraction");
    };
    assert!(!parsers.contains_key("pydocstring"));
    assert!(!parsers.contains_key("panache-parser"));

    workspace.write("python/python/foo/__init__.py", "\"\"\"\"\"\"\n");
    let result = extract(&workspace);
    let ProvenanceActivity::Extraction { parsers, .. } = &result.provenance.activity else {
        panic!("extraction");
    };
    assert!(parsers.contains_key("pydocstring"));
    assert!(!parsers.contains_key("panache-parser"));
}
