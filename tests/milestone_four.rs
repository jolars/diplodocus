mod support;

use diplodocus::ir::{
    ExtractionMode, ItemLanguageData, ProvenanceActivity, PythonCallableRole, PythonDeclaration,
};

#[test]
fn native_python_discovery_retains_stub_signatures_and_parser_provenance() {
    let workspace = support::prepared_acceptance_workspace();
    let result = support::discover_api(&workspace.path().join("core/diplodocus.yaml"), "pyfoo");
    let extraction = result.extraction();
    let ProvenanceActivity::Extraction { mode, parsers, .. } = &extraction.activity else {
        panic!("extraction");
    };
    assert_eq!(*mode, ExtractionMode::Native);
    assert!(parsers.contains_key("sphinx"));
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    for name in [
        "pyproject-toml",
        "ruff_python_parser",
        "ruff_python_ast",
        "ruff_text_size",
    ] {
        assert_eq!(
            manifest["dependencies"][name].as_str(),
            Some(format!("={}", parsers[name].version).as_str()),
        );
        assert_eq!(extraction.tools[name], parsers[name].version);
    }

    let overloads = result.items.values().filter(|item| {
        matches!(
            &item.language_data,
            Some(ItemLanguageData::Python(data)) if matches!(data.declaration,
                PythonDeclaration::Callable { role: PythonCallableRole::Overload { .. }, .. })
        )
    });
    assert_eq!(overloads.count(), 4);
    let fit = result
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
            .any(|evidence| evidence.source.path.as_str().ends_with(".pyi"))
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
}
