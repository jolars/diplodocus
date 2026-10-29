use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::documents::{AuthoredFormat, prepare_collection_document};
use crate::ir::{
    Block, ContentCollection, ContentOwner, Document, DocumentFormat, MetadataValue, Page,
    PageKind, Provenance, ProvenanceActivity, SourcedDocument,
};
use crate::provenance::builtin_tools;

pub(super) fn assemble(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    workspace: &mut Workspace,
    selections: &mut DeclaredSourceInputs,
    observed: &mut ObservedInputs,
) -> Result<BTreeMap<String, PreparedPage>, AssemblyError> {
    let mut pages = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, collection) in configuration.content.iter().enumerate() {
        let resolved = &paths.content[index];
        let repository = &paths.repositories[resolved.repository_index];
        let owner = if collection.owner == "project" {
            ContentOwner::Project
        } else {
            ContentOwner::Package {
                package: collection.owner.clone(),
            }
        };
        workspace.content_collections.insert(
            collection.id.clone(),
            ContentCollection {
                owner: owner.clone(),
                repository: collection.repository.clone(),
                path: relative(&resolved.path, &repository.path)?,
                mount: collection.mount.clone(),
                format: collection.format,
                navigation: collection.navigation,
                execution: crate::ir::ExecutionConfiguration {
                    mode: collection.execution.mode,
                    engine: collection.execution.engine,
                    kernel: collection.execution.kernel.clone(),
                    julia_project: resolved
                        .julia_project
                        .as_ref()
                        .map(|path| {
                            repository
                                .source_location(path, None)
                                .map(|location| location.path)
                        })
                        .transpose()?,
                    declared_environment_inputs: collection
                        .execution
                        .declared_environment_inputs
                        .iter()
                        .map(|path| {
                            crate::paths::portable_relative_path(
                                &paths.configuration_directory,
                                "declared_environment_inputs",
                                path,
                            )
                            .map_err(AssemblyError::from)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .chain(
                            resolved
                                .julia_environment_inputs
                                .iter()
                                .map(|path| {
                                    repository
                                        .source_location(path, None)
                                        .map(|location| location.path)
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .collect(),
                },
            },
        );
        let files = discover(&resolved.path, collection.format, repository)?;
        for relative in &files {
            let declared = resolved.source_root.join(relative);
            let location = repository.source_location(&declared, None)?;
            let path = repository.path.join(location.path.as_str());
            // Explicit overview files have one route, even when a collection contains them.
            if paths.packages.iter().any(|package| {
                package.overview.as_ref() == Some(&path)
                    && !(collection.owner == package.id
                        && collection.mount.is_empty()
                        && relative.with_extension("html") == Path::new("index.html"))
            }) {
                continue;
            }
            let key = (location.repository.clone(), location.path.clone());
            if collection.id.starts_with("rmd-vignettes:") && seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            let text = fs::read_to_string(&path)?;
            observe(observed, &location, fingerprint_bytes(text.as_bytes()))?;
            let prepared = prepare_collection_document(&text, collection).map_err(|error| {
                AssemblyError::Diagnostics(vec![error.to_diagnostic(&collection.id)])
            })?;
            let relative = portable(relative)?;
            let mut parsed = prepared.parsed.with_context(
                source(&location),
                DiagnosticEntity::Document {
                    collection: collection.id.clone(),
                    path: relative.clone(),
                },
            );
            if collection.format == AuthoredFormat::Rmd {
                let (_, mut diagnostics) = crate::documents::prepare_rmd_document(&text);
                for diagnostic in &mut diagnostics {
                    diagnostic.source = Some(source(&location));
                    diagnostic.related_entity = Some(DiagnosticEntity::Document {
                        collection: collection.id.clone(),
                        path: relative.clone(),
                    });
                }
                parsed.diagnostics.extend(diagnostics);
                for path in
                    super::rmarkdown::citations::inputs(&parsed.document, &path, repository)?
                {
                    let location = repository.source_location(&path, None)?;
                    observe(observed, &location, fingerprint_bytes(&fs::read(&path)?))?;
                    selections
                        .package_documentation
                        .entry(repository.id.clone())
                        .or_default()
                        .push(PathBuf::from(location.path.as_str()));
                }
                super::rmarkdown::citations::resolve(&mut parsed.document, &path, repository)?;
            }
            workspace.diagnostics.extend(parsed.diagnostics);
            // Length prefixes keep collection/path punctuation from aliasing another page.
            let id = format!(
                "page1:{}:{}:{}:{}",
                collection.id.len(),
                collection.id,
                relative.as_str().len(),
                relative.as_str()
            );
            let title = title(&parsed.document).unwrap_or_else(|| relative.as_str().to_owned());
            let mut tools = builtin_tools();
            if collection.format == AuthoredFormat::Documenter {
                tools.insert(
                    "fatou-parser".into(),
                    crate::extractors::julia::FATOU_VERSION.into(),
                );
            }
            workspace.pages.insert(
                id.clone(),
                Page {
                    owner: owner.clone(),
                    kind: PageKind::Authored {
                        collection: collection.id.clone(),
                    },
                    title,
                    document: SourcedDocument {
                        document: parsed.document,
                        source_format: DocumentFormat::Authored {
                            format: collection.format,
                        },
                        source_location: Some(location.clone()),
                        generated_source: None,
                        raw_source: Some(text.clone()),
                        provenance: vec![Provenance {
                            activity: ProvenanceActivity::Declaration,
                            source: Some(source(&location)),
                            span: None,
                            tools,
                        }],
                    },
                },
            );
            pages.insert(
                id,
                PreparedPage {
                    source: text,
                    path,
                    relative,
                    location,
                    collection: index,
                    preparation: prepared.preparation,
                },
            );
        }
        selections.content.insert(collection.id.clone(), files);
    }
    Ok(pages)
}

pub(super) fn discover(
    root: &Path,
    format: AuthoredFormat,
    repository: &crate::paths::ResolvedRepositoryPaths,
) -> Result<Vec<PathBuf>, AssemblyError> {
    fn visit(
        root: &Path,
        directory: &Path,
        format: AuthoredFormat,
        repository: &crate::paths::ResolvedRepositoryPaths,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), AssemblyError> {
        if crate::generated_storage::is_reserved(directory) {
            return Ok(());
        }
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            if entry.file_name() == ".git" || crate::generated_storage::is_reserved(&entry.path()) {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                // Directory links can introduce cycles and undeclared traversal.
                if fs::metadata(&path)?.is_dir() {
                    return Err(AssemblyError::Diagnostics(vec![diagnostic(
                        DiagnosticCode::SourcePathOutsideBoundary,
                        "Authored discovery does not follow directory symlinks.",
                    )]));
                }
            }
            if kind.is_dir() {
                visit(root, &path, format, repository, files)?;
            } else if format.matches_path(&path) {
                if !fs::metadata(&path)?.is_file() {
                    return Err(AssemblyError::Diagnostics(vec![diagnostic(
                        DiagnosticCode::SourcePathWrongType,
                        "An authored page must be a regular file.",
                    )]));
                }
                let location = repository.source_location(&path, None)?;
                let canonical = repository.path.join(location.path.as_str());
                if crate::generated_storage::is_generated_file(&canonical) {
                    continue;
                }
                files.push(
                    path.strip_prefix(root)
                        .expect("walk beneath root")
                        .to_owned(),
                );
            }
        }
        Ok(())
    }
    if root.is_file() {
        repository.source_location(root, None)?;
        if crate::generated_storage::is_generated_file(root)
            || crate::generated_storage::is_reserved(root)
        {
            return Ok(vec![]);
        }
        return Ok(vec![PathBuf::from(
            root.file_name().expect("authored file name"),
        )]);
    }
    let mut files = vec![];
    visit(root, root, format, repository, &mut files)?;
    files.sort();
    Ok(files)
}

fn title(document: &Document) -> Option<String> {
    if let Some(MetadataValue::Mapping { entries, .. }) = &document.frontmatter {
        for entry in entries {
            if entry.key.value == "title"
                && let MetadataValue::Scalar { value, .. } = &entry.value
            {
                return Some(value.clone());
            }
        }
    }
    document.blocks.iter().find_map(|block| match block {
        Block::Heading { inlines, .. } => Some(inline_text(inlines)),
        _ => None,
    })
}

pub(super) use crate::documents::inline_text;
