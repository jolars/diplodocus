mod support;

use std::collections::BTreeMap;
use std::path::Path;

use diplodocus::configuration::load_configuration;
use diplodocus::configuration_validation::validate_configuration;
use diplodocus::diagnostics::{DiagnosticEntity, DiagnosticPath, DiagnosticSource};
use diplodocus::documents::{AuthoredFormat, parse_collection_document};
use diplodocus::ir::{
    DocumentFormat, Provenance, ProvenanceActivity, SourcedDocument, TargetReference,
};
use diplodocus::paths::resolve_workspace_paths;
use diplodocus::provenance::{DeclaredSourceInputs, builtin_tools, collect_static_provenance};

fn snapshot(workspace: &support::TestWorkspace, reverse_inputs: bool) -> serde_json::Value {
    let configuration_path = workspace.path().join("workspace/diplodocus.toml");
    let configuration = load_configuration(&configuration_path).unwrap();
    assert!(validate_configuration(&configuration).is_empty());
    let resolved = resolve_workspace_paths(&configuration_path, &configuration).unwrap();
    let mut selection = DeclaredSourceInputs::default();
    // Enumerating the fixture is test setup. Production extractors will supply
    // the files they actually consume through the same explicit input boundary.
    for package in &resolved.packages {
        for target in &package.targets {
            let paths = support::files_under(&target.path)
                .into_iter()
                .map(|path| {
                    target
                        .path
                        .join(path)
                        .strip_prefix(&package.path)
                        .unwrap()
                        .to_owned()
                })
                .collect();
            selection.extraction.insert(
                TargetReference {
                    package: package.id.clone(),
                    target: target.id.clone(),
                },
                paths,
            );
        }
    }
    let mut documents = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for collection in &configuration.content {
        let paths = resolved
            .content
            .iter()
            .find(|paths| paths.id == collection.id)
            .unwrap();
        let files = support::files_under(&paths.path);
        selection
            .content
            .insert(collection.id.clone(), files.clone());
        let repository = &resolved.repositories[paths.repository_index];
        let extension = match collection.format {
            AuthoredFormat::Gfm => "md",
            AuthoredFormat::Qmd => "qmd",
        };
        for relative in files {
            if relative.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
            let absolute = paths.path.join(&relative);
            let source_location = repository.source_location(&absolute, None).unwrap();
            let source = DiagnosticSource::Repository {
                repository: source_location.repository.clone(),
                path: source_location.path.clone(),
            };
            let parsed =
                parse_collection_document(&std::fs::read_to_string(&absolute).unwrap(), collection)
                    .unwrap()
                    .with_context(
                        source.clone(),
                        DiagnosticEntity::Document {
                            collection: collection.id.clone(),
                            path: DiagnosticPath::try_from(relative.to_str().unwrap()).unwrap(),
                        },
                    );
            diagnostics.extend(parsed.diagnostics);
            let document = SourcedDocument {
                document: parsed.document,
                source_format: DocumentFormat::Authored {
                    format: collection.format,
                },
                source_location: Some(source_location),
                raw_source: None,
                provenance: vec![Provenance {
                    activity: ProvenanceActivity::Declaration,
                    source: Some(source),
                    span: None,
                    tools: builtin_tools(),
                }],
            };
            assert!(
                documents
                    .insert(
                        format!("{}/{}", collection.id, relative.display()),
                        document
                    )
                    .is_none()
            );
        }
    }
    if reverse_inputs {
        for inputs in selection
            .extraction
            .values_mut()
            .chain(selection.content.values_mut())
        {
            inputs.reverse();
        }
    }
    let evidence =
        collect_static_provenance(&configuration_path, &configuration, &selection).unwrap();
    assert!(
        evidence
            .repositories
            .values()
            .all(|repository| repository.declared_input_fingerprint.is_some())
    );
    assert_eq!(
        documents.len(),
        support::authored_sources(workspace, "workspace/diplodocus.toml").len()
    );
    diagnostics.sort();
    serde_json::json!({
        "configuration": configuration,
        "evidence": evidence,
        "documents": documents,
        "diagnostics": diagnostics,
    })
}

#[test]
fn acceptance_configuration_documents_and_evidence_are_portable_together() {
    let first_workspace = support::acceptance_workspace();
    let second_workspace = support::acceptance_workspace();
    assert_ne!(first_workspace.path(), second_workspace.path());
    let first = snapshot(&first_workspace, false);
    assert_eq!(first["diagnostics"], serde_json::json!([]));
    assert_eq!(
        first["evidence"]["repositories"].as_object().unwrap().len(),
        3
    );
    let second = snapshot(&second_workspace, true);
    assert_eq!(first, second);
    let bytes = serde_json::to_string_pretty(&first).unwrap();
    assert_eq!(bytes, serde_json::to_string_pretty(&second).unwrap());
    for root in [
        first_workspace.path(),
        second_workspace.path(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    ] {
        assert!(!bytes.contains(root.to_str().unwrap()));
    }
}

#[test]
fn acceptance_authority_and_unsupported_syntax_keep_portable_diagnostics() {
    for (case, expected) in [
        (
            "document-execution-not-authorized",
            "document-execution-not-authorized",
        ),
        ("qmd-unsupported-directive", "unsupported-authored-syntax"),
    ] {
        let first_workspace = support::acceptance_case(case);
        let second_workspace = support::acceptance_case(case);
        let first = snapshot(&first_workspace, false);
        let second = snapshot(&second_workspace, true);
        assert_eq!(first, second);
        let diagnostics = first["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1, "{case}");
        assert_eq!(diagnostics[0]["code"], expected);
        assert_eq!(diagnostics[0]["source"]["repository"], "python");
        assert!(diagnostics[0]["span"].is_object());
        support::assert_json_golden(&diagnostics, format!("milestone-three/{case}.json"));
    }
}
