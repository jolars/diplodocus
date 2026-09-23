//! Compose one authorized page from current inputs through supervised cleanup.

use std::collections::{BTreeMap, BTreeSet};
use std::future::ready;
use std::path::{Path, PathBuf};

use super::FailureSource;
use super::discovery::{SearchEnvironment, discover_kernel};
use super::launch::{ResolvedKernel, with_discovery};
use super::output::{ErrorContext, OutputReducer, ReducedPage, images::validate_with_assets};
use super::page::skip_reason;
use super::session::{KernelRuntime, start_resolved_session};
use crate::configuration::{ExecutionEngine as EngineKind, ExecutionMode};
use crate::diagnostics::{Diagnostic, DiagnosticSource};
use crate::documents::AuthoredFormat;
use crate::execution::assets::PageAssetStore;
use crate::execution::identity::{
    BuildObservation, IdentityInputs, LaunchIdentityInput, RepositoryFile, RuntimeObservation,
    snapshot_inputs, validate_prepared,
};
use crate::execution::output_safety::AuthoredOutputContext;
use crate::execution::validated::DiagnosticEvidence;
use crate::execution::{
    CellOutcome, ExecutionCapabilities, ExecutionContext, ExecutionDeadlines, ExecutionEngine,
    ExecutionFailure, ExecutionFailureKind, ExecutionFeature, ExecutionFuture, ExecutionPolicies,
    ExecutionRequirements, KernelExecutionProvenance, KernelProtocolRequirement,
    PageExecutionProvenance, PageExecutionRecord, PageExecutionRequest, PageExecutionResult,
    PreparedExecution, ValidatedPage,
};
use crate::ir::{ExecutionOrigin, Fingerprint, KernelProvenance};
use crate::provenance::ExecutionObservation;

/// Linux Jupyter execution for one configured collection's declared inputs.
///
/// Construction performs no I/O. Repository roots and file declarations come from
/// the current configuration, independently of a request's fingerprint claims.
/// The caller still authorizes the current command before dispatching a page.
pub struct JupyterEngine {
    repositories: BTreeMap<String, PathBuf>,
    declared_files: Vec<RepositoryFile>,
    environment: Option<SearchEnvironment>,
}

impl std::fmt::Debug for JupyterEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JupyterEngine").finish_non_exhaustive()
    }
}

impl JupyterEngine {
    /// Supply canonical configured roots and this collection's explicit input files.
    pub fn new(
        repositories: BTreeMap<String, PathBuf>,
        declared_files: Vec<RepositoryFile>,
    ) -> Self {
        Self {
            repositories,
            declared_files,
            environment: None,
        }
    }

    #[cfg(test)]
    pub(in crate::execution::jupyter) fn with_search_environment(
        mut self,
        environment: SearchEnvironment,
    ) -> Self {
        self.environment = Some(environment);
        self
    }

    async fn execute(
        &self,
        mut context: ExecutionContext<'_>,
        request: &PageExecutionRequest,
    ) -> Result<PageExecutionResult, ExecutionFailure> {
        let source = FailureSource {
            collection: request.page.collection.clone(),
            source: request.page.source.clone(),
        };
        if request.page.mode != ExecutionMode::Execute
            || request.page.format != AuthoredFormat::Qmd
            || request.page.page_veto
            || !request
                .cells
                .iter()
                .any(|cell| cell.options.execution.eval.value)
        {
            return Err(source.failure(
                ExecutionFailureKind::Startup,
                "The page has no execution authority or candidates.",
            ));
        }
        let preparing = self.prepare(
            &context.repository_root,
            &context.page_path,
            request,
            &source,
        );
        let prepared = tokio::select! {
            biased;
            _ = &mut context.cancellation => return Err(cancelled(&source)),
            result = preparing => result?,
        };
        let captured;
        let environment = if let Some(environment) = &self.environment {
            environment
        } else {
            captured = SearchEnvironment::capture(&source)?;
            &captured
        };
        let kernel = tokio::select! {
            biased;
            _ = &mut context.cancellation => return Err(cancelled(&source)),
            result = discover_kernel(&request.kernel, environment, &source) => result?,
        };
        let before = kernel.diagnostics.clone();
        let mut reducer = OutputReducer::with_context(
            request.page.clone(),
            ErrorContext::new(context.repository_root.clone()),
            prepared.context().clone(),
        );
        let mut assets = PageAssetStore::new(
            request.page.clone(),
            context.repository_root.clone(),
            context.asset_staging_directory.clone(),
        )
        .map_err(|error| {
            with_discovery(
                source.failure(
                    error
                        .failure_kind()
                        .unwrap_or(ExecutionFailureKind::OutputValidation),
                    &error.to_string(),
                ),
                &before,
            )
        })?;
        let result = async {
            if request
                .cells
                .iter()
                .all(|cell| skip_reason(cell, &kernel.language).is_some())
            {
                for cell in &request.cells {
                    reducer
                        .accept_cell(
                            cell,
                            CellOutcome::Skipped {
                                reason: skip_reason(cell, &kernel.language)
                                    .expect("all cells skipped"),
                            },
                            vec![],
                            &mut |candidate| validate_with_assets(candidate, &mut assets),
                        )
                        .map_err(|failure| with_discovery(failure, &before))?;
                }
                let reduced = reducer
                    .finish()
                    .map_err(|failure| with_discovery(failure, &before))?;
                let reading = self.read_source(
                    &context.repository_root, &context.page_path, request, &source,
                );
                let current = tokio::select! {
                    biased;
                    _ = &mut context.cancellation => return Err(with_discovery(cancelled(&source), &before)),
                    result = reading => result.map_err(|failure| with_discovery(failure, &before))?,
                };
                if current != prepared.source() {
                    return Err(with_discovery(changed(&source), &before));
                }
                return finalize(&prepared, reduced, before, None, &mut assets, &source);
            }
            let resolving = ResolvedKernel::resolve(
                kernel,
                self.repositories.clone(),
                &context.repository_root,
                &context.page_path,
                &source,
            );
            let resolved = tokio::select! {
                biased;
                _ = &mut context.cancellation => return Err(with_discovery(cancelled(&source), &before)),
                result = resolving => result?,
            };
            let launch = resolved.identity().clone();
            let observing = async {
                let build = BuildObservation::observe()
                    .await
                    .map_err(|_| changed(&source))?;
                let snapshot = snapshot_inputs(
                    IdentityInputs {
                        repositories: self.repositories.clone(),
                        page: RepositoryFile {
                            repository: request.page.source.repository.clone(),
                            path: request.page.source.path.clone(),
                        },
                        declared_files: self.declared_files.clone(),
                        build: build.clone(),
                        launch: launch.clone(),
                    },
                    request,
                    prepared.source(),
                )
                .await?;
                Ok::<_, ExecutionFailure>((build, snapshot))
            };
            let (build, snapshot) = tokio::select! {
                biased;
                _ = &mut context.cancellation => return Err(with_discovery(cancelled(&source), &before)),
                result = observing => result.map_err(|failure| with_discovery(failure, &before))?,
            };
            let session =
                start_resolved_session(resolved, &mut context, source.clone()).await?;
            let runtime = session.runtime.clone();
            let observed = RuntimeObservation {
                implementation: runtime.implementation.clone(),
                implementation_version: runtime.implementation_version.clone(),
                language: runtime.language.clone(),
                language_version: runtime.language_version.clone(),
                protocol_version: runtime.protocol_version.clone(),
            };
            if let Err(mut failure) = snapshot.identity(&observed, request, &context.deadlines) {
                if let Err(cleanup) = session.shutdown().await {
                    failure.cleanup_diagnostics.extend(cleanup.diagnostics);
                    failure
                        .cleanup_diagnostics
                        .extend(cleanup.cleanup_diagnostics);
                }
                return Err(with_discovery(failure, &before));
            }
            let execution = session
                .execute_detailed(
                    request.cells.clone(),
                    &mut context.cancellation,
                    |mut completed| {
                        let result = reducer.accept_cell(
                            &request.cells[completed.ordinal],
                            completed.outcome,
                            std::mem::take(&mut completed.events),
                            &mut |candidate| validate_with_assets(candidate, &mut assets),
                        );
                        ready(result.map(|()| completed))
                    },
                )
                .await;
            if let Err(failure) = execution {
                return Err(failure.with_reduction(&reducer));
            }
            let reduced = reducer
                .finish()
                .map_err(|failure| with_discovery(failure, &before))?;
            let warnings: Vec<_> = before
                .iter()
                .cloned()
                .chain(reduced.diagnostics.iter().cloned())
                .collect();
            tokio::select! {
                biased;
                _ = &mut context.cancellation => return Err(with_discovery(cancelled(&source), &warnings)),
                result = snapshot.revalidate(&launch) => result.map_err(|failure| with_discovery(failure, &warnings))?,
            }
            let provenance = provenance(
                &build,
                &launch,
                &runtime,
                snapshot.environment_inputs().to_vec(),
                context.deadlines,
                &source,
            );
            finalize(
                &prepared, reduced, before, Some(provenance), &mut assets, &source,
            )
        }
        .await;
        match result {
            Ok(validated) => {
                let warnings = validated.record().diagnostics.clone();
                PageExecutionResult::retain(validated, assets)
                    .map_err(|failure| with_discovery(failure, &warnings))
            }
            Err(mut failure) => {
                if let Err(error) = assets.rollback() {
                    failure.cleanup_diagnostics.extend(
                        source
                            .failure(ExecutionFailureKind::Cleanup, &error.to_string())
                            .diagnostics,
                    );
                }
                Err(failure)
            }
        }
    }

    async fn prepare(
        &self,
        root: &Path,
        page: &Path,
        request: &PageExecutionRequest,
        source: &FailureSource,
    ) -> Result<PreparedExecution, ExecutionFailure> {
        let bytes = self.read_source(root, page, request, source).await?;
        let preparation = validate_prepared(request, &bytes).map_err(|_| changed(source))?;
        let authored = AuthoredOutputContext::new(
            request.page.source.clone(),
            request.page.collection.clone(),
            preparation.authored_anchors,
        );
        PreparedExecution::checked(request.clone(), bytes, authored).map_err(|_| changed(source))
    }

    async fn read_source(
        &self,
        root: &Path,
        page: &Path,
        request: &PageExecutionRequest,
        source: &FailureSource,
    ) -> Result<Vec<u8>, ExecutionFailure> {
        if self
            .repositories
            .get(&request.page.source.repository)
            .map(PathBuf::as_path)
            != Some(root)
            || tokio::fs::canonicalize(root)
                .await
                .map_err(|_| changed(source))?
                != root
        {
            return Err(changed(source));
        }
        let resolved = tokio::fs::canonicalize(root.join(request.page.source.path.as_str()))
            .await
            .map_err(|_| changed(source))?;
        if resolved != page
            || !resolved.starts_with(root)
            || !tokio::fs::metadata(page)
                .await
                .map_err(|_| changed(source))?
                .is_file()
        {
            return Err(changed(source));
        }
        tokio::fs::read(page).await.map_err(|_| changed(source))
    }
}

impl ExecutionEngine for JupyterEngine {
    fn capabilities(&self) -> ExecutionCapabilities {
        ExecutionCapabilities {
            languages: BTreeSet::from(["python".into(), "r".into()]),
            media_types: [
                "image/svg+xml",
                "image/png",
                "image/jpeg",
                "text/markdown",
                "text/html",
                "text/plain",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            features: BTreeSet::from([
                ExecutionFeature::PageSession,
                ExecutionFeature::Streams,
                ExecutionFeature::RichOutput,
                ExecutionFeature::DisplayUpdates,
                ExecutionFeature::ClearOutput,
                ExecutionFeature::AllowedErrors,
                ExecutionFeature::Interruption,
            ]),
        }
    }

    fn requirements(&self) -> ExecutionRequirements {
        ExecutionRequirements {
            operating_systems: BTreeSet::from(["linux".into()]),
            protocol: KernelProtocolRequirement {
                name: "jupyter".into(),
                major: 5,
            },
        }
    }

    fn execute_page<'a>(
        &'a self,
        context: ExecutionContext<'a>,
        request: &'a PageExecutionRequest,
    ) -> ExecutionFuture<'a> {
        Box::pin(self.execute(context, request))
    }
}

fn cancelled(source: &FailureSource) -> ExecutionFailure {
    source.failure(
        ExecutionFailureKind::Cancelled,
        "Page execution was canceled.",
    )
}
fn changed(source: &FailureSource) -> ExecutionFailure {
    source.failure(
        ExecutionFailureKind::InputChanged,
        "The prepared execution inputs are invalid or changed.",
    )
}

fn finalize(
    prepared: &PreparedExecution,
    mut reduced: ReducedPage,
    before: Vec<Diagnostic>,
    provenance: Option<PageExecutionProvenance>,
    assets: &mut PageAssetStore,
    source: &FailureSource,
) -> Result<ValidatedPage, ExecutionFailure> {
    for cell in &mut reduced.cells {
        for output in &mut cell.outputs {
            for index in &mut output.diagnostic_indices {
                *index += before.len();
            }
        }
    }
    let diagnostics: Vec<_> = before.iter().cloned().chain(reduced.diagnostics).collect();
    let record = PageExecutionRecord {
        page: prepared.request().page.clone(),
        defaults: prepared.request().defaults.clone(),
        cells: reduced.cells,
        diagnostics: diagnostics.clone(),
        assets: reduced.assets,
        provenance,
    };
    ValidatedPage::checked(
        prepared,
        record,
        reduced.slots,
        DiagnosticEvidence {
            before,
            execution: reduced.execution_diagnostics,
            after: vec![],
        },
        assets,
    )
    .map_err(|_| {
        with_discovery(
            source.failure(
                ExecutionFailureKind::OutputValidation,
                "The complete execution result failed validation.",
            ),
            &diagnostics,
        )
    })
}

fn provenance(
    build: &BuildObservation,
    launch: &LaunchIdentityInput,
    runtime: &KernelRuntime,
    inputs: Vec<crate::ir::InputFingerprint>,
    deadlines: ExecutionDeadlines,
    source: &FailureSource,
) -> PageExecutionProvenance {
    let fingerprint = |digest: &str| Fingerprint {
        algorithm: "sha256".into(),
        value: digest
            .strip_prefix("sha256:")
            .expect("validated identity digest")
            .into(),
    };
    PageExecutionProvenance {
        execution: ExecutionObservation {
            engine: EngineKind::Jupyter,
            kernel: KernelProvenance {
                name: launch.selector(),
                language: Some(runtime.language.clone()),
                language_version: Some(runtime.language_version.clone()),
                version: Some(runtime.implementation_version.clone()),
            },
            origin: ExecutionOrigin::Executed,
            tools: BTreeMap::from([
                ("diplodocus".into(), build.engine_version.clone()),
                ("jupyter".into(), build.engine_version.clone()),
            ]),
            declared_environment_inputs: inputs,
        }
        .into_provenance(
            Some(DiagnosticSource::Repository {
                repository: source.source.repository.clone(),
                path: source.source.path.clone(),
            }),
            None,
        ),
        engine_build_fingerprint: build.executable_digest.clone(),
        components: build.components.clone(),
        policies: ExecutionPolicies::default(),
        kernel: KernelExecutionProvenance {
            spec_fingerprint: fingerprint(launch.spec_digest()),
            launch_fingerprint: fingerprint(launch.launch_digest()),
            search: launch.search().to_vec(),
            interrupt_mode: launch.interrupt_mode(),
            implementation: runtime.implementation.clone(),
            protocol_version: runtime.protocol_version.clone(),
        },
        platform: build.platform.clone(),
        deadlines_ms: deadlines,
    }
}
