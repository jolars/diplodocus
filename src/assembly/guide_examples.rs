//! Run authorized Markdown fences in the package environments used for extraction.

use super::*;
use crate::documents::guide_examples::{self, Example};
use crate::ir::*;
use crate::validation::ContentAsset;
use serde::Deserialize;
use std::io::Read;
use std::process::Stdio;

type Plans = BTreeMap<String, Vec<Example>>;

fn failure(page: &PreparedPage, span: SourceSpan, message: impl Into<String>) -> AssemblyError {
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::ExecutionCellFailed,
        Severity::Error,
        message,
    )
    .with_source(source(&page.location));
    diagnostic.span = Some(span);
    includes::remap(&mut diagnostic, &page.source_ranges);
    AssemblyError::Diagnostics(vec![diagnostic])
}

impl WorkspaceSources {
    pub(super) fn guide_plans(&self) -> Result<Plans, AssemblyError> {
        let mut plans = BTreeMap::new();
        for (id, page) in &self.prepared_pages {
            let collection = &self.workspace.content_collections
                [&self.configuration.content[page.collection].id];
            let examples = guide_examples::plan(&page.source, collection, &self.workspace, |id| {
                self.paths.packages.iter().any(|package| package.id == id)
            })
            .map_err(|(span, message)| failure(page, span, message))?;
            if !examples.is_empty() {
                let mut blocks = self.workspace.pages[id].document.document.blocks.clone();
                let shapes = examples
                    .iter()
                    .map(|example| GuideExampleOutcome {
                        span: example.span,
                        package: example.package.clone(),
                        runtime_version: String::new(),
                        outputs: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                if !guide_examples::project(&mut blocks, &shapes) {
                    return Err(failure(
                        page,
                        examples[0].span,
                        "Runnable fence is outside supported page content.",
                    ));
                }
                plans.insert(id.clone(), examples);
            }
        }
        Ok(plans)
    }

    pub(crate) async fn execute_guide_examples(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        for (id, examples) in self.guide_plans()? {
            self.revalidate()?;
            let page = &self.prepared_pages[&id];
            let mut groups: Vec<Vec<(usize, &Example)>> = Vec::new();
            for (ordinal, example) in examples.iter().enumerate() {
                if let Some(group) = groups
                    .iter_mut()
                    .find(|group| group[0].1.package == example.package)
                {
                    group.push((ordinal, example));
                } else {
                    groups.push(vec![(ordinal, example)]);
                }
            }
            let mut results = BTreeMap::new();
            let mut assets = BTreeMap::new();
            for group in groups {
                let example = group[0].1;
                let package = self
                    .paths
                    .packages
                    .iter()
                    .find(|p| p.id == example.package)
                    .expect("validated package");
                let metadata = &self.workspace.packages[&package.id].metadata;
                let staging = tempfile::tempdir()?;
                let blocks = group
                    .iter()
                    .map(|(_, block)| block.code.as_str())
                    .collect::<Vec<_>>();
                let mut request = serde_json::json!({"root": package.path, "blocks": blocks});
                let helper = staging.path().join(if example.language == "python" {
                    "guide.py"
                } else {
                    "guide.R"
                });
                let mut command = if example.language == "python" {
                    let interpreter = self.python_interpreter.as_ref().ok_or_else(|| {
                        failure(
                            page,
                            example.span,
                            "No native Python interpreter was selected during extraction.",
                        )
                    })?;
                    let modules: std::collections::BTreeSet<_> = package
                        .targets
                        .iter()
                        .filter(|target| {
                            self.workspace.packages[&package.id].extraction_targets[&target.id]
                                .extractor
                                == "python"
                        })
                        .filter_map(|target| {
                            let entry = if target.path.is_dir() {
                                target.path.join("__init__.py")
                            } else {
                                target.path.clone()
                            };
                            crate::extractors::python::module_name(
                                &entry,
                                &target.path,
                                &package.path,
                                "3.14",
                            )
                            .map(|(module, _)| module)
                        })
                        .collect();
                    request["modules"] = serde_json::json!(modules);
                    request["import_roots"] =
                        serde_json::json!(crate::extractors::python::import_roots(package));
                    fs::write(&helper, include_str!("guide_examples.py"))?;
                    fs::write(
                        staging.path().join("python_environment.py"),
                        include_str!("python_environment.py"),
                    )?;
                    let mut command = super::python_native::command(interpreter, staging.path());
                    command.arg(&helper);
                    command
                } else {
                    let Some(PackageMetadata::R(metadata)) = metadata else {
                        return Err(failure(
                            page,
                            example.span,
                            "Native R execution requires extracted package metadata.",
                        ));
                    };
                    request["name"] = serde_json::json!(metadata.description.name);
                    request["version"] = serde_json::json!(metadata.description.version);
                    fs::write(
                        &helper,
                        concat!(
                            include_str!("r_runtime.R"),
                            "\n",
                            include_str!("r_environment.R"),
                            "\n",
                            include_str!("guide_examples.R")
                        ),
                    )?;
                    let mut command = tokio::process::Command::new("R");
                    command
                        .args(["--vanilla", "--slave", "-f"])
                        .arg(&helper)
                        .arg("--args");
                    command
                };
                fs::write(staging.path().join("request.json"), request.to_string())?;
                let errors = staging.path().join("errors");
                command
                    .arg(staging.path())
                    .current_dir(page.path.parent().expect("page parent"))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(fs::File::create(&errors)?)
                    .kill_on_drop(true);
                #[cfg(target_os = "linux")]
                command.process_group(0);
                let child = command
                    .spawn()
                    .map_err(|error| failure(page, example.span, error.to_string()))?;
                let mut process = super::rd_examples::Process {
                    #[cfg(target_os = "linux")]
                    group: child
                        .id()
                        .and_then(|pid| rustix::process::Pid::from_raw(pid as i32)),
                    child,
                };
                let failed = |message: String| {
                    let index = fs::read_to_string(staging.path().join("progress"))
                        .ok()
                        .and_then(|value| value.parse::<usize>().ok());
                    let block = index
                        .and_then(|index| group.get(index))
                        .map_or(example, |(_, block)| *block);
                    failure(page, block.span, message)
                };
                let status = tokio::select! {
                    status = process.child.wait() => status?,
                    message = super::documenter_examples::progress(staging.path(), group.len(), deadlines) => {
                        super::rd_examples::terminate(&mut process, deadlines).await?;
                        return Err(failed(message.replace("Documenter", "guide")));
                    },
                    () = cancellation.as_mut() => {
                        super::rd_examples::terminate(&mut process, deadlines).await?;
                        return Err(AssemblyError::Cancelled);
                    }
                };
                drop(process);
                if !status.success() {
                    let message = read_bounded(&errors)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_default();
                    let message = message
                        .replace(staging.path().to_string_lossy().as_ref(), "<runner>")
                        .replace(package.path.to_string_lossy().as_ref(), "<package>");
                    return Err(failed(format!(
                        "{} guide execution failed ({status}). {}",
                        example.language,
                        message.trim()
                    )));
                }
                let output: Results = serde_json::from_slice(
                    &read_bounded(&staging.path().join("results.json"))
                        .map_err(|error| failed(error.to_string()))?,
                )
                .map_err(|error| failed(format!("Invalid guide output: {error}")))?;
                if output.blocks.len() != group.len() || output.version.trim().is_empty() {
                    return Err(failed("Incomplete guide execution results.".into()));
                }
                for (index, ((ordinal, block), outputs)) in
                    group.into_iter().zip(output.blocks).enumerate()
                {
                    let outputs = decode_outputs(outputs, index, staging.path(), &mut assets)
                        .map_err(|error| failure(page, block.span, error.to_string()))?;
                    results.insert(
                        ordinal,
                        GuideExampleOutcome {
                            span: block.span,
                            package: block.package.clone(),
                            runtime_version: output.version.clone(),
                            outputs,
                        },
                    );
                }
                staging.close()?;
            }
            self.revalidate()?;
            let results = results.into_values().collect::<Vec<_>>();
            let document = &mut self
                .workspace
                .pages
                .get_mut(&id)
                .expect("authored page")
                .document;
            if !guide_examples::project(&mut document.document.blocks, &results) {
                return Err(failure(
                    page,
                    examples[0].span,
                    "Could not associate guide outputs with their source fences.",
                ));
            }
            document.provenance.push(Provenance {
                activity: ProvenanceActivity::GuideExamples {
                    input: InputFingerprint {
                        source: page.location.clone(),
                        fingerprint: fingerprint_bytes(page.source.as_bytes()),
                    },
                    examples: results,
                },
                source: Some(source(&page.location)),
                span: None,
                tools: crate::provenance::builtin_tools(),
            });
            self.native_assets.extend(assets);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Results {
    version: String,
    blocks: Vec<Vec<Output>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum Output {
    Text { text: String },
    TextFile { file: String },
    Image { file: String },
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let limit = super::documenter_examples::MAX_OUTPUT;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "Guide output exceeds the output limit.",
        ));
    }
    Ok(bytes)
}

fn decode_outputs(
    outputs: Vec<Output>,
    index: usize,
    staging: &Path,
    assets: &mut BTreeMap<DiagnosticPath, ContentAsset>,
) -> std::io::Result<Vec<DocumenterOutput>> {
    let mut decoded = Vec::new();
    for (position, output) in outputs.into_iter().enumerate() {
        match output {
            Output::Text { text } => decoded.push(DocumenterOutput::Text { text }),
            Output::TextFile { file } if file == format!("text-{index}") => {
                let path = crate::paths::resolve_input_file(
                    staging,
                    "guide output",
                    Path::new(&file),
                    staging,
                )
                .map_err(std::io::Error::other)?;
                let text =
                    String::from_utf8(read_bounded(&path)?).map_err(std::io::Error::other)?;
                if !text.is_empty() {
                    decoded.push(DocumenterOutput::Text { text });
                }
            }
            Output::Image { file } if file == format!("figure-{index}-{position}.png") => {
                let path = crate::paths::resolve_input_file(
                    staging,
                    "guide figure",
                    Path::new(&file),
                    staging,
                )
                .map_err(std::io::Error::other)?;
                let bytes = read_bounded(&path)?;
                let media_type = "image/png";
                crate::execution::assets::validate_image_bytes(media_type, &bytes)
                    .map_err(std::io::Error::other)?;
                let fingerprint = fingerprint_bytes(&bytes);
                let path = DiagnosticPath::try_from(format!(
                    "guide-assets/sha256/{}.png",
                    fingerprint.value
                ))
                .map_err(std::io::Error::other)?;
                assets.insert(
                    path.clone(),
                    ContentAsset {
                        fingerprint: fingerprint.clone(),
                        media_type: media_type.into(),
                        bytes,
                    },
                );
                decoded.push(DocumenterOutput::Image {
                    media_type: media_type.into(),
                    asset: AssetReference { path, fingerprint },
                });
            }
            _ => return Err(std::io::Error::other("Unexpected guide output path.")),
        }
    }
    Ok(decoded)
}
