use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::documents::{AuthoredFormat, prepare_collection_document};
use crate::ir::{
    Block, ContentCollection, ContentOwner, Document, DocumentFormat, Inline, MetadataValue, Page,
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
                execution: crate::ir::ExecutionConfiguration {
                    mode: collection.execution.mode,
                    engine: collection.execution.engine,
                    kernel: collection.execution.kernel.clone(),
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
                        .collect::<Result<_, _>>()?,
                },
            },
        );
        let files = discover(&resolved.path, collection.format)?;
        for relative in &files {
            let declared = resolved.path.join(relative);
            let location = repository.source_location(&declared, None)?;
            let path = repository.path.join(location.path.as_str());
            let text = fs::read_to_string(&path)?;
            observe(observed, &location, fingerprint_bytes(text.as_bytes()))?;
            let prepared = prepare_collection_document(&text, collection).map_err(|error| {
                AssemblyError::Diagnostics(vec![error.to_diagnostic(&collection.id)])
            })?;
            let relative = portable(relative)?;
            let parsed = prepared.parsed.with_context(
                source(&location),
                DiagnosticEntity::Document {
                    collection: collection.id.clone(),
                    path: relative.clone(),
                },
            );
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
                        raw_source: Some(text.clone()),
                        provenance: vec![Provenance {
                            activity: ProvenanceActivity::Declaration,
                            source: Some(source(&location)),
                            span: None,
                            tools: builtin_tools(),
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

pub(super) fn discover(root: &Path, format: AuthoredFormat) -> Result<Vec<PathBuf>, AssemblyError> {
    fn visit(
        root: &Path,
        directory: &Path,
        extension: &str,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), AssemblyError> {
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            if matches!(entry.file_name().to_str(), Some(".git" | ".diplodocus")) {
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
                visit(root, &path, extension, files)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some(extension) {
                if !fs::metadata(&path)?.is_file() {
                    return Err(AssemblyError::Diagnostics(vec![diagnostic(
                        DiagnosticCode::SourcePathWrongType,
                        "An authored page must be a regular file.",
                    )]));
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
    let mut files = vec![];
    visit(
        root,
        root,
        match format {
            AuthoredFormat::Gfm => "md",
            AuthoredFormat::Qmd => "qmd",
        },
        &mut files,
    )?;
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

fn inline_text(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text { value, .. } | Inline::Code { value, .. } => value.clone(),
            Inline::Space { .. }
            | Inline::SoftBreak { .. }
            | Inline::HardBreak { .. }
            | Inline::NonbreakingSpace { .. } => " ".into(),
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. }
            | Inline::Link { inlines, .. } => inline_text(inlines),
            Inline::Image { alt, .. } => inline_text(alt),
            Inline::AutoLink { target, .. } | Inline::SemanticReference { target, .. } => {
                target.clone()
            }
            Inline::Unsupported { raw, .. } => raw.clone(),
        })
        .collect()
}
