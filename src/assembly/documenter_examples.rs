//! Native Documenter execution uses supervised processes and retained inert output.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::documents::{AuthoredFormat, documenter};
use crate::ir::*;
use crate::validation::ContentAsset;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;

const MAX_OUTPUT: u64 = 32 * 1024 * 1024;

#[derive(Serialize)]
struct Request<'a> {
    project: &'a Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<Package<'a>>,
    examples: Vec<ExampleRequest<'a>>,
}

#[derive(Serialize)]
struct Package<'a> {
    root: &'a Path,
    metadata: &'a Path,
    name: &'a str,
    version: &'a str,
}

#[derive(Serialize)]
struct ExampleRequest<'a> {
    ordinal: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<&'a str>,
    source: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Results {
    policy: String,
    julia_version: String,
    dependencies: BTreeMap<String, String>,
    examples: Vec<ExampleResult>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExampleResult {
    ordinal: usize,
    outputs: Vec<NativeOutput>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum NativeOutput {
    Text { text: String },
    Image { media_type: String, file: String },
}

fn failure(page: &PreparedPage, collection: &str, message: impl Into<String>) -> AssemblyError {
    AssemblyError::Diagnostics(vec![
        Diagnostic::new(
            DiagnosticCode::ExecutionCellFailed,
            Severity::Error,
            message,
        )
        .with_source(source(&page.location))
        .with_entity(DiagnosticEntity::Document {
            collection: collection.into(),
            path: page.relative.clone(),
        }),
    ])
}

fn read_bounded(path: &Path) -> Result<String, AssemblyError> {
    if fs::metadata(path)?.len() > MAX_OUTPUT {
        return Err(AssemblyError::Diagnostics(vec![diagnostic(
            DiagnosticCode::InvalidCellOutput,
            "Native Documenter output exceeds 32 MiB.",
        )]));
    }
    Ok(fs::read_to_string(path)?)
}

impl WorkspaceSources {
    /// Execute authorized Documenter pages in fresh native Julia processes.
    ///
    /// Named examples share page-local modules; anonymous examples are isolated.
    /// Results and environment evidence become portable only after validation.
    ///
    /// # Errors
    /// Missing prepared dependencies, native errors, invalid outputs, timeouts,
    /// cancellation, changed inputs, or cleanup failures stop publication.
    pub async fn execute_documenter_examples(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        let mut order: Vec<_> = self
            .prepared_pages
            .iter()
            .filter(|(_, page)| {
                let collection = &self.configuration.content[page.collection];
                collection.format == AuthoredFormat::Documenter
                    && collection.execution.mode == crate::configuration::ExecutionMode::Execute
            })
            .map(|(id, page)| (page.collection, page.relative.clone(), id.clone()))
            .collect();
        order.sort();
        for (_, _, id) in order {
            self.revalidate()?;
            let page = &self.prepared_pages[&id];
            let collection = &self.configuration.content[page.collection];
            let examples =
                documenter::examples(&page.source).map_err(AssemblyError::Diagnostics)?;
            if examples.is_empty() {
                continue;
            }
            if self.workspace.pages[&id]
                .document
                .provenance
                .iter()
                .any(|record| {
                    matches!(
                        record.activity,
                        ProvenanceActivity::DocumenterExamples { .. }
                    )
                })
            {
                return Err(AssemblyError::InputsChanged);
            }
            let project = self.paths.content[page.collection]
                .julia_project
                .as_ref()
                .expect("validated Julia project");
            let package = if collection.owner == "project" {
                None
            } else {
                let package = self
                    .paths
                    .packages
                    .iter()
                    .find(|package| package.id == collection.owner)
                    .expect("collection owner");
                let Some(PackageMetadata::Julia(metadata)) =
                    &self.workspace.packages[&package.id].metadata
                else {
                    return Err(failure(
                        page,
                        &collection.id,
                        "Native Documenter execution requires a Julia package owner or project ownership.",
                    ));
                };
                Some(Package {
                    root: &package.path,
                    metadata: &package.metadata_path,
                    name: &metadata.name,
                    version: metadata.version.as_deref().unwrap_or_default(),
                })
            };
            let staging = tempfile::tempdir()?;
            let request = staging.path().join("request.toml");
            let output = staging.path().join("results.toml");
            let errors = staging.path().join("errors");
            let helper = staging.path().join("documenter.jl");
            fs::write(&helper, include_str!("documenter_examples.jl"))?;
            fs::write(
                &request,
                toml::to_string(&Request {
                    project,
                    package,
                    examples: examples
                        .iter()
                        .enumerate()
                        .map(|(ordinal, example)| ExampleRequest {
                            ordinal,
                            session: example.session.as_deref(),
                            source: &example.source,
                        })
                        .collect(),
                })
                .map_err(|_| AssemblyError::InputsChanged)?,
            )?;
            let mut command = tokio::process::Command::new("julia");
            command
                .args(["--startup-file=no", "--history-file=no", "--color=no"])
                .arg(format!("--project={}", project.display()))
                .arg(&helper)
                .arg(&request)
                .arg(&output)
                .current_dir(page.path.parent().expect("page parent"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(fs::File::create(&errors)?)
                .kill_on_drop(true);
            #[cfg(target_os = "linux")]
            command.process_group(0);
            let child = command.spawn().map_err(|_| {
                failure(
                    page,
                    &collection.id,
                    "Documenter examples require Julia and the selected prepared environment.",
                )
            })?;
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
                status = process.child.wait() => status?,
                message = progress(staging.path(), examples.len(), deadlines) => {
                    super::rd_examples::terminate(&mut process, deadlines).await?;
                    return Err(failure(page, &collection.id, message));
                },
                () = cancellation.as_mut() => { super::rd_examples::terminate(&mut process, deadlines).await?; return Err(AssemblyError::Cancelled); }
            };
            if !status.success() {
                let mut message =
                    read_bounded(&errors).unwrap_or_else(|_| "Native Julia helper failed.".into());
                for path in [
                    &self.paths.repositories[self.paths.content[page.collection].repository_index]
                        .path,
                    staging.path(),
                ] {
                    message = message.replace(path.to_string_lossy().as_ref(), "<source>");
                }
                let mut error = failure(
                    page,
                    &collection.id,
                    format!("Documenter execution failed: {}", message.trim()),
                );
                if let AssemblyError::Diagnostics(diagnostics) = &mut error {
                    diagnostics[0].span = fs::read_to_string(staging.path().join("progress"))
                        .ok()
                        .and_then(|value| value.parse::<usize>().ok())
                        .and_then(|ordinal| examples.get(ordinal))
                        .map(|example| example.span);
                }
                return Err(error);
            }
            let results: Results = toml::from_str(&read_bounded(&output)?)
                .map_err(|_| failure(page, &collection.id, "Invalid native Documenter results."))?;
            let julia_version = results.julia_version.clone();
            let dependencies = results.dependencies.clone();
            let (outcomes, assets) = decode(results, &examples, staging.path())
                .map_err(|message| failure(page, &collection.id, message))?;
            self.revalidate()?;
            let environment_inputs =
                self.evidence.declared_environment_inputs[&collection.id].clone();
            let project_location = self.paths.repositories
                [self.paths.content[page.collection].repository_index]
                .source_location(project, None)?;
            let project = environment_inputs
                .iter()
                .find(|input| input.source == project_location)
                .expect("observed Julia project")
                .clone();
            let document = &mut self
                .workspace
                .pages
                .get_mut(&id)
                .expect("authored page")
                .document;
            let mut replaced = std::collections::BTreeSet::new();
            documenter::rewrite(&mut document.document.blocks, &mut |block| {
                if let Block::CodeBlock { source, span, .. } = block
                    && let Some((ordinal, example)) = examples
                        .iter()
                        .enumerate()
                        .find(|(_, example)| example.span == *span && example.source == *source)
                    && replaced.insert(ordinal)
                {
                    *block = documenter::rendered(example, &outcomes[ordinal]);
                }
            });
            if replaced.len() != examples.len() {
                return Err(AssemblyError::InputsChanged);
            }
            document.provenance.push(Provenance {
                activity: ProvenanceActivity::DocumenterExamples {
                    collection: collection.id.clone(),
                    policy: documenter::POLICY.into(),
                    julia_version: julia_version.clone(),
                    dependencies,
                    project,
                    environment_inputs,
                    input: InputFingerprint {
                        source: page.location.clone(),
                        fingerprint: fingerprint_bytes(page.source.as_bytes()),
                    },
                    examples: outcomes,
                },
                source: Some(source(&page.location)),
                span: None,
                tools: {
                    let mut tools = crate::provenance::builtin_tools();
                    tools.insert("julia".into(), julia_version);
                    tools
                },
            });
            self.native_assets.extend(assets);
            staging.close()?;
        }
        self.revalidate()
    }
}

type Decoded = (
    Vec<DocumenterExampleOutcome>,
    BTreeMap<DiagnosticPath, ContentAsset>,
);

fn decode(
    results: Results,
    examples: &[documenter::Example],
    staging: &Path,
) -> Result<Decoded, &'static str> {
    if results.policy != documenter::POLICY
        || results.julia_version.is_empty()
        || results.dependencies.is_empty()
        || results.examples.len() != examples.len()
    {
        return Err("Incomplete native Documenter results.");
    }
    let mut outcomes = Vec::new();
    let mut assets = BTreeMap::new();
    for (ordinal, (result, example)) in results.examples.into_iter().zip(examples).enumerate() {
        if result.ordinal != ordinal {
            return Err("Native Documenter examples are out of order.");
        }
        let mut outputs = Vec::new();
        let mut images = 0;
        for output in result.outputs {
            outputs.push(match output {
                NativeOutput::Text { text } => DocumenterOutput::Text { text },
                NativeOutput::Image { media_type, file } => {
                    images += 1;
                    let extension = match media_type.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpg",
                        "image/svg+xml" => "svg",
                        _ => return Err("Unsupported native Documenter image format."),
                    };
                    if file != format!("image-{images}.{extension}") {
                        return Err("Invalid native Documenter image path.");
                    }
                    let directory = staging.join(format!("block-{ordinal}"));
                    let path = crate::paths::resolve_input_file(
                        staging,
                        "documenter.image",
                        Path::new(&file),
                        &directory,
                    )
                    .map_err(|_| "Native Documenter image escaped staging.")?;
                    if fs::metadata(&path)
                        .map_err(|_| "Missing native Documenter image.")?
                        .len()
                        > MAX_OUTPUT
                    {
                        return Err("Native Documenter image exceeds 32 MiB.");
                    }
                    let bytes = fs::read(path).map_err(|_| "Missing native Documenter image.")?;
                    crate::execution::assets::validate_image_bytes(&media_type, &bytes)
                        .map_err(|_| "Invalid native Documenter image.")?;
                    let fingerprint = fingerprint_bytes(&bytes);
                    let path = DiagnosticPath::try_from(format!(
                        "documenter-assets/sha256/{}.{extension}",
                        fingerprint.value
                    ))
                    .map_err(|_| "Invalid native Documenter asset path.")?;
                    assets.insert(
                        path.clone(),
                        ContentAsset {
                            fingerprint: fingerprint.clone(),
                            media_type: media_type.clone(),
                            bytes,
                        },
                    );
                    DocumenterOutput::Image {
                        media_type,
                        asset: AssetReference { path, fingerprint },
                    }
                }
            });
        }
        outcomes.push(DocumenterExampleOutcome {
            span: example.span,
            session: example.session.clone(),
            outputs,
        });
    }
    Ok((outcomes, assets))
}

async fn progress(staging: &Path, count: usize, deadlines: ExecutionDeadlines) -> &'static str {
    let mut current = None;
    let mut deadline =
        tokio::time::Instant::now() + Duration::from_millis(deadlines.startup.max(1));
    loop {
        match fs::read_to_string(staging.join("progress")) {
            Ok(value) => {
                let Ok(ordinal) = value.parse::<usize>() else {
                    return "Invalid native Documenter progress.";
                };
                if current != Some(ordinal) {
                    if ordinal >= count || current.is_some_and(|previous| ordinal <= previous) {
                        return "Invalid native Documenter progress order.";
                    }
                    current = Some(ordinal);
                    deadline =
                        tokio::time::Instant::now() + Duration::from_millis(deadlines.cell.max(1));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return "Could not read native Documenter progress.",
        }
        if tokio::time::Instant::now() >= deadline {
            return "Documenter example execution timed out.";
        }
        tokio::time::sleep_until(
            deadline.min(tokio::time::Instant::now() + Duration::from_millis(20)),
        )
        .await;
    }
}
