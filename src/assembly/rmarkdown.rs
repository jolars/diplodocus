use super::*;
use crate::configuration::{
    ContentConfiguration, ExecutionConfiguration, ExecutionEngine as Engine, ExecutionMode,
};
use crate::documents::{AuthoredFormat, parse_evaluated_rmd, prepare_rmd_document};
use crate::ir::*;
use crate::validation::ContentAsset;
use std::process::Stdio;
use std::time::Duration;

pub(super) mod citations;
const POLICY: &str = "rmd-knitr-v1";
const MAX_OUTPUT: u64 = 32 * 1024 * 1024;

pub(super) fn collections(
    path: &Path,
    config: &mut WorkspaceConfiguration,
) -> Result<(), AssemblyError> {
    let paths = resolve_workspace_paths(path, config)?;
    for (package, resolved) in config.packages.iter().zip(&paths.packages) {
        if package.ecosystem != "r" {
            continue;
        }
        let root = resolved.path.join("vignettes");
        match fs::symlink_metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let id = format!("rmd-vignettes:{}", package.id);
        if config.content.iter().any(|c| c.id == id) {
            return Err(AssemblyError::Diagnostics(vec![diagnostic(
                DiagnosticCode::InvalidExecutionConfiguration,
                "Collection identifiers beginning with rmd-vignettes: are reserved for discovered vignettes.",
            )]));
        }
        let repository = &paths.repositories[resolved.repository_index];
        config.content.push(ContentConfiguration {
            id,
            owner: package.id.clone(),
            repository: package.repository.clone(),
            path: root
                .strip_prefix(&repository.path)
                .map_err(|_| AssemblyError::InputsChanged)?
                .to_owned(),
            mount: "vignettes".into(),
            format: AuthoredFormat::Rmd,
            navigation: Default::default(),
            execution: ExecutionConfiguration {
                mode: ExecutionMode::Execute,
                engine: Some(Engine::Knitr),
                ..ExecutionConfiguration::default()
            },
        });
    }
    for collection in &config.content {
        if collection.format == AuthoredFormat::Rmd
            && !config
                .packages
                .iter()
                .any(|p| p.id == collection.owner && p.ecosystem == "r")
        {
            return Err(AssemblyError::Diagnostics(vec![diagnostic(
                DiagnosticCode::InvalidExecutionConfiguration,
                "R Markdown collections must belong to a declared R package.",
            )]));
        }
    }
    Ok(())
}

pub(super) fn inputs(
    paths: &ResolvedWorkspacePaths,
    config: &WorkspaceConfiguration,
) -> Result<BTreeMap<String, Vec<PathBuf>>, AssemblyError> {
    fn walk(
        root: &Path,
        repository: &crate::paths::ResolvedRepositoryPaths,
        output: &mut Vec<PathBuf>,
    ) -> Result<(), AssemblyError> {
        let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if entry.file_name() == ".git"
                || crate::generated_storage::is_reserved(&path)
                || crate::generated_storage::is_generated_file(&path)
            {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() && fs::metadata(&path)?.is_dir() {
                return Err(AssemblyError::InputsChanged);
            }
            if kind.is_dir() {
                walk(&path, repository, output)?;
            } else {
                let source = repository.source_location(&path, None)?;
                output.push(PathBuf::from(source.path.as_str()));
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    for (collection, resolved) in config.content.iter().zip(&paths.content) {
        if collection.format == AuthoredFormat::Rmd {
            let mut files = Vec::new();
            walk(
                &resolved.path,
                &paths.repositories[resolved.repository_index],
                &mut files,
            )?;
            result.insert(collection.id.clone(), files);
        }
    }
    Ok(result)
}

fn failure(source: &SourceLocation, message: impl Into<String>) -> AssemblyError {
    AssemblyError::Diagnostics(vec![
        Diagnostic::new(
            DiagnosticCode::ExecutionCellFailed,
            Severity::Error,
            message,
        )
        .with_source(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        }),
    ])
}

impl WorkspaceSources {
    /// Evaluate every R Markdown article in a fresh native session.
    ///
    /// # Errors
    /// Missing prerequisites, native failures, invalid outputs, changed inputs,
    /// timeout, cancellation, or failed cleanup stop the extraction attempt.
    pub async fn execute_rmarkdown(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        let mut order: Vec<_> = self
            .prepared_pages
            .iter()
            .filter(|(_, p)| self.configuration.content[p.collection].format == AuthoredFormat::Rmd)
            .map(|(id, p)| (p.collection, p.relative.clone(), id.clone()))
            .collect();
        order.sort();
        for (_, _, id) in order {
            self.revalidate()?;
            let page = &self.prepared_pages[&id];
            if self.workspace.pages[&id]
                .document
                .generated_source
                .is_some()
            {
                return Err(AssemblyError::InputsChanged);
            }
            let collection = &self.configuration.content[page.collection];
            let package = self
                .paths
                .packages
                .iter()
                .find(|p| p.id == collection.owner)
                .expect("R Markdown owner");
            let Some(PackageMetadata::R(metadata)) = &self.workspace.packages[&package.id].metadata
            else {
                return Err(failure(
                    &page.location,
                    "R Markdown requires package metadata from an R extraction target.",
                ));
            };
            let staging = tempfile::tempdir()?;
            let output = staging.path().join("results");
            let errors = staging.path().join("errors");
            let helper = format!(
                "{}\n{}",
                include_str!("r_runtime.R"),
                include_str!("rmarkdown.R")
            );
            let mut command = tokio::process::Command::new("R");
            command
                .args(["--vanilla", "--slave", "-e", &helper, "--args"])
                .arg(&package.path)
                .arg(&page.path)
                .arg(&output)
                .arg(&metadata.description.name)
                .arg(&metadata.description.version)
                .current_dir(page.path.parent().expect("article parent"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(fs::File::create(&errors)?)
                .kill_on_drop(true);
            #[cfg(target_os = "linux")]
            command.process_group(0);
            let child = command.spawn().map_err(|_| failure(&page.location, "R Markdown requires R, pkgload, evaluate, and knitr in the prepared environment."))?;
            #[cfg(target_os = "linux")]
            let group = child
                .id()
                .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
            let mut process = super::rd_examples::Process {
                child,
                #[cfg(target_os = "linux")]
                group,
            };
            let status = tokio::select! {
                status = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => match status {
                    Ok(status) => status?,
                    Err(_) => { super::rd_examples::terminate(&mut process, deadlines).await?; return Err(failure(&page.location, "R Markdown execution timed out.")); }
                },
                () = cancellation.as_mut() => { super::rd_examples::terminate(&mut process, deadlines).await?; return Err(AssemblyError::Cancelled); }
            };
            if !status.success() {
                let mut message =
                    read_bounded(&errors).unwrap_or_else(|_| "Native R helper failed.".into());
                for path in [&package.path, &page.path, staging.path()] {
                    message = message.replace(&path.to_string_lossy().to_string(), "<source>");
                }
                return Err(failure(
                    &page.location,
                    format!("R Markdown execution failed: {}", message.trim()),
                ));
            }
            let wire = read_bounded(&output)?;
            let markdown = read_bounded(&staging.path().join("article.md"))?;
            let (markdown, provenance, assets, figures) = decode(
                &wire,
                &markdown,
                &page.source,
                &page.location,
                &package.id,
                staging.path(),
            )?;
            let mut parsed = parse_evaluated_rmd(&markdown).with_context(
                source(&page.location),
                crate::diagnostics::DiagnosticEntity::Document {
                    collection: collection.id.clone(),
                    path: page.relative.clone(),
                },
            );
            // Generated executable syntax and active markup must not cross the document boundary.
            for diagnostic in &mut parsed.diagnostics {
                diagnostic.severity = Severity::Error;
            }
            reject_errors(parsed.diagnostics)?;
            let mut diagnostics = Vec::new();
            decorate_figures(&mut parsed.document.blocks, &figures, &mut diagnostics);
            for diagnostic in &mut diagnostics {
                diagnostic.severity = Severity::Error;
                diagnostic.span = None;
                diagnostic.source = Some(source(&page.location));
                diagnostic.message = format!("Generated figure caption: {}", diagnostic.message);
            }
            reject_errors(diagnostics)?;
            citations::resolve(
                &mut parsed.document,
                &page.path,
                &self.paths.repositories[package.repository_index],
            )?;
            self.revalidate()?;
            let document = &mut self.workspace.pages.get_mut(&id).expect("article").document;
            let page_title =
                citations::metadata(&parsed.document, "title").and_then(|value| match value {
                    MetadataValue::Scalar { value, .. } => Some(value.clone()),
                    _ => None,
                });
            document.document = parsed.document;
            document.generated_source = Some(markdown);
            let mut tools = crate::provenance::builtin_tools();
            tools.insert("hayagriva".into(), "0.10.1".into());
            document.provenance.push(Provenance {
                activity: provenance,
                source: Some(source(&page.location)),
                span: None,
                tools,
            });
            if let Some(title) = page_title {
                self.workspace.pages.get_mut(&id).expect("article").title = title;
            }
            self.rd_assets.extend(assets);
            staging.close()?;
        }
        self.revalidate()
    }
}

fn read_bounded(path: &Path) -> Result<String, AssemblyError> {
    if fs::metadata(path)?.len() > MAX_OUTPUT {
        return Err(AssemblyError::Diagnostics(vec![diagnostic(
            DiagnosticCode::UnsupportedCellOutput,
            "Native R Markdown output exceeds 32 MiB.",
        )]));
    }
    Ok(fs::read_to_string(path)?)
}

type Figures = BTreeMap<String, (String, String)>;
type NativeResult = (
    String,
    ProvenanceActivity,
    BTreeMap<DiagnosticPath, ContentAsset>,
    Figures,
);

fn decode(
    wire: &str,
    markdown: &str,
    original: &str,
    location: &SourceLocation,
    package: &str,
    staging: &Path,
) -> Result<NativeResult, AssemblyError> {
    let invalid = || {
        failure(
            location,
            "R Markdown helper returned invalid or incomplete output.",
        )
    };
    let (prepared, diagnostics) = prepare_rmd_document(original);
    reject_errors(diagnostics)?;
    let mut chunks: Vec<RMarkdownChunkOutcome> = Vec::new();
    let mut inline = Vec::new();
    let mut versions = BTreeMap::new();
    let mut dependencies = BTreeMap::new();
    let mut assets = BTreeMap::new();
    let mut figures: Figures = BTreeMap::new();
    let mut current_figure = None;
    let mut figure_count = 0;
    let mut result = markdown.to_owned();
    for line in wire.lines() {
        let (key, hex) = line.split_once('\t').ok_or_else(invalid)?;
        if hex.len() % 2 != 0 {
            return Err(invalid());
        }
        let bytes = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| invalid())?, 16)
                    .map_err(|_| invalid())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let value = String::from_utf8(bytes).map_err(|_| invalid())?;
        match key {
            "chunk" => {
                let span = *prepared.chunks.get(chunks.len()).ok_or_else(invalid)?;
                if chunks.iter().any(|c| c.label == value) {
                    return Err(invalid());
                }
                chunks.push(RMarkdownChunkOutcome {
                    span,
                    label: value,
                    options: BTreeMap::new(),
                    outcome: String::new(),
                    reason: None,
                });
            }
            "option" => {
                let (key, value) = value.split_once('\t').ok_or_else(invalid)?;
                if chunks
                    .last_mut()
                    .ok_or_else(invalid)?
                    .options
                    .insert(key.into(), value.into())
                    .is_some()
                {
                    return Err(invalid());
                }
            }
            "outcome" => {
                let index = chunks.len().checked_sub(1).ok_or_else(invalid)?;
                let chunk = &mut chunks[index];
                if value == "skipped" {
                    let declared = prepared.eval_options[index].as_deref();
                    chunk.outcome =
                        if declared.is_some_and(|v| matches!(v.trim(), "FALSE" | "false" | "F")) {
                            "illustrative"
                        } else {
                            "conditional-skip"
                        }
                        .into();
                    chunk.reason = Some(format!(
                        "eval resolved to FALSE{}.",
                        declared.map(|v| format!(" from `{v}`")).unwrap_or_default()
                    ));
                } else if value == "allowed-error" {
                    chunk.outcome = value;
                    chunk.reason = Some("The chunk explicitly permits an R error.".into());
                } else if value == "executed" && chunk.outcome.is_empty() {
                    chunk.outcome = value;
                } else {
                    return Err(invalid());
                }
            }
            "inline" => {
                let span = *prepared.inline.get(inline.len()).ok_or_else(invalid)?;
                inline.push(RMarkdownInlineOutcome { span, value });
            }
            "dependency" => {
                let (name, version) = value.split_once('=').ok_or_else(invalid)?;
                if dependencies.insert(name.into(), version.into()).is_some() {
                    return Err(invalid());
                }
            }
            "figure" => {
                figure_count += 1;
                if value != format!("figure-{figure_count}.png") {
                    return Err(invalid());
                }
                let path = crate::paths::resolve_input_file(
                    staging,
                    "rmd.figure",
                    Path::new(&value),
                    staging,
                )
                .map_err(|_| invalid())?;
                let bytes = fs::read(path)?;
                crate::execution::assets::validate_image_bytes("image/png", &bytes)
                    .map_err(|_| invalid())?;
                let fingerprint = fingerprint_bytes(&bytes);
                let target = DiagnosticPath::try_from(format!(
                    "rmd-assets/sha256/{}-{figure_count}.png",
                    fingerprint.value
                ))
                .map_err(|_| invalid())?;
                result = result.replace(&format!("rmd-assets/{value}"), target.as_str());
                figures.insert(target.as_str().into(), (String::new(), String::new()));
                current_figure = Some(target.as_str().to_owned());
                assets.insert(
                    target,
                    ContentAsset {
                        fingerprint,
                        media_type: "image/png".into(),
                        bytes,
                    },
                );
            }
            "caption" | "alt" => {
                let figure = figures
                    .get_mut(current_figure.as_ref().ok_or_else(invalid)?)
                    .ok_or_else(invalid)?;
                if key == "caption" {
                    figure.0 = value;
                } else {
                    figure.1 = value;
                }
            }
            "r-version" | "knitr-version" | "package-version" | "complete" => {
                if versions.insert(key, value).is_some() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    if chunks.len() != prepared.chunks.len() || inline.len() != prepared.inline.len() {
        return Err(failure(
            location,
            format!(
                "R Markdown helper returned {} chunks and {} inline results; expected {} chunks and {} inline results.",
                chunks.len(),
                inline.len(),
                prepared.chunks.len(),
                prepared.inline.len()
            ),
        ));
    }
    if versions.remove("complete").as_deref() != Some(POLICY)
        || chunks.len() != prepared.chunks.len()
        || inline.len() != prepared.inline.len()
        || dependencies.is_empty()
        || chunks.iter().any(|c| c.outcome.is_empty())
    {
        return Err(invalid());
    }
    let mut take = |key| {
        versions
            .remove(key)
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)
    };
    let provenance = ProvenanceActivity::RMarkdown {
        package: package.into(),
        policy: POLICY.into(),
        r_version: take("r-version")?,
        knitr_version: take("knitr-version")?,
        package_version: take("package-version")?,
        dependencies,
        input: InputFingerprint {
            source: location.clone(),
            fingerprint: fingerprint_bytes(original.as_bytes()),
        },
        output: fingerprint_bytes(result.as_bytes()),
        chunks,
        inline,
    };
    Ok((result, provenance, assets, figures))
}

fn decorate_figures(blocks: &mut [Block], figures: &Figures, diagnostics: &mut Vec<Diagnostic>) {
    for block in blocks {
        match block {
            Block::Container {
                kind: DocumentContainerKind::Figure,
                blocks,
                span,
                ..
            } => {
                if let Some(Block::Paragraph { inlines, .. }) = blocks.first_mut()
                    && let Some(Inline::Image { target, alt, .. }) = inlines.first_mut()
                    && let Some((caption, text)) = figures.get(target)
                {
                    *alt = vec![Inline::Text {
                        value: text.clone(),
                        span: *span,
                    }];
                    let parsed = parse_evaluated_rmd(caption);
                    diagnostics.extend(parsed.diagnostics);
                    blocks.truncate(1);
                    blocks.extend(parsed.document.blocks);
                }
            }
            Block::Container { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::BlockQuote { blocks, .. }
            | Block::Callout { blocks, .. } => decorate_figures(blocks, figures, diagnostics),
            Block::List { items, .. } => {
                for item in items {
                    decorate_figures(&mut item.blocks, figures, diagnostics);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(records: &[(&str, &str)]) -> String {
        records
            .iter()
            .map(|(key, value)| {
                format!(
                    "{key}\t{}\n",
                    value
                        .bytes()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                )
            })
            .collect()
    }
    fn location() -> SourceLocation {
        SourceLocation {
            repository: "r".into(),
            path: DiagnosticPath::try_from("vignettes/article.Rmd").unwrap(),
            span: None,
        }
    }
    #[test]
    fn helper_completion_and_record_integrity_are_required() {
        let staging = tempfile::tempdir().unwrap();
        let records = [
            ("r-version", "4.6"),
            ("knitr-version", "1.51"),
            ("package-version", "0.1.0"),
            ("dependency", "knitr=1.51"),
            ("complete", POLICY),
        ];
        assert!(
            decode(
                &wire(&records),
                "Plain content.",
                "Plain content.",
                &location(),
                "demo",
                staging.path()
            )
            .is_ok()
        );
        for invalid in [
            wire(&records[..4]),
            format!("{}complete\t00\n", wire(&records)),
            "unknown\t61\n".into(),
            "inline\tzz\n".into(),
        ] {
            assert!(
                decode(
                    &invalid,
                    "Plain content.",
                    "Plain content.",
                    &location(),
                    "demo",
                    staging.path()
                )
                .is_err()
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn figures_cannot_escape_owned_staging() {
        let staging = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let image = outside.path().join("figure.png");
        image::RgbaImage::new(1, 1).save(&image).unwrap();
        std::os::unix::fs::symlink(image, staging.path().join("figure-1.png")).unwrap();
        assert!(
            decode(
                &wire(&[("figure", "figure-1.png")]),
                "",
                "",
                &location(),
                "demo",
                staging.path()
            )
            .is_err()
        );
    }
}
