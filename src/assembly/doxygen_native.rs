//! Supervised Doxygen runs with portable source observations.

use super::*;
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::doxygen::{
    DoxygenConfiguration, DoxygenExtraction, configuration, extract_doxygen,
};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};
use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;

pub(super) struct DoxygenNativeExtraction {
    pub extraction: DoxygenExtraction,
    pub metadata: CppMetadata,
    pub provenance: Provenance,
    pub observation: DoxygenObservation,
}

#[derive(Debug, Clone)]
pub(super) struct DoxygenObservation {
    repository: ResolvedRepositoryPaths,
    roots: Vec<PathBuf>,
    configuration: BTreeSet<PathBuf>,
    inputs: Vec<InputFingerprint>,
}

impl DoxygenObservation {
    pub(super) fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.roots.iter()
    }
    pub(super) fn verify(&self) -> Result<(), AssemblyError> {
        if inventory(&self.repository, &self.roots, &self.configuration)? != self.inputs {
            return Err(AssemblyError::InputsChanged);
        }
        Ok(())
    }
}

struct Native<'a> {
    repository: &'a ResolvedRepositoryPaths,
    package: &'a ResolvedPackagePaths,
    target: &'a ResolvedTargetPath,
    staging: &'a Path,
    deadlines: ExecutionDeadlines,
    preparation_limit: Duration,
}

impl Native<'_> {
    fn failure(&self, message: impl Into<String>) -> AssemblyError {
        AssemblyError::Diagnostics(vec![
            Diagnostic::new(DiagnosticCode::DoxygenExtraction, Severity::Error, message)
                .with_entity(DiagnosticEntity::Target {
                    package: self.package.id.clone(),
                    id: self.target.id.clone(),
                }),
        ])
    }

    fn sanitize(&self, source: &str) -> String {
        source
            .replace(
                self.repository.path.to_string_lossy().as_ref(),
                "<repository>",
            )
            .replace(self.staging.to_string_lossy().as_ref(), "<doxygen-staging>")
    }

    async fn run(
        &self,
        arguments: &[&std::ffi::OsStr],
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(String, String), AssemblyError> {
        let output = self.staging.join("stdout");
        let errors = self.staging.join("stderr");
        let mut command = tokio::process::Command::new("doxygen");
        command
            .args(arguments)
            .current_dir(&self.package.path)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&output)?)
            .stderr(fs::File::create(&errors)?)
            .kill_on_drop(true);
        #[cfg(target_os = "linux")]
        command.process_group(0);
        let child = command
            .spawn()
            .map_err(|_| self.failure("Doxygen could not start; prepare `doxygen` on PATH."))?;
        #[cfg(target_os = "linux")]
        let group = child
            .id()
            .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
        let mut process = rd_examples::Process {
            child,
            #[cfg(target_os = "linux")]
            group,
        };
        let status = tokio::select! {
            result = tokio::time::timeout(self.preparation_limit, process.child.wait()) => match result {
                Ok(status) => status?,
                Err(_) => { rd_examples::terminate(&mut process, self.deadlines).await?; return Err(self.failure("Doxygen extraction timed out.")); }
            },
            () = cancellation.as_mut() => { rd_examples::terminate(&mut process, self.deadlines).await?; return Err(AssemblyError::Cancelled); }
        };
        process.kill();
        #[cfg(target_os = "linux")]
        {
            process.group = None;
        }
        let output = fs::read_to_string(output)?;
        let errors = fs::read_to_string(errors)?;
        if !status.success() {
            return Err(self.failure(format!("Doxygen failed: {}", self.sanitize(&errors).trim())));
        }
        Ok((output, errors))
    }

    async fn configuration(
        &self,
        path: &Path,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<(DoxygenConfiguration, String), AssemblyError> {
        if !path.is_file() {
            return Err(
                self.failure("Doxygen targets and metadata paths must identify regular Doxyfiles.")
            );
        }
        let (source, _) = self
            .run(&["-x".as_ref(), path.as_os_str()], cancellation)
            .await?;
        Ok((DoxygenConfiguration::parse(&source), source))
    }
}

pub(super) async fn extract_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<DoxygenNativeExtraction, AssemblyError> {
    let staging = tempfile::tempdir()?;
    let native = Native {
        repository,
        package,
        target,
        staging: staging.path(),
        deadlines,
        preparation_limit: Duration::from_secs(600),
    };
    if !target.path.is_file() {
        return Err(native.failure("Doxygen targets must identify regular Doxyfiles."));
    }
    let mut config_files = BTreeSet::new();
    for path in [&package.metadata_path, &target.path] {
        configurations(
            repository,
            &package.path,
            path,
            &mut config_files,
            &mut vec![],
            &mut vec![],
        )?;
    }
    let config_before = inventory(repository, &[], &config_files)?;
    let (settings, normalized) = native.configuration(&target.path, cancellation).await?;
    let metadata_settings = if package.metadata_path == target.path {
        settings.clone()
    } else {
        native
            .configuration(&package.metadata_path, cancellation)
            .await?
            .0
    };
    let mut roots = Vec::new();
    for key in [
        "INPUT",
        "INCLUDE_PATH",
        "EXAMPLE_PATH",
        "IMAGE_PATH",
        "TAGFILES",
        "USE_MDFILE_AS_MAINPAGE",
        "CITE_BIB_FILES",
        "LAYOUT_FILE",
        "INPUT_FILTER",
        "FILTER_PATTERNS",
    ] {
        if let Some(values) = settings.values.get(key) {
            for value in values {
                if key == "INPUT_FILTER" || key == "FILTER_PATTERNS" {
                    continue;
                }
                let value = if key == "TAGFILES" {
                    value.split('=').next().unwrap_or(value)
                } else {
                    value
                };
                if value.is_empty() {
                    continue;
                }
                let candidate = package.path.join(value);
                let path = within(repository, &candidate)?;
                if path.is_file() || path.is_dir() {
                    roots.push(path);
                }
            }
        }
    }
    if !settings.values.contains_key("INPUT") {
        roots.push(package.path.clone());
    }
    // Maintained local helpers and preprocessing inputs also affect native output.
    roots.push(package.path.clone());
    roots.sort();
    roots.dedup();
    let before = inventory(repository, &roots, &config_files)?;
    if inventory(repository, &[], &config_files)? != config_before {
        return Err(AssemblyError::InputsChanged);
    }
    let wrapper = staging.path().join("Doxyfile");
    let quoted = |path: &Path| {
        format!(
            "\"{}\"",
            path.to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    };
    let mut configuration = format!(
        "@INCLUDE = {}\nOUTPUT_DIRECTORY = {}\nXML_OUTPUT = xml\nGENERATE_XML = YES\nCREATE_SUBDIRS = NO\nXML_PROGRAMLISTING = NO\nHAVE_DOT = NO\nQUIET = YES\nGENERATE_TAGFILE =\nWARN_LOGFILE =\n",
        quoted(&target.path),
        quoted(staging.path())
    );
    for format in [
        "HTML",
        "LATEX",
        "RTF",
        "MAN",
        "DOCBOOK",
        "AUTOGEN_DEF",
        "PERLMOD",
        "SQLITE3",
    ] {
        configuration.push_str(&format!("GENERATE_{format} = NO\n"));
    }
    fs::write(&wrapper, configuration)?;
    let (version, _) = native.run(&["--version".as_ref()], cancellation).await?;
    let (_, warnings) = native.run(&[wrapper.as_os_str()], cancellation).await?;
    let xml = staging.path().join("xml");
    let index = fs::read_to_string(xml.join("index.xml"))
        .map_err(|_| native.failure("Doxygen did not produce XML index.xml."))?;
    let document = roxmltree::Document::parse(&index)
        .map_err(|_| native.failure("Doxygen produced malformed XML index.xml."))?;
    let mut files = BTreeMap::new();
    for entry in document
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("compound"))
    {
        if matches!(
            entry.attribute("kind"),
            Some("page" | "group" | "dir" | "example")
        ) {
            continue;
        }
        let id = entry
            .attribute("refid")
            .ok_or_else(|| native.failure("Doxygen compound has no refid."))?;
        if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(native.failure("Doxygen compound has an invalid XML filename."));
        }
        let name = format!("{id}.xml");
        let path = crate::paths::resolve_input_file(&xml, "doxygen.xml", Path::new(&name), &xml)?;
        files.insert(name, fs::read_to_string(path)?);
    }
    files.insert("index.xml".into(), index);
    let mut extraction = extract_doxygen(repository, package, target, &files, &settings);
    if !warnings.trim().is_empty() {
        extraction.diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::DoxygenUnsupported,
                Severity::Warning,
                format!("Doxygen: {}", native.sanitize(&warnings).trim()),
            )
            .with_entity(DiagnosticEntity::Target {
                package: package.id.clone(),
                id: target.id.clone(),
            }),
        );
    }
    reject_errors(extraction.diagnostics.clone())?;
    let observation = DoxygenObservation {
        repository: repository.clone(),
        roots,
        configuration: config_files,
        inputs: before,
    };
    observation.verify()?;
    let mut current_configs = BTreeSet::new();
    for path in [&package.metadata_path, &target.path] {
        configurations(
            repository,
            &package.path,
            path,
            &mut current_configs,
            &mut vec![],
            &mut vec![],
        )?;
    }
    if current_configs != observation.configuration {
        return Err(AssemblyError::InputsChanged);
    }
    let optional = |key| {
        let value = metadata_settings.value(key, "");
        (!value.is_empty()).then(|| value.to_owned())
    };
    let metadata = CppMetadata {
        name: metadata_settings.value("PROJECT_NAME", "My Project").into(),
        version: optional("PROJECT_NUMBER"),
        description: optional("PROJECT_BRIEF"),
        source: repository.source_location(&package.metadata_path, None)?,
    };
    let provenance = Provenance {
        activity: ProvenanceActivity::Extraction {
            target: TargetReference {
                package: package.id.clone(),
                target: target.id.clone(),
            },
            mode: ExtractionMode::Static,
            capabilities: BTreeSet::from(["doxygen-cpp-api-v1".into()]),
            parsers: BTreeMap::from([(
                "doxygen".into(),
                ParserProvenance {
                    version: version.trim().into(),
                    role: "cpp-api".into(),
                    settings: BTreeMap::from([(
                        "configuration-sha256".into(),
                        fingerprint_bytes(native.sanitize(&normalized).as_bytes()).value,
                    )]),
                },
            )]),
            inputs: BTreeMap::from([(
                repository.id.clone(),
                observation
                    .inputs
                    .iter()
                    .map(|input| {
                        (
                            input.source.path.clone(),
                            ExtractionInput {
                                kind: "cpp-source".into(),
                                fingerprint: input.fingerprint.clone(),
                                parsers: BTreeSet::from(["doxygen".into()]),
                            },
                        )
                    })
                    .collect(),
            )]),
        },
        source: Some(source(&repository.source_location(&target.path, None)?)),
        span: None,
        tools: BTreeMap::from([
            ("doxygen".into(), version.trim().into()),
            ("roxmltree".into(), "0.21.2".into()),
        ]),
    };
    staging.close()?;
    Ok(DoxygenNativeExtraction {
        extraction,
        metadata,
        provenance,
        observation,
    })
}

fn within(repository: &ResolvedRepositoryPaths, path: &Path) -> Result<PathBuf, AssemblyError> {
    let relative = path
        .strip_prefix(&repository.path)
        .map_err(|_| AssemblyError::InputsChanged)?;
    Ok(crate::paths::resolve_repository_target(
        if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        },
        &repository.path,
    )?)
}

fn configurations(
    repository: &ResolvedRepositoryPaths,
    working: &Path,
    path: &Path,
    files: &mut BTreeSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    search: &mut Vec<PathBuf>,
) -> Result<(), AssemblyError> {
    let path = within(repository, path)?;
    if stack.contains(&path) {
        return Err(AssemblyError::Diagnostics(vec![diagnostic(
            DiagnosticCode::DoxygenExtraction,
            "Doxyfile includes form a cycle.",
        )]));
    }
    files.insert(path.clone());
    stack.push(path.clone());
    let source =
        configuration::logical_source(&configuration::environment(&fs::read_to_string(&path)?));
    for line in source.lines() {
        let settings = DoxygenConfiguration::parse(line);
        if let Some(paths) = settings.values.get("@INCLUDE_PATH") {
            if !line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim_end().ends_with('+'))
            {
                search.clear();
            }
            search.extend(paths.iter().map(|path| working.join(path)));
        }
        if let Some(includes) = settings.values.get("@INCLUDE") {
            for include in includes {
                let included = search
                    .iter()
                    .map(|root| root.join(include))
                    .chain(std::iter::once(working.join(include)))
                    .find(|path| path.is_file())
                    .unwrap_or_else(|| working.join(include));
                configurations(repository, working, &included, files, stack, search)?;
            }
        }
    }
    stack.pop();
    Ok(())
}

fn inventory(
    repository: &ResolvedRepositoryPaths,
    roots: &[PathBuf],
    configuration: &BTreeSet<PathBuf>,
) -> Result<Vec<InputFingerprint>, AssemblyError> {
    fn walk(
        repository: &ResolvedRepositoryPaths,
        path: &Path,
        files: &mut BTreeSet<PathBuf>,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), AssemblyError> {
        let path = within(repository, path)?;
        if path.is_file() {
            files.insert(path);
        } else if visited.insert(path.clone()) {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                if !configuration::excluded(&entry.file_name().to_string_lossy()) {
                    walk(repository, &entry.path(), files, visited)?;
                }
            }
        }
        Ok(())
    }
    let mut files = configuration.clone();
    let mut visited = BTreeSet::new();
    for root in roots {
        walk(repository, root, &mut files, &mut visited)?;
    }
    files
        .into_iter()
        .map(|path| {
            Ok(InputFingerprint {
                source: repository.source_location(&path, None)?,
                fingerprint: fingerprint_bytes(&fs::read(path)?),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_timeout_and_cancellation_stop_input_filters() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("diplodocus.yaml"),
            include_str!("../../tests/fixtures/doxygen-api/diplodocus.yaml"),
        )
        .unwrap();
        fs::write(root.path().join("Doxyfile"), "INPUT = include\nINPUT_FILTER = \"sh slow-filter.sh\"\nGENERATE_HTML = NO\nGENERATE_LATEX = NO\n").unwrap();
        fs::create_dir(root.path().join("include")).unwrap();
        fs::write(root.path().join("include/api.hpp"), "struct Example {};\n").unwrap();
        fs::write(
            root.path().join("slow-filter.sh"),
            "echo $$ > filter.pid\nsleep 60\ncat \"$1\"\n",
        )
        .unwrap();
        let path = root.path().join("diplodocus.yaml");
        let configuration = crate::configuration::load_configuration(&path).unwrap();
        let paths = resolve_workspace_paths(&path, &configuration).unwrap();
        let package = &paths.packages[0];
        let native = Native {
            repository: &paths.repositories[0],
            package,
            target: &package.targets[0],
            staging: root.path(),
            deadlines: ExecutionDeadlines::default(),
            preparation_limit: Duration::from_millis(100),
        };
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
        let failed = native
            .run(&[package.metadata_path.as_os_str()], &mut cancellation)
            .await
            .unwrap_err();
        assert!(
            matches!(failed, AssemblyError::Diagnostics(ref diagnostics) if diagnostics[0].message.contains("timed out"))
        );
        let native = Native {
            preparation_limit: Duration::from_secs(600),
            ..native
        };
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let failed = native
            .run(&[package.metadata_path.as_os_str()], &mut cancellation)
            .await
            .unwrap_err();
        assert!(matches!(failed, AssemblyError::Cancelled));
    }
}
