//! Native Rd execution retains plain output and validated raster figures.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::execution::assets::validate_image_bytes;
use crate::ir::*;
use crate::validation::ContentAsset;
use std::process::Stdio;
use std::time::Duration;

const HELPER: &str = include_str!("rd_examples.R");

struct Process {
    child: tokio::process::Child,
    #[cfg(target_os = "linux")]
    group: Option<rustix::process::Pid>,
}

impl Process {
    fn kill(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(group) = self.group {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.child.start_kill();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

fn failure(package: &str, source: &SourceLocation, message: impl Into<String>) -> AssemblyError {
    AssemblyError::Diagnostics(vec![
        Diagnostic::new(
            DiagnosticCode::ExecutionCellFailed,
            Severity::Error,
            message,
        )
        .with_source(DiagnosticSource::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
        })
        .with_entity(DiagnosticEntity::Package { id: package.into() }),
    ])
}

impl WorkspaceSources {
    /// Execute R help-topic examples in fresh native sessions before publication.
    ///
    /// # Errors
    /// Missing prerequisites, example errors, invalid figures, timeouts, cancellation,
    /// changed source inputs, and cleanup failures stop the extraction attempt.
    pub async fn execute_rd_examples(
        &mut self,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(), AssemblyError> {
        if self
            .workspace
            .packages
            .values()
            .flat_map(|package| package.items.values())
            .filter_map(|item| item.documentation.as_ref())
            .any(|document| {
                document
                    .provenance
                    .iter()
                    .any(|record| matches!(record.activity, ProvenanceActivity::RdExamples { .. }))
            })
        {
            return Err(AssemblyError::InputsChanged);
        }
        self.rd_assets = execute(self, deadlines, cancellation).await?;
        Ok(())
    }
}

pub(super) async fn execute(
    sources: &mut WorkspaceSources,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<BTreeMap<DiagnosticPath, ContentAsset>, AssemblyError> {
    let mut assets = BTreeMap::new();
    for package in &sources.paths.packages {
        let record = &sources.workspace.packages[&package.id];
        let Some(PackageMetadata::R(metadata)) = &record.metadata else {
            continue;
        };
        let expected_name = metadata.description.name.clone();
        let expected_version = metadata.description.version.clone();
        let repository = &sources.paths.repositories[package.repository_index];
        let mut topics = BTreeMap::new();
        for item in record.items.values() {
            let Some(document) = &item.documentation else {
                continue;
            };
            if !matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "rd")
            {
                continue;
            }
            let Some(location) = &document.source_location else {
                continue;
            };
            if document.document.blocks.iter().any(is_example_heading) {
                topics
                    .entry(location.path.clone())
                    .or_insert_with(|| location.clone());
            }
        }
        for location in topics.into_values() {
            sources.revalidate()?;
            let document = sources.workspace.packages[&package.id]
                .items
                .values()
                .filter_map(|item| item.documentation.as_ref())
                .find(|document| document.source_location.as_ref() == Some(&location))
                .expect("selected Rd topic");
            if document.document.blocks.iter().any(|block| matches!(block, Block::Unsupported { source_kind, .. } if source_kind == "rd-examples")) {
                return Err(failure(&package.id, &location, "Unsupported executable Rd example syntax."));
            }
            let staging = tempfile::tempdir()?;
            let output = staging.path().join("results");
            let errors = staging.path().join("errors");
            let path = repository.path.join(location.path.as_str());
            let input_bytes = fs::read(&path)?;
            let mut command = tokio::process::Command::new("R");
            command
                .args(["--vanilla", "--slave", "-e", HELPER, "--args"])
                .arg(&package.path)
                .arg(&path)
                .arg(&output)
                .arg(&expected_name)
                .arg(&expected_version)
                .current_dir(&package.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(fs::File::create(&errors)?)
                .kill_on_drop(true);
            #[cfg(target_os = "linux")]
            command.process_group(0);
            let child = command.spawn().map_err(|_| {
                failure(
                    &package.id,
                    &location,
                    "Rd examples require R, pkgload, and evaluate in the prepared environment.",
                )
            })?;
            #[cfg(target_os = "linux")]
            let group = child
                .id()
                .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
            let mut process = Process {
                child,
                #[cfg(target_os = "linux")]
                group,
            };
            let status = tokio::select! {
                result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => {
                    match result {
                        Ok(result) => result?,
                        Err(_) => {
                            terminate(&mut process, deadlines).await?;
                            return Err(failure(&package.id, &location, "Rd example execution timed out."));
                        }
                    }
                }
                () = cancellation.as_mut() => {
                    terminate(&mut process, deadlines).await?;
                    return Err(AssemblyError::Cancelled);
                }
            };
            if !status.success() {
                let message = fs::read_to_string(errors)
                    .unwrap_or_default()
                    .replace(repository.path.to_string_lossy().as_ref(), "<repository>")
                    .replace(staging.path().to_string_lossy().as_ref(), "<examples>");
                return Err(failure(
                    &package.id,
                    &location,
                    format!("Rd example execution failed: {}", message.trim()),
                ));
            }
            let (blocks, activity, generated) = decode(
                &fs::read_to_string(output)?,
                &package.id,
                &location,
                &input_bytes,
                staging.path(),
            )?;
            assets.extend(generated);
            let provenance = Provenance {
                activity,
                source: Some(DiagnosticSource::Repository {
                    repository: location.repository.clone(),
                    path: location.path.clone(),
                }),
                span: None,
                tools: BTreeMap::from([("diplodocus".into(), env!("CARGO_PKG_VERSION").into())]),
            };
            for item in sources
                .workspace
                .packages
                .get_mut(&package.id)
                .unwrap()
                .items
                .values_mut()
            {
                if let Some(document) = &mut item.documentation
                    && document
                        .source_location
                        .as_ref()
                        .is_some_and(|source| source.path == location.path)
                {
                    let index = document
                        .document
                        .blocks
                        .iter()
                        .position(is_example_heading)
                        .unwrap();
                    // Rd preparation emits one display block after the example heading.
                    document
                        .document
                        .blocks
                        .splice(index + 2..index + 2, blocks.clone());
                    document.provenance.push(provenance.clone());
                }
            }
            staging.close()?;
        }
    }
    sources.revalidate()?;
    Ok(assets)
}

async fn terminate(
    process: &mut Process,
    deadlines: ExecutionDeadlines,
) -> Result<(), AssemblyError> {
    process.kill();
    tokio::time::timeout(
        Duration::from_millis(deadlines.forced_exit.max(1)),
        process.child.wait(),
    )
    .await
    .map_err(|_| std::io::Error::other("Rd helper did not exit after termination"))??;
    Ok(())
}

fn is_example_heading(block: &Block) -> bool {
    matches!(block, Block::Heading { attributes, .. } if attributes.classes.iter().any(|class| class.value == "rd-examples"))
}

type Decoded = (
    Vec<Block>,
    ProvenanceActivity,
    BTreeMap<DiagnosticPath, ContentAsset>,
);

fn decode(
    text: &str,
    package: &str,
    location: &SourceLocation,
    input: &[u8],
    staging: &Path,
) -> Result<Decoded, AssemblyError> {
    let invalid = || {
        failure(
            package,
            location,
            "Rd example helper returned invalid output.",
        )
    };
    let span = SourceSpan {
        start: 0,
        end: input.len(),
    };
    let mut blocks = Vec::new();
    let mut assets = BTreeMap::new();
    let mut fields = BTreeMap::new();
    let mut dependencies = BTreeMap::new();
    let mut executed = Vec::new();
    let mut illustrative = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines() {
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
            "text" => blocks.push(Block::CodeBlock {
                language: None,
                source: value,
                source_segments: vec![],
                span,
            }),
            "executed" => executed.push(value),
            "illustrative" => illustrative.push(value),
            "skipped" => skipped.push(value),
            "dependency" => {
                let (name, version) = value.split_once('=').ok_or_else(invalid)?;
                if dependencies.insert(name.into(), version.into()).is_some() {
                    return Err(invalid());
                }
            }
            "figure" => {
                if !value.starts_with("figure-")
                    || !value.ends_with(".png")
                    || value.contains(['/', '\\'])
                {
                    return Err(invalid());
                }
                let path = crate::paths::resolve_input_file(
                    staging,
                    "rd.figure",
                    Path::new(&value),
                    staging,
                )
                .map_err(|_| invalid())?;
                let bytes = fs::read(path)?;
                validate_image_bytes("image/png", &bytes).map_err(|_| invalid())?;
                let fingerprint = fingerprint_bytes(&bytes);
                let target = DiagnosticPath::try_from(format!(
                    "rd-example-assets/sha256/{}.png",
                    fingerprint.value
                ))
                .map_err(|_| invalid())?;
                assets.insert(
                    target.clone(),
                    ContentAsset {
                        fingerprint,
                        media_type: "image/png".into(),
                        bytes,
                    },
                );
                blocks.push(Block::Paragraph {
                    inlines: vec![Inline::Image {
                        alt: vec![Inline::Text {
                            value: "Example figure".into(),
                            span,
                        }],
                        target: target.as_str().into(),
                        title: None,
                        attributes: Attributes::default(),
                        span,
                    }],
                    span,
                });
            }
            "r-version" | "package-version" | "complete" => {
                if fields.insert(key, value).is_some() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    if fields.remove("complete").as_deref() != Some("rd-examples-v1") || dependencies.is_empty() {
        return Err(invalid());
    }
    let activity = ProvenanceActivity::RdExamples {
        package: package.into(),
        policy: "rd-examples-v1".into(),
        r_version: fields
            .remove("r-version")
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)?,
        package_version: fields
            .remove("package-version")
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)?,
        dependencies,
        input: InputFingerprint {
            source: SourceLocation {
                span: None,
                ..location.clone()
            },
            fingerprint: fingerprint_bytes(input),
        },
        executed,
        illustrative,
        skipped,
    };
    Ok((blocks, activity, assets))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn native_figures_cannot_escape_the_owned_staging_directory() {
        let outside = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let image = outside.path().join("image.png");
        image::RgbaImage::new(1, 1).save(&image).unwrap();
        std::os::unix::fs::symlink(image, staging.path().join("figure-1.png")).unwrap();
        let wire = [
            ("figure", "figure-1.png"),
            ("dependency", "rdex=0.1.0"),
            ("r-version", "4.6.1"),
            ("package-version", "0.1.0"),
            ("complete", "rd-examples-v1"),
        ]
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{key}\t{}",
                value
                    .bytes()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
        let location = SourceLocation {
            repository: "r".into(),
            path: "man/answer.Rd".try_into().unwrap(),
            span: None,
        };
        assert!(decode(&wire, "rdex", &location, b"", staging.path()).is_err());
        fs::remove_file(staging.path().join("figure-1.png")).unwrap();
        fs::copy(
            outside.path().join("image.png"),
            staging.path().join("figure-1.png"),
        )
        .unwrap();
        assert!(decode(&wire, "rdex", &location, b"", staging.path()).is_ok());
    }
}
