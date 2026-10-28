//! Package overviews and conventional R documents share the authored link and asset pipeline.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::documents::{AuthoredFormat, parse_authored_document};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, resolve_input_file};
use crate::provenance::builtin_tools;

mod native;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PackageFiles {
    pub overview: Option<PathBuf>,
    pub news: Option<PathBuf>,
    pub citation: Option<PathBuf>,
    pub licenses: Vec<PathBuf>,
    pub community: Vec<PathBuf>,
}

impl PackageFiles {
    fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.overview
            .iter()
            .chain(self.news.iter())
            .chain(self.citation.iter())
            .chain(&self.licenses)
            .chain(&self.community)
    }
}

pub(super) fn discover(
    paths: &ResolvedWorkspacePaths,
    workspace: &Workspace,
) -> Result<BTreeMap<String, PackageFiles>, AssemblyError> {
    let mut result = BTreeMap::new();
    for package in &paths.packages {
        let repository = &paths.repositories[package.repository_index];
        let overview = match &package.overview {
            Some(path) => Some(path.clone()),
            None => optional(&package.path, "README.md", repository)?,
        };
        let mut licenses = Vec::new();
        for name in PACKAGE_LICENSE_FILENAMES {
            if let Some(path) = optional(&package.path, name, repository)? {
                licenses.push(path);
            }
        }
        if let Some(PackageMetadata::R(metadata)) = &workspace.packages[&package.id].metadata
            && let Some(license) = &metadata.description.license
        {
            for name in crate::extractors::r::license_files(license) {
                let path = resolve_input_file(
                    &repository.path,
                    "package.license",
                    Path::new(name.trim()),
                    &package.path,
                )?;
                licenses.push(path);
            }
        }
        licenses.sort();
        licenses.dedup();
        if !matches!(
            &workspace.packages[&package.id].metadata,
            Some(PackageMetadata::R(_))
        ) {
            result.insert(
                package.id.clone(),
                PackageFiles {
                    overview,
                    news: None,
                    citation: None,
                    licenses,
                    community: Vec::new(),
                },
            );
            continue;
        }
        let mut community = Vec::new();
        for name in ["CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "CONDUCT.md"] {
            let selected = optional(&package.path, name, repository)?;
            if let Some(path) = match selected {
                Some(path) => Some(path),
                None => optional(&repository.path, name, repository)?,
            } {
                community.push(path);
            }
        }
        result.insert(
            package.id.clone(),
            PackageFiles {
                overview,
                news: first(&package.path, &["NEWS.md", "NEWS.Rd", "NEWS"], repository)?,
                citation: optional(&package.path, "inst/CITATION", repository)?,
                licenses,
                community,
            },
        );
    }
    Ok(result)
}

fn optional(
    root: &Path,
    name: &str,
    repository: &ResolvedRepositoryPaths,
) -> Result<Option<PathBuf>, AssemblyError> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(_) => {
            let path = resolve_input_file(
                &repository.path,
                "package.documentation",
                Path::new(name),
                root,
            )?;
            if crate::generated_storage::is_generated_file(&path) {
                Ok(None)
            } else {
                Ok(Some(path))
            }
        }
    }
}

fn first(
    root: &Path,
    names: &[&str],
    repository: &ResolvedRepositoryPaths,
) -> Result<Option<PathBuf>, AssemblyError> {
    for name in names {
        if let Some(path) = optional(root, name, repository)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub(super) fn assemble(
    paths: &ResolvedWorkspacePaths,
    workspace: &mut Workspace,
    prepared_pages: &BTreeMap<String, PreparedPage>,
    selections: &mut DeclaredSourceInputs,
    observed: &mut ObservedInputs,
) -> Result<BTreeMap<String, PackageFiles>, AssemblyError> {
    let files = discover(paths, workspace)?;
    for (id, selected) in &files {
        let package = paths
            .packages
            .iter()
            .find(|package| &package.id == id)
            .expect("declared package");
        let repository = &paths.repositories[package.repository_index];
        for path in selected.paths() {
            let location = repository.source_location(path, None)?;
            let bytes = fs::read(path)?;
            observe(observed, &location, fingerprint_bytes(&bytes))?;
            selections
                .package_documentation
                .entry(repository.id.clone())
                .or_default()
                .push(
                    path.strip_prefix(&repository.path)
                        .map_err(|_| AssemblyError::InputsChanged)?
                        .to_owned(),
                );
        }
        let overview_overridden = register_authored_override(
            workspace,
            prepared_pages,
            id,
            PackagePageRole::Overview,
            repository,
            selected.overview.iter(),
        )?;
        if !overview_overridden {
            let overview = if let Some(path) = selected
                .overview
                .as_ref()
                .filter(|path| !already_authored(workspace, repository, path))
            {
                markdown(workspace, id, repository, path, true)?
            } else {
                let introduction = workspace.packages[id]
                    .metadata
                    .as_ref()
                    .and_then(PackageMetadata::description)
                    .unwrap_or_default()
                    .to_owned();
                generated(
                    if introduction.is_empty() {
                        Vec::new()
                    } else {
                        vec![paragraph(&introduction)]
                    },
                    introduction,
                )
            };
            insert(
                workspace,
                prepared_pages,
                id,
                PackagePageRole::Overview,
                overview,
            );
        }
        let license_overridden = register_authored_override(
            workspace,
            prepared_pages,
            id,
            PackagePageRole::License,
            repository,
            &selected.licenses,
        )?;
        let license = workspace.packages[id]
            .metadata
            .as_ref()
            .and_then(PackageMetadata::license)
            .map(str::to_owned);
        if !license_overridden
            && (!selected.licenses.is_empty()
                || (matches!(
                    &workspace.packages[id].metadata,
                    Some(PackageMetadata::R(_))
                ) && license.is_some()))
        {
            let mut document = generated(Vec::new(), String::new());
            if let Some(license) = &license {
                document.document.blocks.push(paragraph(license));
                document.raw_source = Some(license.clone());
            }
            for path in selected
                .licenses
                .iter()
                .filter(|path| !already_authored(workspace, repository, path))
                .collect::<Vec<_>>()
            {
                let addition = if path.extension().is_some_and(|extension| extension == "md") {
                    markdown(workspace, id, repository, path, false)?
                } else {
                    text_document(repository, path)?
                };
                merge(&mut document, addition, repository);
            }
            insert(
                workspace,
                prepared_pages,
                id,
                PackagePageRole::License,
                document,
            );
        }
        let Some(PackageMetadata::R(metadata)) = &workspace.packages[id].metadata else {
            continue;
        };
        let metadata = metadata.description.clone();
        let news_overridden = register_authored_override(
            workspace,
            prepared_pages,
            id,
            PackagePageRole::News,
            repository,
            selected.news.iter(),
        )?;
        let community_overridden = register_authored_override(
            workspace,
            prepared_pages,
            id,
            PackagePageRole::Community,
            repository,
            &selected.community,
        )?;
        for expression in metadata
            .fields
            .get("Authors@R")
            .into_iter()
            .map(|field| (&field.value, &field.source))
        {
            let parsed = arity_parser::parser::parse(expression.0);
            if !parsed.diagnostics.is_empty() {
                return Err(AssemblyError::Diagnostics(vec![native::diagnostic(
                    id,
                    expression.1,
                    "Invalid Authors@R syntax.",
                )]));
            }
        }
        if let Some(path) = &selected.citation {
            let text = fs::read_to_string(path)?;
            if !arity_parser::parser::parse(&text).diagnostics.is_empty() {
                return Err(AssemblyError::Diagnostics(vec![native::diagnostic(
                    id,
                    &repository.source_location(path, None)?,
                    "Invalid CITATION syntax.",
                )]));
            }
        }
        if !news_overridden
            && let Some(path) = &selected.news
            && !already_authored(workspace, repository, path)
            && path.extension().is_some_and(|extension| extension == "Rd")
        {
            let parsed = rd_source::parse(fs::read(path)?.as_slice()).map_err(|error| {
                AssemblyError::Diagnostics(vec![native::diagnostic(
                    id,
                    &repository
                        .source_location(path, None)
                        .expect("validated source"),
                    error.to_string(),
                )])
            })?;
            if parsed
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic.severity(), rd_source::Severity::Error))
            {
                return Err(AssemblyError::Diagnostics(vec![native::diagnostic(
                    id,
                    &repository.source_location(path, None)?,
                    "Invalid NEWS.Rd syntax.",
                )]));
            }
        }
        if !news_overridden
            && let Some(path) = selected
                .news
                .as_ref()
                .filter(|path| !already_authored(workspace, repository, path))
        {
            let document = if path.extension().is_some_and(|extension| extension == "md") {
                markdown(workspace, id, repository, path, false)?
            } else {
                text_document(repository, path)?
            };
            insert(
                workspace,
                prepared_pages,
                id,
                PackagePageRole::News,
                document,
            );
        }
        if !community_overridden {
            let mut community = generated(Vec::new(), String::new());
            for path in selected
                .community
                .iter()
                .filter(|path| !already_authored(workspace, repository, path))
                .collect::<Vec<_>>()
            {
                let addition = markdown(workspace, id, repository, path, false)?;
                merge(&mut community, addition, repository);
            }
            if !community.document.blocks.is_empty() {
                insert(
                    workspace,
                    prepared_pages,
                    id,
                    PackagePageRole::Community,
                    community,
                );
            }
        }
        if metadata.fields.contains_key("Authors@R")
            || metadata.fields.contains_key("Author")
            || metadata.fields.contains_key("Maintainer")
            || selected.citation.is_some()
        {
            let blocks = ["Author", "Maintainer"]
                .into_iter()
                .filter_map(|key| metadata.fields.get(key))
                .map(|field| paragraph(&field.value))
                .collect();
            insert(
                workspace,
                prepared_pages,
                id,
                PackagePageRole::Authors,
                generated(blocks, String::new()),
            );
        }
    }
    Ok(files)
}

fn already_authored(
    workspace: &Workspace,
    repository: &ResolvedRepositoryPaths,
    path: &Path,
) -> bool {
    workspace.pages.values().any(|page| {
        matches!(page.kind, PageKind::Authored { .. })
            && page
                .document
                .source_location
                .as_ref()
                .is_some_and(|source| {
                    source.repository == repository.id
                        && repository.path.join(source.path.as_str()) == path
                })
    })
}

fn authored_override(
    workspace: &Workspace,
    prepared_pages: &BTreeMap<String, PreparedPage>,
    id: &str,
    role: PackagePageRole,
) -> Option<String> {
    workspace.pages.iter().find_map(|(page_id, page)| {
        let PageKind::Authored { collection } = &page.kind else {
            return None;
        };
        let collection = &workspace.content_collections[collection];
        let matches = page.owner == ContentOwner::Package { package: id.into() }
            && collection.mount.is_empty()
            && prepared_pages.get(page_id).is_some_and(|prepared| {
                Path::new(prepared.relative.as_str()).with_extension("html")
                    == Path::new(role.filename())
            });
        matches.then(|| page_id.clone())
    })
}

fn register_authored_override<'a>(
    workspace: &mut Workspace,
    prepared_pages: &BTreeMap<String, PreparedPage>,
    id: &str,
    role: PackagePageRole,
    repository: &ResolvedRepositoryPaths,
    paths: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<bool, AssemblyError> {
    let Some(page_id) = authored_override(workspace, prepared_pages, id, role) else {
        return Ok(false);
    };
    // Source aliases keep incoming links working without parsing replaced content.
    let aliases = paths
        .into_iter()
        .filter(|path| !already_authored(workspace, repository, path))
        .map(|path| {
            repository
                .source_location(path, None)
                .map(|source| declaration(&source))
        })
        .collect::<Result<Vec<_>, _>>()?;
    workspace
        .pages
        .get_mut(&page_id)
        .expect("authored override")
        .document
        .provenance
        .extend(aliases);
    Ok(true)
}

pub(super) fn page_id(id: &str, role: PackagePageRole) -> String {
    format!("package-page1:{}:{id}:{}", id.len(), role.filename())
}

fn insert(
    workspace: &mut Workspace,
    prepared_pages: &BTreeMap<String, PreparedPage>,
    id: &str,
    role: PackagePageRole,
    document: SourcedDocument,
) {
    if let Some(page_id) = authored_override(workspace, prepared_pages, id, role) {
        let mut aliases: Vec<_> = document
            .provenance
            .into_iter()
            .filter(|record| matches!(record.activity, ProvenanceActivity::Declaration))
            .collect();
        if let Some(location) = document.source_location {
            aliases.push(declaration(&location));
        }
        workspace
            .pages
            .get_mut(&page_id)
            .expect("authored override")
            .document
            .provenance
            .extend(aliases);
        return;
    }
    let title = if role == PackagePageRole::Overview {
        workspace.packages[id].name.clone()
    } else {
        role.title().into()
    };
    workspace.pages.insert(
        page_id(id, role),
        Page {
            owner: ContentOwner::Package { package: id.into() },
            kind: PageKind::PackageDocumentation { role },
            title,
            document,
        },
    );
}

fn markdown(
    workspace: &mut Workspace,
    id: &str,
    repository: &ResolvedRepositoryPaths,
    path: &Path,
    overview: bool,
) -> Result<SourcedDocument, AssemblyError> {
    let location = repository.source_location(path, None)?;
    let text = fs::read_to_string(path)?;
    let parsed = parse_authored_document(&text, AuthoredFormat::Gfm).with_context(
        source(&location),
        DiagnosticEntity::Package { id: id.into() },
    );
    workspace.diagnostics.extend(parsed.diagnostics);
    let mut document = parsed.document;
    let mut identifiers = crate::validation::document_anchors(&document.blocks);
    identifiers.extend(["main", "search", "search-results"].map(str::to_owned));
    heading_identifiers(&mut document.blocks, &mut identifiers);
    let title_heading = overview
        && matches!(
            document.blocks.first(),
            Some(Block::Heading { level: 1, .. })
        );
    for block in &mut document.blocks {
        if let Block::Heading { level, .. } = block
            && *level == 1
        {
            *level = 2;
        }
    }
    if title_heading
        && let Some(Block::Heading {
            level: 2, inlines, ..
        }) = document.blocks.first_mut()
    {
        *inlines = vec![Inline::Text {
            value: workspace.packages[id].name.clone(),
            span: document.span,
        }];
        if let Some(Block::Heading { level, .. }) = document.blocks.first_mut() {
            *level = 1;
        }
    }
    Ok(SourcedDocument {
        document,
        source_format: DocumentFormat::Authored {
            format: AuthoredFormat::Gfm,
        },
        source_location: Some(location.clone()),
        generated_source: None,
        raw_source: Some(text),
        provenance: vec![declaration(&location)],
    })
}

fn text_document(
    repository: &ResolvedRepositoryPaths,
    path: &Path,
) -> Result<SourcedDocument, AssemblyError> {
    let text = fs::read_to_string(path)?;
    let location = repository.source_location(path, None)?;
    let mut document = generated(vec![code(&text, None)], text);
    document.source_location = Some(location.clone());
    document.provenance.push(declaration(&location));
    Ok(document)
}

fn declaration(location: &SourceLocation) -> Provenance {
    Provenance {
        activity: ProvenanceActivity::Declaration,
        source: Some(source(location)),
        span: None,
        tools: builtin_tools(),
    }
}

fn heading_identifiers(blocks: &mut [Block], identifiers: &mut std::collections::BTreeSet<String>) {
    for block in blocks {
        match block {
            Block::Heading {
                inlines,
                attributes,
                span,
                ..
            } if attributes.identifier.is_none() => {
                let label = super::authored::inline_text(inlines);
                let slug = label
                    .to_lowercase()
                    .split(|character: char| !character.is_alphanumeric())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
                let base = if slug.is_empty() { "section" } else { &slug };
                let mut identifier = base.to_owned();
                let mut suffix = 2;
                while !identifiers.insert(identifier.clone()) {
                    identifier = format!("{base}-{suffix}");
                    suffix += 1;
                }
                attributes.identifier = Some(SpannedString {
                    value: identifier,
                    span: *span,
                });
            }
            Block::BlockQuote { blocks, .. }
            | Block::Container { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::Callout { blocks, .. } => heading_identifiers(blocks, identifiers),
            Block::List { items, .. } => {
                for item in items {
                    heading_identifiers(&mut item.blocks, identifiers);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        heading_identifiers(&mut cell.blocks, identifiers);
                    }
                }
            }
            _ => {}
        }
    }
}

fn generated(blocks: Vec<Block>, text: String) -> SourcedDocument {
    SourcedDocument {
        document: Document {
            span: SourceSpan {
                start: 0,
                end: text.len(),
            },
            frontmatter: None,
            blocks,
        },
        source_format: DocumentFormat::Generated,
        source_location: None,
        generated_source: None,
        raw_source: Some(text),
        provenance: vec![],
    }
}

fn paragraph(text: &str) -> Block {
    Block::Paragraph {
        inlines: vec![Inline::Text {
            value: text.into(),
            span: SourceSpan {
                start: 0,
                end: text.len(),
            },
        }],
        span: SourceSpan {
            start: 0,
            end: text.len(),
        },
    }
}

fn code(text: &str, language: Option<&str>) -> Block {
    Block::CodeBlock {
        language: language.map(str::to_owned),
        source: text.into(),
        source_segments: vec![],
        span: SourceSpan {
            start: 0,
            end: text.len(),
        },
    }
}

fn heading(title: &str) -> Block {
    Block::Heading {
        level: 2,
        inlines: vec![Inline::Text {
            value: title.into(),
            span: SourceSpan {
                start: 0,
                end: title.len(),
            },
        }],
        attributes: Attributes {
            identifier: Some(SpannedString {
                value: title.to_lowercase().replace(' ', "-"),
                span: SourceSpan {
                    start: 0,
                    end: title.len(),
                },
            }),
            ..Attributes::default()
        },
        span: SourceSpan {
            start: 0,
            end: title.len(),
        },
    }
}

fn merge(
    document: &mut SourcedDocument,
    mut addition: SourcedDocument,
    repository: &ResolvedRepositoryPaths,
) {
    if let (Some(base), Some(origin)) = (&document.source_location, &addition.source_location) {
        rebase_blocks(&mut addition.document.blocks, base, origin, repository);
    } else if document.source_location.is_none() {
        document.source_location = addition.source_location.clone();
    }
    document.document.blocks.extend(addition.document.blocks);
    document
        .raw_source
        .get_or_insert_default()
        .push_str(&format!("\n{}", addition.raw_source.unwrap_or_default()));
    document.document.span.end = document.raw_source.as_ref().map_or(0, String::len);
    document.provenance.extend(addition.provenance);
}

fn rebase_blocks(
    blocks: &mut [Block],
    base: &SourceLocation,
    origin: &SourceLocation,
    repository: &ResolvedRepositoryPaths,
) {
    for block in blocks {
        match block {
            Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                rebase_inlines(inlines, base, origin, repository)
            }
            Block::BlockQuote { blocks, .. }
            | Block::Container { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::Callout { blocks, .. } => rebase_blocks(blocks, base, origin, repository),
            Block::List { items, .. } => {
                for item in items {
                    rebase_blocks(&mut item.blocks, base, origin, repository);
                }
            }
            Block::Table { caption, rows, .. } => {
                rebase_inlines(caption, base, origin, repository);
                for row in rows {
                    for cell in &mut row.cells {
                        rebase_blocks(&mut cell.blocks, base, origin, repository);
                    }
                }
            }
            _ => {}
        }
    }
}

fn rebase_inlines(
    inlines: &mut [Inline],
    base: &SourceLocation,
    origin: &SourceLocation,
    repository: &ResolvedRepositoryPaths,
) {
    for inline in inlines {
        match inline {
            Inline::Link {
                target, inlines, ..
            } => {
                rebase_target(target, base, origin, repository);
                rebase_inlines(inlines, base, origin, repository);
            }
            Inline::Image { target, alt, .. } => {
                rebase_target(target, base, origin, repository);
                rebase_inlines(alt, base, origin, repository);
            }
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Styled { inlines, .. }
            | Inline::Citation { inlines, .. }
            | Inline::Strikeout { inlines, .. } => {
                rebase_inlines(inlines, base, origin, repository)
            }
            _ => {}
        }
    }
}

fn rebase_target(
    target: &mut String,
    base: &SourceLocation,
    origin: &SourceLocation,
    repository: &ResolvedRepositoryPaths,
) {
    if target.starts_with('#') || url::Url::parse(target).is_ok() || target.starts_with('/') {
        return;
    }
    let root = url::Url::from_directory_path(&repository.path).expect("absolute repository");
    if let (Ok(base), Ok(origin)) = (
        root.join(base.path.as_str()),
        root.join(origin.path.as_str()),
    ) && let Ok(destination) = origin.join(target)
        && let Some(relative) = base.make_relative(&destination)
    {
        *target = relative;
    }
}

impl WorkspaceSources {
    pub(crate) async fn complete_package_metadata(
        &mut self,
        deadlines: crate::execution::ExecutionDeadlines,
        cancellation: &mut crate::execution::ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        self.revalidate()?;
        for (id, files) in self.package_files.clone() {
            let package = self
                .paths
                .packages
                .iter()
                .find(|package| package.id == id)
                .expect("declared package");
            let Some(PackageMetadata::R(metadata)) = &self.workspace.packages[&id].metadata else {
                continue;
            };
            let news = files.news.as_ref().filter(|path| {
                path.extension().is_some_and(|extension| extension == "Rd")
                    && self
                        .workspace
                        .pages
                        .contains_key(&page_id(&id, PackagePageRole::News))
            });
            let result = native::complete(
                &id,
                package,
                files.citation.as_deref(),
                news.map(PathBuf::as_path),
                &metadata.description.source,
                deadlines,
                cancellation,
            )
            .await?;
            let mut blocks = vec![];
            if !result.runtime.authors.is_empty() {
                blocks.push(heading("Authors"));
                blocks.extend(
                    result
                        .runtime
                        .authors
                        .iter()
                        .map(|author| paragraph(author)),
                );
            }
            if let Some(citation) = &result.runtime.citation {
                blocks.extend([
                    heading("Citation"),
                    paragraph(&citation.text),
                    heading("BibTeX"),
                    code(&citation.bibtex, Some("bibtex")),
                ]);
            }
            if let Some(field) = metadata.description.fields.get("Maintainer") {
                blocks.extend([heading("Maintainer"), paragraph(&field.value)]);
            } else if !result.runtime.maintainers.is_empty() {
                blocks.push(heading("Maintainer"));
                blocks.extend(
                    result
                        .runtime
                        .maintainers
                        .iter()
                        .map(|maintainer| paragraph(maintainer)),
                );
            }
            let mut inputs = vec![InputFingerprint {
                source: metadata.description.source.clone(),
                fingerprint: self.evidence.inputs[&metadata.description.source.repository]
                    [&metadata.description.source.path]
                    .clone(),
            }];
            for path in files.citation.iter().chain(news) {
                let repository = &self.paths.repositories[package.repository_index];
                let source = repository.source_location(path, None)?;
                inputs.push(InputFingerprint {
                    fingerprint: self.evidence.inputs[&source.repository][&source.path].clone(),
                    source,
                });
            }
            let provenance = Provenance {
                activity: ProvenanceActivity::PackageMetadata {
                    package: id.clone(),
                    r_version: result.runtime.r_version.clone(),
                    policy: "r-package-metadata-v1".into(),
                    inputs,
                },
                source: Some(source(&metadata.description.source)),
                span: None,
                tools: BTreeMap::from([
                    ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
                    ("R".into(), result.runtime.r_version.clone()),
                ]),
            };
            let text = result
                .runtime
                .authors
                .iter()
                .cloned()
                .chain(
                    result
                        .runtime
                        .citation
                        .iter()
                        .map(|citation| format!("{}\n{}", citation.text, citation.bibtex)),
                )
                .collect::<Vec<_>>()
                .join("\n");
            if !blocks.is_empty() {
                let mut document = generated(blocks, text);
                if let Some(path) = &files.citation {
                    document.source_location = Some(
                        self.paths.repositories[package.repository_index]
                            .source_location(path, None)?,
                    );
                }
                document.provenance.push(provenance.clone());
                insert(
                    &mut self.workspace,
                    &self.prepared_pages,
                    &id,
                    PackagePageRole::Authors,
                    document,
                );
            }
            if let Some(text) = result.news
                && let Some(page) = self
                    .workspace
                    .pages
                    .get_mut(&page_id(&id, PackagePageRole::News))
            {
                page.document.document.blocks = vec![code(&text, None)];
                page.document.raw_source = Some(text.clone());
                page.document.document.span.end = text.len();
                page.document.provenance.push(provenance.clone());
            }
            if let Some(PackageMetadata::R(metadata)) = &mut self
                .workspace
                .packages
                .get_mut(&id)
                .expect("declared package")
                .metadata
            {
                metadata.runtime = Some(result.runtime);
            }
            self.workspace.provenance.push(provenance);
        }
        self.revalidate()?;
        Ok(())
    }
}
