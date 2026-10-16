//! Compiler-backed Rust extraction and supervised native doctests.

use super::*;
use crate::configuration::RustTargetConfiguration;
use crate::diagnostics::DiagnosticEntity;
use crate::extractors::rust::{RustExtraction, extract_rustdoc};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

const COMPILER_COMMIT: &str = "d080e7dff1b0fc54541545252818f8cccf995d05";
const PREPARATION_LIMIT: Duration = Duration::from_secs(600);

pub(crate) struct RustNativeExtraction {
    pub extraction: RustExtraction,
    pub provenance: Provenance,
    pub doctests: Option<Provenance>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<CargoPackage>,
    workspace_root: PathBuf,
    resolve: Option<Resolve>,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    source: Option<String>,
    targets: Vec<CargoTarget>,
}

#[derive(Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Deserialize)]
struct ResolveNode {
    id: String,
    features: Vec<String>,
    dependencies: Vec<String>,
}

struct Native<'a> {
    repository: &'a ResolvedRepositoryPaths,
    package: &'a ResolvedPackagePaths,
    target: &'a ResolvedTargetPath,
    settings: &'a RustTargetConfiguration,
    toolchain: PathBuf,
    build: PathBuf,
    deadlines: ExecutionDeadlines,
    rustdoc_flags: Vec<String>,
}

impl Native<'_> {
    fn failure(&self, code: DiagnosticCode, message: impl Into<String>) -> AssemblyError {
        AssemblyError::Diagnostics(vec![
            Diagnostic::new(code, Severity::Error, message).with_entity(DiagnosticEntity::Target {
                package: self.package.id.clone(),
                id: self.target.id.clone(),
            }),
        ])
    }

    fn sanitize(&self, text: &str) -> String {
        let text = if let Some(cargo_home) = std::env::var_os("CARGO_HOME").or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo").into_os_string())
        }) {
            text.replace(cargo_home.to_string_lossy().as_ref(), "<cargo-home>")
        } else {
            text.to_owned()
        };
        text.replace(
            self.repository.path.to_string_lossy().as_ref(),
            "<repository>",
        )
        .replace(self.build.to_string_lossy().as_ref(), "<rust-cache>")
        .replace(
            self.toolchain.to_string_lossy().as_ref(),
            "<rust-toolchain>",
        )
    }

    fn command(&self, tool: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(self.toolchain.join("bin").join(tool));
        let mut paths = vec![self.toolchain.join("bin")];
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path));
        }
        command
            .current_dir(&self.package.path)
            .env("RUSTC", self.toolchain.join("bin/rustc"))
            .env("RUSTDOC", self.toolchain.join("bin/rustdoc"))
            .env("CARGO", self.toolchain.join("bin/cargo"))
            .env("CARGO_TARGET_DIR", &self.build)
            .env("CARGO_TERM_COLOR", "never")
            .env(
                "PATH",
                std::env::join_paths(paths).expect("existing process search paths"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    fn cargo(
        &self,
        operation: &str,
        package: &CargoPackage,
        host: &str,
    ) -> tokio::process::Command {
        let mut command = self.command("cargo");
        command
            .arg(operation)
            .args(["--offline", "--locked", "--manifest-path"])
            .arg(&self.package.metadata_path)
            .args(["--package", &package.id, "--target", host]);
        if operation == "rustdoc" {
            command.arg("--lib");
        }
        if !self.settings.default_features {
            command.arg("--no-default-features");
        }
        if !self.settings.features.is_empty() {
            command
                .arg("--features")
                .arg(self.settings.features.join(","));
        }
        command
    }

    fn doctest_flags(&self, discovery: bool) -> String {
        let mut flags = self.rustdoc_flags.clone();
        flags.extend(["-Zunstable-options".into(), "--merge-doctests=no".into()]);
        if discovery {
            flags.push("--output-format=doctest".into());
        }
        flags.join("\u{1f}")
    }

    async fn run(
        &self,
        mut command: tokio::process::Command,
        code: DiagnosticCode,
        label: &str,
        test_events: bool,
        cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<String, AssemblyError> {
        validate_cache_tree(&self.build).map_err(|_| self.failure(DiagnosticCode::RustExtraction, "The Rust compilation cache must contain regular files and directories without symbolic links."))?;
        let staging = tempfile::tempdir()?;
        let error_path = staging.path().join("stderr");
        command.stderr(fs::File::create(&error_path)?);
        #[cfg(target_os = "linux")]
        command.process_group(0);
        let child = command.spawn().map_err(|_| {
            self.failure(code, format!("{label} could not start; prepare the pinned Rust toolchain and offline Cargo dependencies."))
        })?;
        #[cfg(target_os = "linux")]
        let group = child
            .id()
            .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
        let mut process = rd_examples::Process {
            child,
            #[cfg(target_os = "linux")]
            group,
        };
        let stdout = process.child.stdout.take().expect("piped compiler output");
        let mut lines = BufReader::new(stdout).lines();
        let deadline = tokio::time::sleep(PREPARATION_LIMIT);
        tokio::pin!(deadline);
        let mut output = String::new();
        let mut status = None;
        let mut eof = false;
        let mut active_test = false;
        while status.is_none() || !eof {
            tokio::select! {
                line = lines.next_line(), if !eof => {
                    match line? {
                        Some(line) => {
                            if test_events
                                && let Ok(event) = serde_json::from_str::<Value>(&line)
                                && event["type"] == "test"
                            {
                                active_test = event["event"] == "started";
                                let limit = if active_test { Duration::from_millis(self.deadlines.cell.max(1)) } else { PREPARATION_LIMIT };
                                deadline.as_mut().reset(tokio::time::Instant::now() + limit);
                            }
                            output.push_str(&line);
                            output.push('\n');
                        }
                        None => eof = true,
                    }
                }
                result = process.child.wait(), if status.is_none() => {
                    status = Some(result?);
                    // A completed runner must not leave example descendants alive.
                    process.kill();
                    #[cfg(target_os = "linux")]
                    { process.group = None; }
                    deadline.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(self.deadlines.forced_exit.max(1)));
                }
                () = &mut deadline => {
                    rd_examples::terminate(&mut process, self.deadlines).await?;
                    let phase = if active_test { "individual doctest" } else { "native preparation" };
                    return Err(self.failure(code, format!("{label} timed out during {phase}.")));
                }
                () = cancellation.as_mut() => {
                    rd_examples::terminate(&mut process, self.deadlines).await?;
                    return Err(AssemblyError::Cancelled);
                }
            }
        }
        if !status.expect("reaped Rust compiler").success() {
            let errors = self.sanitize(&fs::read_to_string(&error_path)?);
            let output = self.sanitize(&output);
            return Err(self.failure(
                code,
                format!("{label} failed: {}\n{}", errors.trim(), output.trim()),
            ));
        }
        staging.close()?;
        Ok(output)
    }
}

/// Compile the explicitly declared library and retain portable compiler evidence.
#[allow(clippy::too_many_arguments)]
pub(super) async fn extract_target(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    target: &ResolvedTargetPath,
    settings: &RustTargetConfiguration,
    cache_root: &Path,
    run_examples: bool,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<RustNativeExtraction, AssemblyError> {
    let toolchain = std::env::var_os("DIPLODOCUS_RUST_TOOLCHAIN")
        .map(PathBuf::from)
        .ok_or_else(|| AssemblyError::Diagnostics(vec![Diagnostic::new(
            DiagnosticCode::RustToolchain,
            Severity::Error,
            "Rust extraction requires DIPLODOCUS_RUST_TOOLCHAIN pointing to the prepared nightly-2026-09-28 toolchain.",
        )]))?;
    let key = fingerprint_bytes(
        format!("{}:{}:{}", repository.path.display(), package.id, target.id).as_bytes(),
    );
    let mut native = Native {
        repository,
        package,
        target,
        settings,
        toolchain,
        build: cache_root.join("rust").join(key.value),
        deadlines,
        rustdoc_flags: Vec::new(),
    };
    prepare_build_directory(&native.build).map_err(|_| {
        native.failure(
            DiagnosticCode::RustExtraction,
            "The Rust compilation cache must use directories without symbolic links.",
        )
    })?;
    let mut compiler_command = native.command("rustc");
    compiler_command.args(["--version", "--verbose"]);
    let compiler = native
        .run(
            compiler_command,
            DiagnosticCode::RustToolchain,
            "Rust compiler inspection",
            false,
            cancellation,
        )
        .await?;
    let fields: BTreeMap<_, _> = compiler
        .lines()
        .filter_map(|line| line.split_once(": "))
        .collect();
    if fields.get("commit-hash") != Some(&COMPILER_COMMIT) {
        return Err(native.failure(
            DiagnosticCode::RustToolchain,
            "Rust extraction requires the pinned nightly-2026-09-28 compiler (d080e7dff).",
        ));
    }
    let host = fields.get("host").ok_or_else(|| {
        native.failure(
            DiagnosticCode::RustToolchain,
            "The Rust compiler did not report a host target.",
        )
    })?;
    let compiler_version = compiler.lines().next().unwrap_or_default().to_owned();
    let mut rustdoc_command = native.command("rustdoc");
    rustdoc_command.args(["--version", "--verbose"]);
    let rustdoc_version = native
        .run(
            rustdoc_command,
            DiagnosticCode::RustToolchain,
            "Rustdoc inspection",
            false,
            cancellation,
        )
        .await?;
    if !rustdoc_version
        .lines()
        .any(|line| line == format!("commit-hash: {COMPILER_COMMIT}"))
    {
        return Err(native.failure(
            DiagnosticCode::RustToolchain,
            "Rustdoc and rustc must come from the same pinned toolchain.",
        ));
    }
    let mut cargo_version_command = native.command("cargo");
    cargo_version_command.arg("--version");
    let cargo_version = native
        .run(
            cargo_version_command,
            DiagnosticCode::RustToolchain,
            "Cargo inspection",
            false,
            cancellation,
        )
        .await?;
    check_host_configuration(&native, host)?;
    let mut configuration_command = native.command("cargo");
    configuration_command.args(["-Zunstable-options", "config", "get", "--format=json"]);
    let cargo_configuration: Value = serde_json::from_str(
        &native
            .run(
                configuration_command,
                DiagnosticCode::RustMetadata,
                "Cargo configuration inspection",
                false,
                cancellation,
            )
            .await?,
    )
    .map_err(|_| {
        native.failure(
            DiagnosticCode::RustMetadata,
            "Cargo returned invalid effective configuration.",
        )
    })?;
    if let Some(configured_target) = cargo_configuration
        .get("build")
        .and_then(|build| build.get("target"))
        && configured_target.as_str() != Some(host)
    {
        return Err(native.failure(DiagnosticCode::RustToolchain, "Rust extraction initially supports the compiler host target only; remove the cross-target Cargo configuration."));
    }
    native.rustdoc_flags = effective_rustdoc_flags(&cargo_configuration);
    let initial_roots = vec![package.path.clone()];
    let initial = inventory(repository, &initial_roots, &BTreeSet::new(), &native.build)?;
    let mut metadata_command = native.command("cargo");
    metadata_command
        .args([
            "metadata",
            "--format-version=1",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(&package.metadata_path)
        .args(["--filter-platform", host]);
    if !settings.default_features {
        metadata_command.arg("--no-default-features");
    }
    if !settings.features.is_empty() {
        metadata_command
            .arg("--features")
            .arg(settings.features.join(","));
    }
    let metadata: Metadata = serde_json::from_str(
        &native
            .run(
                metadata_command,
                DiagnosticCode::RustMetadata,
                "Cargo metadata",
                false,
                cancellation,
            )
            .await?,
    )
    .map_err(|_| {
        native.failure(
            DiagnosticCode::RustMetadata,
            "Cargo returned invalid package metadata.",
        )
    })?;
    if inventory(repository, &initial_roots, &BTreeSet::new(), &native.build)? != initial {
        return Err(AssemblyError::InputsChanged);
    }
    let selected = metadata
        .packages
        .iter()
        .find(|candidate| {
            fs::canonicalize(&candidate.manifest_path).ok().as_ref() == Some(&package.metadata_path)
        })
        .ok_or_else(|| {
            native.failure(
                DiagnosticCode::RustMetadata,
                "Cargo metadata did not contain the declared package manifest.",
            )
        })?;
    let library = selected
        .targets
        .iter()
        .find(|candidate| {
            candidate.kind.iter().any(|kind| {
                matches!(
                    kind.as_str(),
                    "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                )
            })
        })
        .ok_or_else(|| {
            native.failure(
                DiagnosticCode::RustMetadata,
                "The declared Rust package has no library target.",
            )
        })?;
    if fs::canonicalize(&library.src_path)? != target.path {
        return Err(native.failure(
            DiagnosticCode::RustMetadata,
            "The declared Rust source must match Cargo's explicit library target.",
        ));
    }
    if !metadata.workspace_root.starts_with(&repository.path) {
        return Err(native.failure(
            DiagnosticCode::RustMetadata,
            "The Cargo workspace must stay inside the declared repository.",
        ));
    }
    if metadata.packages.iter().any(|dependency| {
        dependency.source.is_none() && !dependency.manifest_path.starts_with(&repository.path)
    }) {
        return Err(native.failure(DiagnosticCode::RustMetadata, "Local Cargo dependencies must stay inside the declared repository so their source inputs can be fingerprinted."));
    }
    let roots: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| p.source.is_none())
        .filter_map(|p| p.manifest_path.parent())
        .filter(|p| p.starts_with(&repository.path))
        .map(Path::to_owned)
        .collect();
    let mut extra = BTreeSet::new();
    let before = inventory(repository, &roots, &extra, &native.build)?;
    let dep_info = native.build.join("rustdoc-inputs.d");
    let mut dep_command = native.cargo("rustdoc", selected, host);
    dep_command
        .arg("--")
        .args(["-Zunstable-options", "--document-private-items"])
        .arg(format!("--emit=dep-info={}", dep_info.display()));
    native
        .run(
            dep_command,
            DiagnosticCode::RustExtraction,
            "Rustdoc input discovery",
            false,
            cancellation,
        )
        .await?;
    if inventory(repository, &roots, &extra, &native.build)? != before {
        return Err(AssemblyError::InputsChanged);
    }
    for path in dependency_paths(&fs::read_to_string(&dep_info)?) {
        let absolute = if path.is_absolute() {
            path
        } else {
            package.path.join(path)
        };
        if absolute.is_file() {
            let canonical = fs::canonicalize(&absolute)?;
            if (absolute.starts_with(&repository.path) || canonical.starts_with(&repository.path))
                && !canonical.starts_with(&native.build)
            {
                let source = repository.source_location(&absolute, None)?;
                extra.insert(repository.path.join(source.path.as_str()));
            }
        }
    }
    let inputs = inventory(repository, &roots, &extra, &native.build)?;
    let mut extract_command = native.cargo("rustdoc", selected, host);
    extract_command.arg("--").args([
        "-Zunstable-options",
        "--output-format=json",
        "--document-private-items",
    ]);
    native
        .run(
            extract_command,
            DiagnosticCode::RustExtraction,
            "Rustdoc API extraction",
            false,
            cancellation,
        )
        .await?;
    if inventory(repository, &roots, &extra, &native.build)? != inputs {
        return Err(AssemblyError::InputsChanged);
    }
    let json_path = native
        .build
        .join(host)
        .join("doc")
        .join(format!("{}.json", library.name.replace('-', "_")));
    let json = dependency_documentation_urls(&metadata, &fs::read(&json_path)?)?;
    let extraction = extract_rustdoc(repository, package, target, &json);
    let parser = "rustdoc-json".to_owned();
    let input_records = inputs
        .iter()
        .map(|input| {
            (
                input.source.path.clone(),
                ExtractionInput {
                    kind: "rust-compiler-input".into(),
                    fingerprint: input.fingerprint.clone(),
                    parsers: BTreeSet::from([
                        parser.clone(),
                        "panache-parser".into(),
                        "rustdoc-types".into(),
                    ]),
                },
            )
        })
        .collect();
    let features = metadata
        .resolve
        .as_ref()
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == selected.id))
        .map(|node| node.features.clone())
        .unwrap_or_default();
    let provenance = Provenance {
        activity: ProvenanceActivity::Extraction {
            target: TargetReference {
                package: package.id.clone(),
                target: target.id.clone(),
            },
            mode: ExtractionMode::Compiler,
            capabilities: BTreeSet::from([
                "rust-public-api".into(),
                "rustdoc-links".into(),
                "rust-implementations".into(),
            ]),
            parsers: BTreeMap::from([
                (
                    "rustdoc-types".into(),
                    ParserProvenance {
                        version: "0.61.0".into(),
                        role: "rustdoc-json-schema".into(),
                        settings: BTreeMap::from([(
                            "format-version".into(),
                            rustdoc_types::FORMAT_VERSION.to_string(),
                        )]),
                    },
                ),
                (
                    "panache-parser".into(),
                    ParserProvenance {
                        version: crate::provenance::PANACHE_VERSION.into(),
                        role: "rustdoc-markdown".into(),
                        settings: BTreeMap::from([("format".into(), "gfm".into())]),
                    },
                ),
                (
                    parser,
                    ParserProvenance {
                        version: "61".into(),
                        role: "rust-public-api".into(),
                        settings: BTreeMap::from([
                            ("toolchain".into(), "nightly-2026-09-28".into()),
                            ("host".into(), (*host).into()),
                            ("features".into(), features.join(",")),
                            ("native-preparation-ms".into(), "600000".into()),
                            ("doctest-cell-ms".into(), deadlines.cell.max(1).to_string()),
                            (
                                "rustdoc-flags-sha256".into(),
                                fingerprint_bytes(native.rustdoc_flags.join("\u{1f}").as_bytes())
                                    .value,
                            ),
                            (
                                "default-features".into(),
                                settings.default_features.to_string(),
                            ),
                        ]),
                    },
                ),
            ]),
            inputs: BTreeMap::from([(repository.id.clone(), input_records)]),
        },
        source: Some(DiagnosticSource::Repository {
            repository: repository.id.clone(),
            path: repository.source_location(&target.path, None)?.path,
        }),
        span: None,
        tools: BTreeMap::from([
            (
                "cargo".into(),
                cargo_version.lines().next().unwrap_or_default().into(),
            ),
            ("rustc".into(), compiler_version.clone()),
            (
                "rustdoc".into(),
                rustdoc_version.lines().next().unwrap_or_default().into(),
            ),
            ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
        ]),
    };
    let doctests = if run_examples {
        let examples = execute_doctests(&native, selected, host, cancellation).await?;
        if inventory(repository, &roots, &extra, &native.build)? != inputs {
            return Err(AssemblyError::InputsChanged);
        }
        Some(Provenance {
            activity: ProvenanceActivity::RustDoctests {
                target: TargetReference {
                    package: package.id.clone(),
                    target: target.id.clone(),
                },
                policy: "rustdoc-doctest-v1".into(),
                package_name: selected.name.clone(),
                package_version: selected.version.clone(),
                compiler_version,
                dependencies: dependencies(&metadata, selected),
                features,
                host: (*host).into(),
                inputs,
                examples,
            },
            source: None,
            span: None,
            tools: provenance.tools.clone(),
        })
    } else {
        None
    };
    Ok(RustNativeExtraction {
        extraction,
        provenance,
        doctests,
    })
}

fn prepare_build_directory(path: &Path) -> std::io::Result<()> {
    if !path.is_absolute() {
        return Err(std::io::Error::other(
            "Rust cache requires an absolute path",
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(std::io::Error::other(
                "Rust cache cannot traverse parent directories",
            ));
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(std::io::Error::other(
                    "Rust cache contains a symbolic link or nondirectory",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                if !fs::symlink_metadata(&current)?.file_type().is_dir() {
                    return Err(std::io::Error::other(
                        "Rust cache directory changed during creation",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    validate_cache_tree(path)
}

fn validate_cache_tree(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_file() {
        return Ok(());
    }
    if !metadata.file_type().is_dir() {
        return Err(std::io::Error::other(
            "Rust cache contains a symbolic link or special file",
        ));
    }
    for entry in fs::read_dir(path)? {
        validate_cache_tree(&entry?.path())?;
    }
    Ok(())
}

fn dependency_documentation_urls(
    metadata: &Metadata,
    input: &[u8],
) -> Result<Vec<u8>, AssemblyError> {
    let Ok(mut json) = serde_json::from_slice::<Value>(input) else {
        return Ok(input.to_vec());
    };
    if let Some(crates) = json
        .get_mut("external_crates")
        .and_then(Value::as_object_mut)
    {
        for dependency in crates.values_mut() {
            if dependency
                .get("html_root_url")
                .and_then(Value::as_str)
                .is_some_and(|root| !root.starts_with("https://docs.rs/"))
            {
                continue;
            }
            let Some(name) = dependency.get("name").and_then(Value::as_str) else {
                continue;
            };
            let candidates: Vec<_> = metadata
                .packages
                .iter()
                .filter(|package| {
                    package.targets.iter().any(|target| {
                        target.name.replace('-', "_") == name
                            && target.kind.iter().any(|kind| {
                                matches!(
                                    kind.as_str(),
                                    "lib"
                                        | "rlib"
                                        | "dylib"
                                        | "cdylib"
                                        | "staticlib"
                                        | "proc-macro"
                                )
                            })
                    })
                })
                .collect();
            if let [package] = candidates.as_slice() {
                dependency["html_root_url"] = Value::String(format!(
                    "https://docs.rs/{}/{}/{}/",
                    package.name, package.version, name
                ));
            }
        }
    }
    serde_json::to_vec(&json).map_err(|error| AssemblyError::Io(std::io::Error::other(error)))
}

fn dependencies(metadata: &Metadata, package: &CargoPackage) -> BTreeMap<String, String> {
    let mut pending = vec![package.id.as_str()];
    let mut reached = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !reached.insert(id) {
            continue;
        }
        if let Some(node) = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == id))
        {
            pending.extend(node.dependencies.iter().map(String::as_str));
        }
    }
    metadata
        .packages
        .iter()
        .filter(|p| p.id != package.id && reached.contains(p.id.as_str()))
        .map(|p| (format!("{}@{}", p.name, p.version), p.version.clone()))
        .collect()
}

fn effective_rustdoc_flags(configuration: &Value) -> Vec<String> {
    if let Ok(flags) = std::env::var("CARGO_ENCODED_RUSTDOCFLAGS") {
        return flags
            .split('\u{1f}')
            .filter(|flag| !flag.is_empty())
            .map(str::to_owned)
            .collect();
    }
    for name in ["RUSTDOCFLAGS", "CARGO_BUILD_RUSTDOCFLAGS"] {
        if let Ok(flags) = std::env::var(name) {
            return flags.split_whitespace().map(str::to_owned).collect();
        }
    }
    match configuration
        .get("build")
        .and_then(|build| build.get("rustdocflags"))
    {
        Some(Value::Array(flags)) => flags
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(flags)) => flags.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

fn check_host_configuration(native: &Native<'_>, host: &str) -> Result<(), AssemblyError> {
    if let Some(target) = std::env::var_os("CARGO_BUILD_TARGET")
        && target != host
    {
        return Err(native.failure(DiagnosticCode::RustToolchain, "Rust extraction initially supports the compiler host target only; remove the cross-target Cargo configuration."));
    }
    for ancestor in native
        .package
        .path
        .ancestors()
        .take_while(|path| path.starts_with(&native.repository.path))
    {
        for name in [".cargo/config", ".cargo/config.toml"] {
            let path = ancestor.join(name);
            if !path.is_file() {
                continue;
            }
            native.repository.source_location(&path, None)?;
            let value: toml::Value = toml::from_str(&fs::read_to_string(path)?).map_err(|_| {
                native.failure(
                    DiagnosticCode::RustMetadata,
                    "Cargo configuration could not be parsed.",
                )
            })?;
            if let Some(target) = value.get("build").and_then(|build| build.get("target"))
                && target.as_str() != Some(host)
            {
                return Err(native.failure(DiagnosticCode::RustToolchain, "Rust extraction initially supports the compiler host target only; remove the cross-target Cargo configuration."));
            }
        }
    }
    Ok(())
}

fn excluded(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".devenv"
            | ".direnv"
            | ".cache"
            | ".diplodocus"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "_site"
            | ".docs-sources"
            | "site"
            | "artifacts"
            | "coverage"
            | "__pycache__"
            | ".playwright-cli"
            | "playwright-report"
            | "test-results"
    )
}

fn inventory(
    repository: &ResolvedRepositoryPaths,
    roots: &[PathBuf],
    extra: &BTreeSet<PathBuf>,
    generated: &Path,
) -> Result<Vec<InputFingerprint>, AssemblyError> {
    fn walk(
        path: &Path,
        files: &mut BTreeSet<PathBuf>,
        visited: &mut BTreeSet<PathBuf>,
        repository: &ResolvedRepositoryPaths,
        generated: &Path,
    ) -> Result<(), AssemblyError> {
        let relative = path
            .strip_prefix(&repository.path)
            .map_err(|_| AssemblyError::InputsChanged)?;
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        let canonical = crate::paths::resolve_repository_target(relative, &repository.path)?;
        if canonical.starts_with(generated) {
            return Ok(());
        }
        if !canonical.starts_with(&repository.path) {
            return Err(AssemblyError::InputsChanged);
        }
        if canonical.is_file() {
            files.insert(canonical);
            return Ok(());
        }
        if !visited.insert(canonical.clone()) {
            return Ok(());
        }
        for entry in fs::read_dir(canonical)? {
            let entry = entry?;
            if !excluded(entry.file_name().to_string_lossy().as_ref()) {
                walk(&entry.path(), files, visited, repository, generated)?;
            }
        }
        Ok(())
    }
    let mut files = extra.clone();
    let mut visited = BTreeSet::new();
    for root in roots {
        walk(root, &mut files, &mut visited, repository, generated)?;
        for ancestor in root
            .ancestors()
            .take_while(|path| path.starts_with(&repository.path))
        {
            for name in [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain",
                "rust-toolchain.toml",
                ".cargo/config",
                ".cargo/config.toml",
            ] {
                let candidate = ancestor.join(name);
                if candidate.is_file() {
                    files.insert(candidate);
                }
            }
        }
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

fn dependency_paths(input: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut token = String::new();
    let mut escaped = false;
    let mut dependencies = false;
    for ch in input.chars() {
        if escaped {
            if ch != '\n' {
                token.push(ch);
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if !dependencies && ch == ':' {
            dependencies = true;
            token.clear();
        } else if ch == '\n' {
            if dependencies && !token.is_empty() {
                paths.push(PathBuf::from(std::mem::take(&mut token)));
            }
            break;
        } else if dependencies && ch.is_whitespace() {
            if !token.is_empty() {
                paths.push(PathBuf::from(std::mem::take(&mut token)));
            }
        } else {
            token.push(ch);
        }
    }
    paths
}

#[derive(Deserialize)]
struct DiscoveredTests {
    format_version: u32,
    doctests: Vec<DiscoveredTest>,
}

#[derive(Deserialize)]
struct DiscoveredTest {
    file: PathBuf,
    line: usize,
    doctest_attributes: BTreeMap<String, Value>,
    original_code: String,
    name: String,
}

async fn execute_doctests(
    native: &Native<'_>,
    package: &CargoPackage,
    host: &str,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<Vec<RustDoctestOutcome>, AssemblyError> {
    let mut discovery = native.cargo("test", package, host);
    discovery
        .arg("--doc")
        .env("CARGO_ENCODED_RUSTDOCFLAGS", native.doctest_flags(true));
    let discovered: DiscoveredTests = serde_json::from_str(
        &native
            .run(
                discovery,
                DiagnosticCode::RustExtraction,
                "Rust doctest discovery",
                false,
                cancellation,
            )
            .await?,
    )
    .map_err(|_| {
        native.failure(
            DiagnosticCode::RustExtraction,
            "Rustdoc returned invalid structured doctest discovery.",
        )
    })?;
    if discovered.format_version != 2 {
        return Err(native.failure(
            DiagnosticCode::RustToolchain,
            "Unsupported rustdoc doctest discovery format; expected version 2.",
        ));
    }
    let mut execution = native.cargo("test", package, host);
    execution
        .arg("--doc")
        .env("CARGO_ENCODED_RUSTDOCFLAGS", native.doctest_flags(false))
        .args([
            "--",
            "-Zunstable-options",
            "--format=json",
            "--test-threads=1",
        ]);
    let output = native
        .run(
            execution,
            DiagnosticCode::ExecutionCellFailed,
            "Rust doctests",
            true,
            cancellation,
        )
        .await?;
    decode_doctests(native, discovered, &output)
}

fn decode_doctests(
    native: &Native<'_>,
    discovered: DiscoveredTests,
    output: &str,
) -> Result<Vec<RustDoctestOutcome>, AssemblyError> {
    let fail = || {
        native.failure(DiagnosticCode::ExecutionProtocolFailed, "Rust doctest results did not exactly match the native discovery or complete successfully.")
    };
    let mut terminals = BTreeMap::new();
    let mut started = BTreeSet::new();
    let mut suite_started = false;
    let mut suite_finished = false;
    let mut reported_count = 0;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).map_err(|_| fail())?;
        match (event["type"].as_str(), event["event"].as_str()) {
            (Some("suite"), Some("started")) if !suite_started => {
                suite_started = true;
                if event["test_count"].as_u64() != Some(discovered.doctests.len() as u64) {
                    return Err(fail());
                }
            }
            (Some("suite"), Some("ok")) if suite_started && !suite_finished => {
                suite_finished = true;
                if event["failed"].as_u64() != Some(0) || event["filtered_out"].as_u64() != Some(0)
                {
                    return Err(fail());
                }
                reported_count = event["passed"].as_u64().ok_or_else(fail)?
                    + event["ignored"].as_u64().ok_or_else(fail)?;
            }
            (Some("test"), Some("started")) if suite_started && !suite_finished => {
                if !started.insert(event["name"].as_str().ok_or_else(fail)?.to_owned()) {
                    return Err(fail());
                }
            }
            (Some("test"), Some("ok" | "ignored")) if suite_started && !suite_finished => {
                let name = event["name"].as_str().ok_or_else(fail)?.to_owned();
                if !started.remove(&name) || terminals.insert(name, event).is_some() {
                    return Err(fail());
                }
            }
            _ => return Err(fail()),
        }
    }
    if !suite_finished
        || !started.is_empty()
        || reported_count != discovered.doctests.len() as u64
        || terminals.len() != discovered.doctests.len()
    {
        return Err(fail());
    }
    let mut outcomes = Vec::new();
    for test in discovered.doctests {
        let terminal = terminals.remove(&test.name).ok_or_else(fail)?;
        let attrs = &test.doctest_attributes;
        let ignored = terminal["event"] == "ignored";
        let outcome = if ignored {
            RustDoctestStatus::ExplicitSkip
        } else if attrs.get("compile_fail").and_then(Value::as_bool) == Some(true)
            || (attrs.get("should_panic").and_then(Value::as_bool) == Some(true)
                && attrs.get("no_run").and_then(Value::as_bool) != Some(true))
        {
            RustDoctestStatus::ExpectedError
        } else if attrs.get("no_run").and_then(Value::as_bool) == Some(true) {
            RustDoctestStatus::CompileOnly
        } else {
            RustDoctestStatus::Executed
        };
        let absolute = if test.file.is_absolute() {
            test.file.clone()
        } else {
            native.package.path.join(&test.file)
        };
        let source = if absolute.is_file() && absolute.starts_with(&native.repository.path) {
            Some(native.repository.source_location(absolute, None)?)
        } else {
            None
        };
        outcomes.push(RustDoctestOutcome {
            name: native.sanitize(&test.name),
            source,
            line: test.line,
            code: test.original_code,
            attributes: test
                .doctest_attributes
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    )
                })
                .collect(),
            outcome,
            output: terminal
                .get("stdout")
                .and_then(Value::as_str)
                .map(|value| native.sanitize(value))
                .unwrap_or_default(),
            reason: ignored.then(|| "Ignored by the native Rust doctest runner.".into()),
        });
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        directory: tempfile::TempDir,
        repository: ResolvedRepositoryPaths,
        package: ResolvedPackagePaths,
        target: ResolvedTargetPath,
    }

    impl Fixture {
        fn new(documentation: &str) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(directory.path()).unwrap();
            fs::create_dir(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"native_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            fs::write(
                root.join("Cargo.lock"),
                "version = 4\n\n[[package]]\nname = \"native_fixture\"\nversion = \"0.1.0\"\n",
            )
            .unwrap();
            fs::write(
                root.join("src/lib.rs"),
                format!("{documentation}\npub fn example() {{}}\n"),
            )
            .unwrap();
            let target = ResolvedTargetPath {
                id: "library".into(),
                path: root.join("src/lib.rs"),
            };
            let package = ResolvedPackagePaths {
                id: "native-fixture".into(),
                repository_index: 0,
                path: root.clone(),
                metadata_path: root.join("Cargo.toml"),
                targets: vec![target.clone()],
            };
            let repository = ResolvedRepositoryPaths {
                id: "fixture".into(),
                path: root,
            };
            Self {
                directory,
                repository,
                package,
                target,
            }
        }
    }

    #[test]
    fn dependency_info_decodes_escaped_source_paths() {
        assert_eq!(
            dependency_paths(
                "/tmp/output.d: src/lib.rs src/with\\ space.rs \\\n src/continued.rs\n\nsrc/lib.rs:\n"
            ),
            vec![
                PathBuf::from("src/lib.rs"),
                PathBuf::from("src/with space.rs"),
                PathBuf::from("src/continued.rs")
            ]
        );
    }

    #[test]
    fn inventory_observes_membership_and_skips_generated_caches() {
        let fixture = Fixture::new("");
        let cache = fixture.directory.path().join("generated");
        fs::create_dir(&cache).unwrap();
        let before = inventory(
            &fixture.repository,
            std::slice::from_ref(&fixture.package.path),
            &BTreeSet::new(),
            &cache,
        )
        .unwrap();
        fs::write(cache.join("output"), "generated").unwrap();
        assert_eq!(
            inventory(
                &fixture.repository,
                std::slice::from_ref(&fixture.package.path),
                &BTreeSet::new(),
                &cache
            )
            .unwrap(),
            before
        );
        fs::write(fixture.package.path.join("src/doctest_only.rs"), "new").unwrap();
        assert_ne!(
            inventory(
                &fixture.repository,
                std::slice::from_ref(&fixture.package.path),
                &BTreeSet::new(),
                &cache
            )
            .unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_creation_rejects_source_directory_aliases_without_writes() {
        let fixture = Fixture::new("");
        let alias = fixture.directory.path().join(".diplodocus");
        std::os::unix::fs::symlink(fixture.package.path.join("src"), &alias).unwrap();
        assert!(prepare_build_directory(&alias.join("cache/rust")).is_err());
        assert!(!fixture.package.path.join("src/cache").exists());
        let build = fixture.directory.path().join("generated-cache");
        prepare_build_directory(&build).unwrap();
        std::os::unix::fs::symlink(&fixture.target.path, build.join("rustdoc-inputs.d")).unwrap();
        assert!(prepare_build_directory(&build).is_err());
        assert!(
            fs::read_to_string(&fixture.target.path)
                .unwrap()
                .contains("pub fn example()")
        );
    }

    #[cfg(unix)]
    #[test]
    fn inventory_rejects_paths_that_leave_and_reenter_the_repository() {
        let fixture = Fixture::new("");
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&fixture.repository.path, outside.path().join("return"))
            .unwrap();
        std::os::unix::fs::symlink(outside.path(), fixture.repository.path.join("escape")).unwrap();
        let roots = vec![fixture.repository.path.join("escape/return/src")];
        assert!(
            inventory(
                &fixture.repository,
                &roots,
                &BTreeSet::new(),
                &fixture.repository.path.join("generated-cache")
            )
            .is_err()
        );
    }

    #[test]
    fn dependency_documentation_uses_unambiguous_cargo_package_versions() {
        let package = |name: &str, version: &str, target: &str| CargoPackage {
            id: format!("{name}@{version}"),
            name: name.into(),
            version: version.into(),
            manifest_path: PathBuf::from("/unused/Cargo.toml"),
            source: Some("registry".into()),
            targets: vec![CargoTarget {
                name: target.into(),
                kind: vec!["lib".into()],
                src_path: PathBuf::from("/unused/src/lib.rs"),
            }],
        };
        let metadata = Metadata {
            packages: vec![
                package("hyphenated-package", "1.2.3", "rust_library"),
                package("ambiguous", "1.0.0", "ambiguous"),
                package("ambiguous", "2.0.0", "ambiguous"),
            ],
            workspace_root: PathBuf::from("/unused"),
            resolve: None,
        };
        let input = serde_json::json!({ "external_crates": {
            "1": { "name": "rust_library", "html_root_url": null },
            "2": { "name": "rust_library", "html_root_url": "https://example.com/custom/" },
            "3": { "name": "ambiguous", "html_root_url": null },
            "4": { "name": "rust_library", "html_root_url": "https://docs.rs/hyphenated-package/0.1/" },
        }});
        let output: Value = serde_json::from_slice(
            &dependency_documentation_urls(&metadata, &serde_json::to_vec(&input).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            output["external_crates"]["1"]["html_root_url"],
            "https://docs.rs/hyphenated-package/1.2.3/rust_library/"
        );
        assert_eq!(
            output["external_crates"]["2"]["html_root_url"],
            "https://example.com/custom/"
        );
        assert!(output["external_crates"]["3"]["html_root_url"].is_null());
        assert_eq!(
            output["external_crates"]["4"]["html_root_url"],
            "https://docs.rs/hyphenated-package/1.2.3/rust_library/"
        );
    }

    #[tokio::test]
    async fn compiler_and_native_runner_preserve_rust_doctest_semantics() {
        if std::env::var_os("DIPLODOCUS_RUST_TOOLCHAIN").is_none() {
            return;
        }
        let fixture = Fixture::new(
            r#"//! Crate documentation.
//! ```
//! # let hidden = 1;
//! assert_eq!(hidden, 1);
//! ```
//! ```no_run
//! loop {}
//! ```
//! ```compile_fail
//! does_not_exist();
//! ```
//! ```ignore
//! missing_external_service();
//! ```
//! ```should_panic
//! panic!("expected");
//! ```
"#,
        );
        let cache = tempfile::tempdir().unwrap();
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
        let result = extract_target(
            &fixture.repository,
            &fixture.package,
            &fixture.target,
            &RustTargetConfiguration::default(),
            cache.path(),
            true,
            ExecutionDeadlines::default(),
            &mut cancellation,
        )
        .await
        .unwrap();
        assert!(!result.extraction.items.is_empty());
        assert!(
            result
                .provenance
                .tools
                .get("cargo")
                .is_some_and(|version| version.starts_with("cargo "))
        );
        let ProvenanceActivity::Extraction { parsers, .. } = &result.provenance.activity else {
            panic!("compiler extraction provenance")
        };
        assert_eq!(parsers["rustdoc-types"].version, "0.61.0");
        let ProvenanceActivity::RustDoctests {
            examples, inputs, ..
        } = result.doctests.unwrap().activity
        else {
            panic!("doctest provenance")
        };
        assert_eq!(examples.len(), 5);
        assert_eq!(
            examples
                .iter()
                .filter(|example| example.outcome == RustDoctestStatus::Executed)
                .count(),
            1
        );
        assert!(
            examples
                .iter()
                .any(|example| example.outcome == RustDoctestStatus::CompileOnly)
        );
        assert!(
            examples
                .iter()
                .any(|example| example.outcome == RustDoctestStatus::ExpectedError)
        );
        assert!(
            examples
                .iter()
                .any(|example| example.outcome == RustDoctestStatus::ExplicitSkip)
        );
        assert!(
            examples
                .iter()
                .all(|example| example.output.is_empty() && example.source.is_some())
        );
        assert!(
            inputs
                .iter()
                .any(|input| input.source.path.as_str() == "Cargo.lock")
        );
    }

    #[tokio::test]
    async fn native_doctests_preserve_project_rustdoc_configuration() {
        if std::env::var_os("DIPLODOCUS_RUST_TOOLCHAIN").is_none() {
            return;
        }
        let fixture = Fixture::new(
            "#[cfg(documentation_fixture)]\n#[doc = \"```\\nassert!(true);\\n```\"]\npub fn configured() {}",
        );
        fs::create_dir(fixture.package.path.join(".cargo")).unwrap();
        fs::write(
            fixture.package.path.join(".cargo/config.toml"),
            "[build]\nrustdocflags = [\"--cfg\", \"documentation_fixture\"]\n",
        )
        .unwrap();
        let cache = tempfile::tempdir().unwrap();
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
        let result = extract_target(
            &fixture.repository,
            &fixture.package,
            &fixture.target,
            &RustTargetConfiguration::default(),
            cache.path(),
            true,
            ExecutionDeadlines::default(),
            &mut cancellation,
        )
        .await
        .unwrap();
        let ProvenanceActivity::RustDoctests { examples, .. } = result.doctests.unwrap().activity
        else {
            panic!("doctest provenance")
        };
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].outcome, RustDoctestStatus::Executed);
    }

    #[tokio::test]
    async fn native_doctest_timeout_kills_the_process_group() {
        if std::env::var_os("DIPLODOCUS_RUST_TOOLCHAIN").is_none() {
            return;
        }
        let fixture = Fixture::new(
            "//! ```\n//! loop { std::thread::sleep(std::time::Duration::from_secs(1)); }\n//! ```",
        );
        let cache = tempfile::tempdir().unwrap();
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
        let deadlines = ExecutionDeadlines {
            cell: 500,
            ..ExecutionDeadlines::default()
        };
        let result = extract_target(
            &fixture.repository,
            &fixture.package,
            &fixture.target,
            &RustTargetConfiguration::default(),
            cache.path(),
            true,
            deadlines,
            &mut cancellation,
        )
        .await;
        let Err(AssemblyError::Diagnostics(diagnostics)) = result else {
            panic!("expected supervised timeout")
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("individual doctest"))
        );
    }

    #[tokio::test]
    async fn cancellation_stops_native_preparation() {
        if std::env::var_os("DIPLODOCUS_RUST_TOOLCHAIN").is_none() {
            return;
        }
        let fixture = Fixture::new("");
        let cache = tempfile::tempdir().unwrap();
        let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::ready(()));
        let result = extract_target(
            &fixture.repository,
            &fixture.package,
            &fixture.target,
            &RustTargetConfiguration::default(),
            cache.path(),
            false,
            ExecutionDeadlines::default(),
            &mut cancellation,
        )
        .await;
        assert!(matches!(result, Err(AssemblyError::Cancelled)));
    }
}
