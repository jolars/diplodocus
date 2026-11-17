//! Completed native results remain candidates until their current inputs agree.

use super::*;
use crate::incremental::{Manifest, Store};
use crate::ir::*;
use crate::validation::ContentAsset;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub(super) struct Context {
    store: Store,
    stage: &'static str,
    owner: String,
    identity: Fingerprint,
    inputs: Vec<Manifest>,
    package: crate::paths::ResolvedPackagePaths,
    restored: std::sync::Mutex<Vec<Manifest>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completed<T> {
    result: T,
    dependencies: Vec<Manifest>,
}

impl Context {
    pub(super) fn load_with<T: DeserializeOwned, R>(
        &self,
        reporting: &Reporting,
        restore: impl FnOnce(T) -> Option<R>,
    ) -> Option<R> {
        let result = crate::incremental::verification_scope(|| {
            self.store
                .load::<Completed<T>>(self.stage, &self.identity)
                .and_then(|record| {
                    if record.dependencies.is_empty()
                        || record.dependencies.iter().any(|inputs| !inputs.verify())
                    {
                        let _ = self.store.discard(self.stage, &self.identity);
                        return None;
                    }
                    let Some(restored) = restore(record.result) else {
                        let _ = self.store.discard(self.stage, &self.identity);
                        return None;
                    };
                    self.restored
                        .lock()
                        .expect("cache observations")
                        .extend(record.dependencies);
                    Some(restored)
                })
        });
        reporting.cache(
            self.stage,
            &self.owner,
            result.is_some(),
            result
                .is_none()
                .then_some("inputs changed or no valid completed entry"),
        );
        result
    }

    pub(super) fn retain(&self, sources: &mut WorkspaceSources) {
        sources
            .cache_observations
            .extend(self.inputs.iter().cloned());
        sources.cache_observations.extend(
            self.restored
                .lock()
                .expect("cache observations")
                .iter()
                .cloned(),
        );
    }

    pub(super) async fn save_execution<T: Serialize>(
        &self,
        value: &T,
        dependencies: &[String],
        native: Vec<Manifest>,
        deadlines: ExecutionDeadlines,
        cancellation: &mut ExecutionCancellation<'_>,
        sources: &mut WorkspaceSources,
    ) -> Result<(), AssemblyError> {
        if let Some((_, inputs)) =
            r_native::environment_manifest(&self.package, dependencies, deadlines, cancellation)
                .await?
        {
            let mut dependencies = native;
            let complete = !dependencies.is_empty();
            dependencies.extend(inputs);
            let current = crate::incremental::verification_scope(|| {
                self.inputs.iter().all(Manifest::verify)
                    && dependencies.iter().all(Manifest::verify)
            });
            if complete
                && current
                && self
                    .store
                    .save(
                        self.stage,
                        &self.identity,
                        &Completed {
                            result: value,
                            dependencies: dependencies.clone(),
                        },
                    )
                    .is_err()
            {
                sources.reporting.emit(crate::reporting::Event::Warning("The completed native execution could not be cached; continuing with its verified result."));
            }
            sources.cache_observations.extend(dependencies);
        }
        self.retain(sources);
        Ok(())
    }
}

pub(super) fn document_dependencies<'a>(
    documents: impl IntoIterator<Item = &'a SourcedDocument>,
) -> Vec<String> {
    documents
        .into_iter()
        .flat_map(|document| &document.provenance)
        .filter_map(|record| match &record.activity {
            ProvenanceActivity::RdExamples { dependencies, .. }
            | ProvenanceActivity::RMarkdown { dependencies, .. } => Some(dependencies.keys()),
            _ => None,
        })
        .flatten()
        .cloned()
        .collect()
}

pub(super) async fn r_context(
    sources: &WorkspaceSources,
    package_id: &str,
    stage: &'static str,
    semantic: &impl Serialize,
    helpers: &[&str],
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<Option<Context>, AssemblyError> {
    let timing = sources
        .reporting
        .phase(Phase::CacheValidation, Some(package_id), None);
    let index = sources
        .configuration
        .packages
        .iter()
        .position(|p| p.id == package_id)
        .expect("declared package");
    let Some((store, build, inputs)) =
        crate::incremental::package_context(&sources.configuration, &sources.paths, index)
    else {
        timing.finish();
        return Ok(None);
    };
    // Declared optional dependencies remain resolution guards even when absent.
    // Only helpers needed by this stage must resolve before execution.
    let dependencies: Vec<String> = helpers.iter().map(|s| (*s).to_owned()).collect();
    let Some((environment, runtime)) = r_native::environment_manifest(
        &sources.paths.packages[index],
        &dependencies,
        deadlines,
        cancellation,
    )
    .await?
    else {
        sources.reporting.cache(
            stage,
            package_id,
            false,
            Some("native dependency coverage is uncertain"),
        );
        timing.finish();
        return Ok(None);
    };
    let identity = crate::incremental::key(&(
        stage,
        build,
        package_id,
        &inputs,
        environment,
        &runtime,
        semantic,
        deadlines,
    ));
    timing.finish();
    Ok(Some(Context {
        store,
        stage,
        owner: package_id.into(),
        identity,
        inputs: std::iter::once(inputs).chain(runtime).collect(),
        package: sources.paths.packages[index].clone(),
        restored: Default::default(),
    }))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Asset {
    path: DiagnosticPath,
    fingerprint: Fingerprint,
    media_type: String,
    bytes: Vec<u8>,
}

impl Asset {
    pub(super) fn capture(assets: &BTreeMap<DiagnosticPath, ContentAsset>) -> Vec<Self> {
        assets
            .iter()
            .map(|(path, asset)| Self {
                path: path.clone(),
                fingerprint: asset.fingerprint.clone(),
                media_type: asset.media_type.clone(),
                bytes: asset.bytes.clone(),
            })
            .collect()
    }

    pub(super) fn restore(assets: Vec<Self>) -> Option<BTreeMap<DiagnosticPath, ContentAsset>> {
        let mut result = BTreeMap::new();
        for asset in assets {
            if fingerprint_bytes(&asset.bytes) != asset.fingerprint
                || crate::execution::assets::validate_image_bytes(&asset.media_type, &asset.bytes)
                    .is_err()
            {
                return None;
            }
            if result
                .insert(
                    asset.path,
                    ContentAsset {
                        fingerprint: asset.fingerprint,
                        media_type: asset.media_type,
                        bytes: asset.bytes,
                    },
                )
                .is_some()
            {
                return None;
            }
        }
        Some(result)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackageResult {
    pub package: Package,
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageResult {
    pub page: Page,
    pub assets: Vec<Asset>,
    pub references: Manifest,
}
