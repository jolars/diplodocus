use super::*;
use crate::execution::identity::RepositoryFile;
use crate::execution::{
    ExecutionContext, ExecutionEngine, ExecutionPage, JupyterEngine, PageExecutionRequest,
};
use crate::ir::Block;
use crate::provenance::PANACHE_VERSION;

impl WorkspaceSources {
    /// Execute eligible pages only when the current command authorizes execution.
    ///
    /// Static errors have already stopped assembly. This operation starts a fresh
    /// supervised engine session per eligible page, retains validation evidence,
    /// and rechecks all selected workspace inputs after the final page. Failure
    /// or dropping the future discards the entire attempt's staged assets.
    ///
    /// # Errors
    /// Returns execution, changed-input, or staging failures without a partial
    /// workspace. Cleanup failure retains the original cause.
    pub async fn execute(
        mut self,
        mut options: WorkspaceExecution<'_>,
    ) -> Result<ExecutedWorkspace, AssemblyError> {
        check_cancellation(&mut options.cancellation).await?;
        self.revalidate()?;
        let mut staging: Option<tempfile::TempDir> = None;
        let mut pages = BTreeMap::new();
        let result = async {
            let repositories: BTreeMap<_, _> = self
                .paths
                .repositories
                .iter()
                .map(|r| (r.id.clone(), r.path.clone()))
                .collect();
            let mut order: Vec<_> = self.prepared_pages.iter().collect();
            order.sort_by_key(|(_, page)| (page.collection, &page.relative));
            for (id, page) in order {
                check_cancellation(&mut options.cancellation).await?;
                let Some(preparation) = page.preparation.as_ref().filter(|p| p.execution_eligible)
                else {
                    continue;
                };
                let collection = &self.configuration.content[page.collection];
                let repository_root = &repositories[&collection.repository];
                let declared = collection
                    .execution
                    .declared_environment_inputs
                    .iter()
                    .map(|path| {
                        RepositoryFile::new(&collection.repository, path)
                            .map_err(|_| AssemblyError::InputsChanged)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let engine = JupyterEngine::new(repositories.clone(), declared);
                let request = PageExecutionRequest {
                    page: ExecutionPage {
                        source: page.location.clone(),
                        collection: collection.id.clone(),
                        working_directory: relative(
                            page.path.parent().expect("authored file parent"),
                            repository_root,
                        )?,
                        source_fingerprint: fingerprint_bytes(page.source.as_bytes()),
                        format: collection.format,
                        mode: collection.execution.mode,
                        page_veto: preparation.page_veto,
                        parser_version: PANACHE_VERSION.into(),
                        qmd_policy: "qmd-mvp-v1".into(),
                    },
                    kernel: collection
                        .execution
                        .kernel
                        .clone()
                        .expect("validated execution configuration"),
                    defaults: preparation.defaults.clone(),
                    cells: preparation.cells.clone(),
                    declared_environment_inputs: self.evidence.declared_environment_inputs
                        [&collection.id]
                        .clone(),
                };
                if staging.is_none() {
                    staging = Some(
                        tempfile::Builder::new()
                            .prefix("diplodocus-workspace-")
                            .tempdir_in(options.staging_parent)?,
                    );
                }
                let context = ExecutionContext {
                    repository_root: repository_root.clone(),
                    page_path: page.path.clone(),
                    asset_staging_directory: staging.as_ref().unwrap().path().join("assets"),
                    deadlines: options.deadlines,
                    cancellation: Box::pin(&mut options.cancellation),
                };
                let result = engine.execute_page(context, &request).await?;
                let record = result.validated().record();
                let document = &mut self
                    .workspace
                    .pages
                    .get_mut(id)
                    .expect("assembled page")
                    .document;
                let mut ordinal = 0;
                project(&mut document.document.blocks, &record.cells, &mut ordinal);
                debug_assert_eq!(ordinal, record.cells.len());
                if let Some(provenance) = &record.provenance {
                    document.provenance.push(provenance.execution.clone());
                }
                self.workspace
                    .diagnostics
                    .extend(record.diagnostics.iter().cloned());
                pages.insert(id.clone(), result);
            }
            self.revalidate()?;
            check_cancellation(&mut options.cancellation).await?;
            Ok::<(), AssemblyError>(())
        }
        .await;
        match result {
            Ok(()) => Ok(ExecutedWorkspace {
                sources: self,
                pages,
                staging,
            }),
            Err(mut error) => {
                if let AssemblyError::Execution(failure) = &mut error {
                    failure
                        .diagnostics
                        .splice(0..0, self.workspace.diagnostics.iter().cloned());
                }
                drop(pages);
                if let Some(staging) = staging
                    && let Err(source) = staging.close()
                {
                    return Err(AssemblyError::Cleanup {
                        cause: Box::new(error),
                        source,
                    });
                }
                Err(error)
            }
        }
    }
}

async fn check_cancellation(
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<(), AssemblyError> {
    tokio::select! {
        biased;
        _ = cancellation => Err(AssemblyError::Cancelled),
        _ = std::future::ready(()) => Ok(()),
    }
}

fn project(
    blocks: &mut [Block],
    cells: &[crate::execution::CellExecutionResult],
    ordinal: &mut usize,
) {
    for block in blocks {
        match block {
            Block::CodeCell(cell) => {
                cell.outputs = cells[*ordinal]
                    .outputs
                    .iter()
                    .map(|output| output.output.clone())
                    .collect();
                *ordinal += 1;
            }
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                project(blocks, cells, ordinal)
            }
            Block::List { items, .. } => {
                for item in items {
                    project(&mut item.blocks, cells, ordinal);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        project(&mut cell.blocks, cells, ordinal);
                    }
                }
            }
            _ => {}
        }
    }
}
