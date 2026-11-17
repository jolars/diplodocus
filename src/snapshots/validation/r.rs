//! Validate portable native R evidence without consulting a producer checkout.

use super::require;
use crate::ir::*;
use crate::snapshots::SnapshotError;
use std::path::Path;

type Result = std::result::Result<(), SnapshotError>;

pub(super) fn validate(workspace: &Workspace) -> Result {
    for (id, package) in &workspace.packages {
        let runtime = workspace.provenance.iter().filter(|record| {
            matches!(&record.activity, ProvenanceActivity::RRuntime { package, .. } if package == id)
        }).collect::<Vec<_>>();
        let generation = workspace.provenance.iter().filter(|record| {
            matches!(&record.activity, ProvenanceActivity::RDocumentation { package, .. } if package == id)
        }).collect::<Vec<_>>();
        let observed = runtime.first().map(|record| match &record.activity {
            ProvenanceActivity::RRuntime {
                inputs, r_version, ..
            } => (inputs, r_version, &record.tools),
            _ => unreachable!(),
        });
        let mut native_targets = 0;
        for (target_id, target) in &package.extraction_targets {
            let mut count = 0;
            for record in &workspace.provenance {
                if let ProvenanceActivity::Extraction {
                    target: owner,
                    mode: ExtractionMode::Native,
                    inputs,
                    parsers,
                    ..
                } = &record.activity
                    && owner.package == *id
                    && owner.target == *target_id
                    && target.extractor != "python"
                {
                    require(
                        package.ecosystem == "r" && target.extractor == "r",
                        "native extraction ecosystem",
                    )?;
                    count += 1;
                    native_targets += 1;
                    if let Some((observed, r_version, tools)) = observed {
                        let metadata = package
                            .metadata
                            .as_ref()
                            .ok_or(SnapshotError::Invalid("native R package metadata"))?
                            .source();
                        require(
                            inputs.get(&metadata.repository).is_some_and(|files| {
                                files
                                    .get(&metadata.path)
                                    .is_some_and(|input| input.kind == "package-metadata")
                            }),
                            "native R extraction metadata input",
                        )?;
                        for (name, version) in
                            [("R", Some(r_version)), ("pkgload", tools.get("pkgload"))]
                        {
                            require(
                                record.tools.get(name) == version
                                    && parsers.get(name).is_some_and(|parser| {
                                        Some(&parser.version) == version
                                            && parser.role == "r-native-discovery"
                                    }),
                                "native R extraction tool evidence",
                            )?;
                        }
                        for (repository, files) in inputs {
                            for (path, input) in files {
                                require(
                                    observed.iter().any(|observation| {
                                        observation.source.repository == *repository
                                            && observation.source.path == *path
                                            && observation.fingerprint == input.fingerprint
                                    }),
                                    "native R extraction input evidence",
                                )?;
                            }
                        }
                    }
                }
            }
            require(count <= 1, "duplicate native R extraction evidence")?;
            require(
                runtime.is_empty() || target.extractor != "r" || count == 1,
                "R target requires native discovery",
            )?;
        }
        require(
            runtime.len() <= 1 && generation.len() <= 1,
            "duplicate native R provenance",
        )?;
        require(
            (native_targets > 0) == (runtime.len() == 1),
            "native R discovery evidence",
        )?;
        if let Some((inputs, _, _)) = observed {
            for document in package
                .items
                .values()
                .filter_map(|item| item.documentation.as_ref())
            {
                if !matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "rd")
                {
                    continue;
                }
                let source = document
                    .source_location
                    .as_ref()
                    .ok_or(SnapshotError::Invalid("native R documentation source"))?;
                let raw = document
                    .raw_source
                    .as_ref()
                    .ok_or(SnapshotError::Invalid("native R original documentation"))?;
                let fingerprint = crate::provenance::fingerprint_bytes(raw.as_bytes());
                require(
                    inputs.iter().any(|input| {
                        input.source.repository == source.repository
                            && input.source.path == source.path
                            && input.fingerprint == fingerprint
                    }),
                    "native R documentation input evidence",
                )?;
            }
        }
        if let Some(generated) = generation.first() {
            let Some(discovered) = runtime.first() else {
                return Err(SnapshotError::Invalid(
                    "R generation requires native discovery",
                ));
            };
            let ProvenanceActivity::RDocumentation {
                r_version, outputs, ..
            } = &generated.activity
            else {
                unreachable!()
            };
            let ProvenanceActivity::RRuntime {
                r_version: runtime_version,
                inputs,
                ..
            } = &discovered.activity
            else {
                unreachable!()
            };
            require(r_version == runtime_version, "R generation runtime version")?;
            for output in outputs {
                require(inputs.contains(output), "stale generated R input")?;
            }
        }
    }
    Ok(())
}

pub(super) fn provenance(workspace: &Workspace, record: &Provenance) -> Result {
    let (id, r_version, dependencies, inputs) = match &record.activity {
        ProvenanceActivity::RDocumentation {
            package,
            policy,
            r_version,
            roxygen2_version,
            loader,
            roclets,
            dependencies,
            inputs,
            outputs,
        } => {
            require(
                policy == "r-roxygen2-v1"
                    && matches!(loader.as_str(), "pkgload" | "source")
                    && supported_roxygen(roxygen2_version)
                    && record.tools.get("roxygen2") == Some(roxygen2_version)
                    && dependencies.get("roxygen2").is_some_and(|version| {
                        crate::validation::r_versions_equal(version, roxygen2_version)
                    })
                    && roclets.iter().any(|roclet| roclet == "rd")
                    && roclets.iter().all(|roclet| portable_word(roclet)),
                "R documentation provenance",
            )?;
            let owner = r_package(workspace, package)?;
            fingerprints(owner, outputs)?;
            (package, r_version, dependencies, inputs)
        }
        ProvenanceActivity::RRuntime {
            package,
            policy,
            r_version,
            package_version,
            dependencies,
            inputs,
        } => {
            let owner = r_package(workspace, package)?;
            let Some(PackageMetadata::R(metadata)) = &owner.metadata else {
                unreachable!()
            };
            require(
                policy == "r-runtime-v1"
                    && crate::validation::r_versions_equal(
                        package_version,
                        &metadata.description.version,
                    )
                    && dependencies
                        .get(&metadata.description.name)
                        .is_some_and(|version| {
                            crate::validation::r_versions_equal(version, package_version)
                        }),
                "R runtime provenance",
            )?;
            (package, r_version, dependencies, inputs)
        }
        _ => unreachable!(),
    };
    let owner = r_package(workspace, id)?;
    require(
        record.source.is_none()
            && record.span.is_none()
            && version(r_version)
            && record.tools.get("r") == Some(r_version)
            && record
                .tools
                .get("pkgload")
                .is_some_and(|value| dependencies.get("pkgload") == Some(value) && version(value))
            && !dependencies.is_empty()
            && dependencies
                .iter()
                .all(|(name, value)| r_name(name) && version(value)),
        "native R tool evidence",
    )?;
    fingerprints(owner, inputs)?;
    let root = owner
        .path
        .as_ref()
        .map_or(Path::new(""), |path| Path::new(path.as_str()));
    let metadata = root.join(owner.metadata_path.as_str());
    require(
        inputs
            .iter()
            .any(|input| Path::new(input.source.path.as_str()) == metadata),
        "native R metadata input",
    )?;
    Ok(())
}

fn r_package<'a>(
    workspace: &'a Workspace,
    id: &str,
) -> std::result::Result<&'a Package, SnapshotError> {
    workspace
        .packages
        .get(id)
        .filter(|package| {
            package.ecosystem == "r" && matches!(package.metadata, Some(PackageMetadata::R(_)))
        })
        .ok_or(SnapshotError::Invalid("native R package identity"))
}

fn fingerprints(package: &Package, inputs: &[InputFingerprint]) -> Result {
    require(!inputs.is_empty(), "missing native R inputs")?;
    let root = package
        .path
        .as_ref()
        .map_or(Path::new(""), |path| Path::new(path.as_str()));
    require(
        inputs.windows(2).all(|pair| {
            (&pair[0].source.repository, &pair[0].source.path)
                < (&pair[1].source.repository, &pair[1].source.path)
        }),
        "native R input order",
    )?;
    for input in inputs {
        require(
            input.source.repository == package.repository
                && Path::new(input.source.path.as_str()).starts_with(root)
                && input.source.span.is_none()
                && input.fingerprint.algorithm == "sha256"
                && input.fingerprint.value.len() == 64
                && input
                    .fingerprint
                    .value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "native R input fingerprint",
        )?;
    }
    Ok(())
}

fn portable_word(value: &str) -> bool {
    !value.trim().is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
}

fn r_name(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
}

fn version(value: &str) -> bool {
    !value.is_empty()
        && value
            .split(['.', '-'])
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn supported_roxygen(value: &str) -> bool {
    if !version(value) {
        return false;
    }
    let parts = value
        .split(['.', '-'])
        .map(|part| part.parse::<u64>().unwrap_or(u64::MAX))
        .collect::<Vec<_>>();
    parts.first().is_some_and(|major| *major < 9)
        && parts
            .iter()
            .copied()
            .chain(std::iter::repeat(0))
            .take(3)
            .collect::<Vec<_>>()
            .as_slice()
            >= [7, 3, 2].as_slice()
}
