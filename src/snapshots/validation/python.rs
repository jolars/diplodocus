//! Verify native Python completion without a Python installation or checkout.

use super::require;
use crate::ir::*;
use crate::snapshots::SnapshotError;
use std::collections::BTreeSet;

fn is_native_docstring(document: &SourcedDocument) -> bool {
    matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "python-docstring")
}

pub(super) fn validate(workspace: &Workspace) -> Result<(), SnapshotError> {
    require(
        !workspace
            .pages
            .values()
            .any(|page| is_native_docstring(&page.document))
            && !workspace
                .concepts
                .values()
                .filter_map(|concept| concept.documentation.as_ref())
                .any(is_native_docstring),
        "Python native documentation item owner",
    )?;
    for (id, package) in &workspace.packages {
        let mut verified_inputs = BTreeSet::new();
        for (target_id, target) in &package.extraction_targets {
            let native: Vec<_> = workspace.provenance.iter().filter(|record| matches!(&record.activity,
                ProvenanceActivity::Extraction { target: owner, mode: ExtractionMode::Native, .. }
                    if owner.package == *id && owner.target == *target_id && target.extractor == "python"
            )).collect();
            let completed: Vec<_> = workspace
                .provenance
                .iter()
                .filter(|record| {
                    matches!(&record.activity,
                        ProvenanceActivity::PythonRuntime { target: owner, .. }
                            if owner.package == *id && owner.target == *target_id
                    )
                })
                .collect();
            if native.is_empty() && completed.is_empty() {
                continue;
            }
            require(
                package.ecosystem == "python"
                    && target.extractor == "python"
                    && native.len() == 1
                    && completed.len() == 1,
                "Python native discovery completion",
            )?;
            let ProvenanceActivity::Extraction {
                inputs, parsers, ..
            } = &native[0].activity
            else {
                unreachable!()
            };
            let ProvenanceActivity::PythonRuntime {
                inputs: observed,
                python_version,
                sphinx_version,
                docutils_version,
                ..
            } = &completed[0].activity
            else {
                unreachable!()
            };
            require(
                native[0].tools.get("python") == Some(python_version)
                    && [("sphinx", sphinx_version), ("docutils", docutils_version)]
                        .into_iter()
                        .all(|(name, version)| {
                            native[0].tools.get(name) == Some(version)
                                && parsers.get(name).is_some_and(|parser| {
                                    &parser.version == version
                                        && parser.role == "python-native-documentation"
                                })
                        }),
                "Python native tool evidence",
            )?;
            let metadata = package
                .metadata
                .as_ref()
                .ok_or(SnapshotError::Invalid("Python native package metadata"))?
                .source();
            require(
                inputs
                    .get(&metadata.repository)
                    .is_some_and(|files| files.contains_key(&metadata.path)),
                "Python native metadata input",
            )?;
            require(
                inputs.iter().all(|(repository, files)| {
                    files.iter().all(|(path, input)| {
                        observed.iter().any(|observation| {
                            observation.source.repository == *repository
                                && observation.source.path == *path
                                && observation.fingerprint == input.fingerprint
                        })
                    })
                }),
                "Python native input evidence",
            )?;
            for (repository, files) in inputs {
                verified_inputs.extend(files.keys().map(|path| (repository, path)));
            }
        }
        let verified = |source: Option<&SourceLocation>| {
            source.is_some_and(|location| {
                verified_inputs.contains(&(&location.repository, &location.path))
            })
        };
        for item in package.items.values() {
            if let Some(ItemLanguageData::Python(data)) = &item.language_data
                && matches!(
                    &data.declaration,
                    PythonDeclaration::Module {
                        source: PythonModuleSource::Runtime,
                        ..
                    } | PythonDeclaration::Module {
                        exports: PythonExports::Runtime { .. },
                        ..
                    }
                )
            {
                require(
                    verified(item.source_location.as_ref()),
                    "Python native module completion",
                )?;
            }
            if let Some(document) = item
                .documentation
                .as_ref()
                .filter(|d| is_native_docstring(d))
            {
                require(
                    verified(document.source_location.as_ref()),
                    "Python native docstring completion",
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn runtime_only_data_requires_completion_even_when_both_records_are_absent() {
        let location = SourceLocation {
            repository: "repo".into(),
            path: "demo/__init__.py".try_into().unwrap(),
            span: None,
        };
        let mut module = Item {
            kind: ItemKind::Module,
            name: "demo".into(),
            qualified_name: "demo".into(),
            language_data: None,
            aliases: Vec::new(),
            signatures: Vec::new(),
            documentation: None,
            source_location: Some(location.clone()),
            children: Vec::new(),
            provenance: Vec::new(),
        };
        let mut package = Package {
            slug: "demo".into(),
            name: "Demo".into(),
            ecosystem: "python".into(),
            version: Some("1.0.0".into()),
            metadata: None,
            repository: "repo".into(),
            path: None,
            metadata_path: "pyproject.toml".try_into().unwrap(),
            kind: PackageKind::Package,
            visibility: PackageVisibility::Public,
            extraction_targets: BTreeMap::from([(
                "api".into(),
                ExtractionTarget {
                    extractor: "python".into(),
                    path: Some("demo".try_into().unwrap()),
                    role: "public-api".into(),
                },
            )]),
            items: BTreeMap::new(),
        };
        let workspace = |package| Workspace {
            packages: BTreeMap::from([("demo".into(), package)]),
            ..Workspace::default()
        };
        for (source, exports, valid) in [
            (
                PythonModuleSource::Implementation,
                PythonExports::Implicit,
                true,
            ),
            (PythonModuleSource::Runtime, PythonExports::Implicit, false),
            (
                PythonModuleSource::Implementation,
                PythonExports::Runtime { names: vec![] },
                false,
            ),
        ] {
            module.language_data = Some(ItemLanguageData::Python(PythonItemData {
                declaration: PythonDeclaration::Module { source, exports },
                visibility: PythonVisibility::Public,
                decorators: vec![],
            }));
            package.items.insert("demo".into(), module.clone());
            assert_eq!(validate(&workspace(package.clone())).is_ok(), valid);
        }
        module.language_data = None;
        module.documentation = Some(SourcedDocument {
            document: Document {
                span: SourceSpan { start: 0, end: 0 },
                frontmatter: None,
                blocks: vec![],
            },
            source_format: DocumentFormat::Extracted {
                name: "python-docstring".into(),
            },
            source_location: Some(location),
            raw_source: None,
            generated_source: None,
            provenance: vec![],
        });
        package.items.insert("demo".into(), module);
        assert!(matches!(
            validate(&workspace(package)),
            Err(SnapshotError::Invalid("Python native docstring completion"))
        ));
    }
}
