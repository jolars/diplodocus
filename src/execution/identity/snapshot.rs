use std::collections::BTreeMap;
use std::path::PathBuf;

use super::local::{
    canonical, digest, exact_version, fingerprint, normalize_language, read_declared, roots,
};
use super::{
    BuildObservation, CanonicalValue, IdentityError, LaunchIdentityInput, RepositoryFile,
    content_digest, domain_digest,
};
use crate::configuration::{
    ContentConfiguration, ExecutionConfiguration, ExecutionEngine, ExecutionMode,
};
use crate::documents::{AuthoredFormat, QmdPreparation, prepare_collection_document};
use crate::execution::{
    EffectiveCellOptions, ExecutionDeadlines, ExecutionDefaults, ExecutionFailure,
    ExecutionFailureKind, ExecutionPolicies, OutputVisibility, PageExecutionRequest,
};
use crate::ir::{InputFingerprint, SourceLocation};
use serde_json::{Value, json};

/// Explicit local input facts supplied by the engine adapter.
#[derive(Clone)]
pub struct IdentityInputs {
    /// Declared canonical repository roots.
    pub repositories: BTreeMap<String, PathBuf>,
    /// The authored source declaration.
    pub page: RepositoryFile,
    /// Explicit declarations, not cached digest assertions.
    pub declared_files: Vec<RepositoryFile>,
    /// Running engine and build observations.
    pub build: BuildObservation,
    /// The same resolved facts that will be used for spawning.
    pub launch: LaunchIdentityInput,
}
impl std::fmt::Debug for IdentityInputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdentityInputs { <private> }")
    }
}
/// Exact validated kernel-info fields observed from the current live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeObservation {
    /// Kernel implementation name.
    pub implementation: String,
    /// Exact implementation version.
    pub implementation_version: String,
    /// Runtime language, normalized only by the documented aliases.
    pub language: String,
    /// Exact language version.
    pub language_version: String,
    /// Exact protocol version, including the minor version.
    pub protocol_version: String,
}
/// Immutable local capture of prepared source and declared input bytes.
///
/// ```compile_fail
/// use diplodocus::execution::identity::InputSnapshot;
/// fn serialize<T: serde::Serialize>() {}
/// serialize::<InputSnapshot>();
/// ```
pub struct InputSnapshot {
    inputs: IdentityInputs,
    request: PageExecutionRequest,
    source: Vec<u8>,
    files: Vec<(RepositoryFile, Vec<u8>)>,
    environment: Vec<InputFingerprint>,
}
impl std::fmt::Debug for InputSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InputSnapshot { <private> }")
    }
}
/// Immutable key constructed from a bound snapshot and current observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionIdentity {
    value: CanonicalValue,
    key: String,
}
impl ExecutionIdentity {
    /// Portable exact key input, shared by cache and provenance projections.
    pub fn key_input(&self) -> &CanonicalValue {
        &self.value
    }
    /// Domain-separated page key.
    pub fn key(&self) -> &str {
        &self.key
    }
}
fn failure(request: &PageExecutionRequest) -> ExecutionFailure {
    let kind = ExecutionFailureKind::InputChanged;
    ExecutionFailure {
        kind,
        diagnostics: vec![
            kind.to_diagnostic(&request.page.collection, request.page.source.clone()),
        ],
        cleanup_diagnostics: vec![],
    }
}
/// Capture and bind source and declared inputs before any kernel launch.
/// Caller digest assertions are checked against freshly read declared bytes.
pub async fn snapshot_inputs(
    inputs: IdentityInputs,
    request: &PageExecutionRequest,
    prepared_source: &[u8],
) -> Result<InputSnapshot, ExecutionFailure> {
    capture(inputs, request, prepared_source)
        .await
        .map_err(|_| failure(request))
}
async fn capture(
    inputs: IdentityInputs,
    request: &PageExecutionRequest,
    prepared_source: &[u8],
) -> Result<InputSnapshot, IdentityError> {
    roots(&inputs.repositories).await?;
    validate_build(&inputs.build)?;
    if inputs.launch.resolver().repositories != inputs.repositories
        || inputs.page.repository != request.page.source.repository
        || inputs.page.path != request.page.source.path
        || inputs.launch.resolver().repository != inputs.page.repository
        || inputs.launch.selector() != request.kernel.to_ascii_lowercase()
    {
        return Err(IdentityError);
    }
    let parent = std::path::Path::new(inputs.page.path.as_str())
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if inputs.launch.resolver().working_directory != parent
        || request
            .page
            .working_directory
            .as_ref()
            .map_or(".", |p| p.as_str())
            != parent
    {
        return Err(IdentityError);
    }
    let source = read_declared(&inputs.repositories, &inputs.page).await?;
    if source != prepared_source
        || digest(&request.page.source_fingerprint)? != content_digest(&source)
    {
        return Err(IdentityError);
    }
    validate_prepared(request, &source)?;
    inputs.launch.revalidate().await?;
    let mut declarations = inputs.declared_files.clone();
    declarations.sort();
    if declarations.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(IdentityError);
    }
    let mut files = Vec::new();
    let mut environment = Vec::new();
    for declaration in declarations {
        let bytes = read_declared(&inputs.repositories, &declaration).await?;
        environment.push(InputFingerprint {
            source: SourceLocation {
                repository: declaration.repository.clone(),
                path: declaration.path.clone(),
                span: None,
            },
            fingerprint: fingerprint(&content_digest(&bytes))?,
        });
        files.push((declaration, bytes));
    }
    let mut request = request.clone();
    sort_environment(&mut request.declared_environment_inputs);
    if request.declared_environment_inputs != environment {
        return Err(IdentityError);
    }
    Ok(InputSnapshot {
        inputs,
        request,
        source,
        files,
        environment,
    })
}
fn sort_environment(environment: &mut [InputFingerprint]) {
    environment.sort_by(|a, b| {
        (&a.source.repository, &a.source.path).cmp(&(&b.source.repository, &b.source.path))
    });
}
pub(crate) fn validate_prepared(
    request: &PageExecutionRequest,
    source: &[u8],
) -> Result<QmdPreparation, IdentityError> {
    if request.page.format != AuthoredFormat::Qmd
        || request.page.mode != ExecutionMode::Execute
        || request.page.page_veto
        || request.page.source.span.is_some()
        || request.page.collection.is_empty()
        || request.page.qmd_policy != "qmd-mvp-v1"
        || request.page.parser_version != crate::provenance::PANACHE_VERSION
    {
        return Err(IdentityError);
    }
    let collection = ContentConfiguration {
        id: request.page.collection.clone(),
        owner: "project".into(),
        repository: request.page.source.repository.clone(),
        path: ".".into(),
        mount: String::new(),
        format: AuthoredFormat::Qmd,
        execution: ExecutionConfiguration {
            mode: ExecutionMode::Execute,
            engine: Some(ExecutionEngine::Jupyter),
            kernel: Some(request.kernel.clone()),
            declared_environment_inputs: Vec::new(),
        },
    };
    let prepared = prepare_collection_document(
        std::str::from_utf8(source).map_err(|_| IdentityError)?,
        &collection,
    )
    .map_err(|_| IdentityError)?
    .preparation
    .ok_or(IdentityError)?;
    if !prepared.execution_eligible
        || prepared.page_veto != request.page.page_veto
        || prepared.defaults != request.defaults
        || prepared.cells != request.cells
    {
        return Err(IdentityError);
    }
    Ok(prepared)
}
fn validate_build(build: &BuildObservation) -> Result<(), IdentityError> {
    digest(&build.executable_digest)?;
    let roles = [
        "async-runtime",
        "authored-parser",
        "fragment-parser",
        "html-parser",
        "html-sanitizer",
        "jupyter-protocol",
        "jupyter-transport",
        "raster-decoder",
        "raster-validator",
        "svg-parser",
        "svg-validator",
    ];
    if roles.iter().any(|r| !build.components.contains_key(*r))
        || !exact_version(&build.engine_version)
        || build.components.iter().any(|(role, component)| {
            role.is_empty() || component.name.is_empty() || !exact_version(&component.version)
        })
        || build.platform.os != "linux"
        || build.platform.architecture.is_empty()
        || build.platform.target.is_empty()
    {
        return Err(IdentityError);
    }
    Ok(())
}
impl InputSnapshot {
    /// Complete identity after readiness with the current kernel information.
    pub fn identity(
        &self,
        runtime: &RuntimeObservation,
        request: &PageExecutionRequest,
        deadlines: &ExecutionDeadlines,
    ) -> Result<ExecutionIdentity, ExecutionFailure> {
        self.make_identity(runtime, request, deadlines)
            .map_err(|_| failure(request))
    }
    fn make_identity(
        &self,
        runtime: &RuntimeObservation,
        request: &PageExecutionRequest,
        deadlines: &ExecutionDeadlines,
    ) -> Result<ExecutionIdentity, IdentityError> {
        let mut request = request.clone();
        sort_environment(&mut request.declared_environment_inputs);
        if request != self.request {
            return Err(IdentityError);
        }
        if [
            &runtime.implementation,
            &runtime.implementation_version,
            &runtime.language,
            &runtime.language_version,
            &runtime.protocol_version,
        ]
        .iter()
        .any(|s| s.trim().is_empty() || s.contains('\0'))
            || normalize_language(&runtime.language) != self.inputs.launch.language()
        {
            return Err(IdentityError);
        }
        let protocol: Vec<_> = runtime.protocol_version.split('.').collect();
        if protocol.len() != 2 || protocol[0] != "5" || protocol[1].parse::<u64>().is_err() {
            return Err(IdentityError);
        }
        let limits = [
            deadlines.startup,
            deadlines.cell,
            deadlines.terminal_sync,
            deadlines.interrupt,
            deadlines.shutdown,
            deadlines.termination,
            deadlines.forced_exit,
        ];
        if limits.contains(&0) {
            return Err(IdentityError);
        }
        let cells:Vec<_>=request.cells.iter().map(|cell| {
            let language=cell.cell.language.as_ref().map(|s|normalize_language(s));
            let eligible=language.as_deref()==Some(self.inputs.launch.language()) && cell.options.execution.eval.value;
            json!({"ordinal":cell.ordinal,"language":language,"eligible":eligible,"submitted_source_digest":eligible.then(||content_digest(cell.cell.source.as_bytes())),"effective":effective(&cell.options)})
        }).collect();
        if !cells.iter().any(|c| c["eligible"] == true) {
            return Err(IdentityError);
        }
        let build = &self.inputs.build;
        let components: Vec<_> = build
            .components
            .iter()
            .map(|(role, c)| json!({"role":role,"name":c.name,"version":c.version}))
            .collect();
        let environment:Vec<_>=self.environment.iter().map(|e|Ok(json!({"repository":e.source.repository,"path":e.source.path,"digest":digest(&e.fingerprint)?}))).collect::<Result<_,IdentityError>>()?;
        let policies = ExecutionPolicies::default();
        let value = canonical(json!({
            "schema":"page-execution-key-v1","schemas":{"encoding":"execution-json-v1","artifact":"page-execution-artifact-v1","ir":"execution-result-v1"},
            "page":{"repository":request.page.source.repository,"collection":request.page.collection,"path":request.page.source.path,"working_directory":request.page.working_directory.as_ref().map_or(".",|p|p.as_str()),"format":"qmd","source_digest":digest(&request.page.source_fingerprint)?},
            "options":{"mode":"execute","page_veto":false,"defaults":defaults(&request.defaults),"cells":cells},
            "engine":{"id":"jupyter","version":build.engine_version,"build_digest":digest(&build.executable_digest)?},
            "policies":{"qmd":request.page.qmd_policy,"execution":policies.execution,"mime":policies.mime,"html":policies.html,"svg":policies.svg},
            "components":components,"platform":build.platform,"deadlines_ms":deadlines,"environment_inputs":environment,
            "kernel":{"name":self.inputs.launch.selector(),"spec_digest":self.inputs.launch.spec_digest(),"launch_digest":self.inputs.launch.launch_digest(),"search":self.inputs.launch.search(),"interrupt_mode":self.inputs.launch.interrupt_mode(),"runtime":{"implementation":runtime.implementation,"implementation_version":runtime.implementation_version,"language":normalize_language(&runtime.language),"language_version":runtime.language_version,"protocol_version":runtime.protocol_version}}
        }))?;
        let key = domain_digest("diplodocus/page-execution-key-v1", &value)?;
        Ok(ExecutionIdentity { value, key })
    }
    /// Read-only observed environment evidence for the final prepared request.
    pub fn environment_inputs(&self) -> &[InputFingerprint] {
        &self.environment
    }
    /// Revalidate after cleanup before accepting either execution or restoration.
    pub async fn revalidate(&self, launch: &LaunchIdentityInput) -> Result<(), ExecutionFailure> {
        self.recheck(launch)
            .await
            .map_err(|_| failure(&self.request))
    }
    async fn recheck(&self, launch: &LaunchIdentityInput) -> Result<(), IdentityError> {
        roots(&self.inputs.repositories).await?;
        if read_declared(&self.inputs.repositories, &self.inputs.page).await? != self.source {
            return Err(IdentityError);
        }
        for (file, bytes) in &self.files {
            if read_declared(&self.inputs.repositories, file).await? != *bytes {
                return Err(IdentityError);
            }
        }
        self.inputs.launch.revalidate().await?;
        launch.revalidate().await?;
        if launch.resolver().repositories != self.inputs.repositories
            || !self.inputs.launch.same_observation(launch)
        {
            return Err(IdentityError);
        }
        Ok(())
    }
}
fn defaults(options: &ExecutionDefaults) -> Value {
    json!({"eval":options.eval.value,"echo":options.echo.value,"output":match options.output.value {OutputVisibility::Show=>json!(true),OutputVisibility::Hide=>json!(false),OutputVisibility::AsIs=>json!("asis")},"include":options.include.value,"error":options.error.value})
}
fn effective(options: &EffectiveCellOptions) -> Value {
    let mut value = defaults(&options.execution);
    let object = value.as_object_mut().unwrap();
    object.insert("label".into(), json!(options.label.value));
    object.insert("fig-alt".into(), json!(options.fig_alt.value));
    object.insert("fig-cap".into(), json!(options.fig_cap.value));
    object.insert("fig-subcap".into(), json!(options.fig_subcap.value));
    value
}
