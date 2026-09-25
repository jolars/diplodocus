use super::*;
use serde::{Deserialize, Serialize};

use crate::assembly::ExecutedWorkspace;
use crate::documents::prepare_collection_document;
use crate::execution::output_safety::*;
use crate::execution::validated::{
    DiagnosticEvidence, OutputProducer, OwnedRepresentation, SlotEvidence,
};
use crate::execution::{
    PageExecutionRecord, PageExecutionRequest, PreparedExecution, ValidatedPage,
    ValidatedRepresentationRef,
};
use crate::ir::{Block, CodeCell, PageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredPage {
    record: PageExecutionRecord,
    diagnostics: Vec<ExecutionDiagnostic>,
    diagnostic_offset: usize,
    slots: Vec<StoredSlot>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSlot {
    owning_cell: usize,
    slot: usize,
    origin: OutputProducer,
    representations: Vec<StoredRepresentation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum StoredRepresentation {
    Text {
        text: String,
    },
    Markdown {
        content: DecodedMarkdown,
        origin: OutputOrigin,
    },
    Html {
        content: DecodedHtml,
    },
    Asset {
        asset: crate::execution::ExecutionAsset,
    },
}

impl StoredPage {
    fn capture(page: &ValidatedPage) -> Result<Self, SnapshotError> {
        let mut slots = Vec::new();
        for cell in &page.record().cells {
            for output in &cell.outputs {
                let mut representations = Vec::new();
                for index in 0..output.output.representations.len() {
                    let stored = match page
                        .representation(cell.ordinal, output.slot, index)
                        .ok_or(SnapshotError::Invalid("missing live output evidence"))?
                    {
                        ValidatedRepresentationRef::Text(text) => {
                            StoredRepresentation::Text { text: text.into() }
                        }
                        ValidatedRepresentationRef::Markdown(value) => {
                            StoredRepresentation::Markdown {
                                content: value.canonical_content().clone(),
                                origin: page
                                    .representation_origin(cell.ordinal, output.slot, index)
                                    .ok_or(SnapshotError::Invalid("missing fragment attribution"))?
                                    .clone(),
                            }
                        }
                        ValidatedRepresentationRef::Html(value) => StoredRepresentation::Html {
                            content: value.canonical_content().clone(),
                        },
                        ValidatedRepresentationRef::Asset(asset) => StoredRepresentation::Asset {
                            asset: asset.clone(),
                        },
                    };
                    representations.push(stored);
                }
                slots.push(StoredSlot {
                    owning_cell: cell.ordinal,
                    slot: output.slot,
                    origin: page
                        .output_origin(cell.ordinal, output.slot)
                        .ok_or(SnapshotError::Invalid("missing output attribution"))?
                        .clone(),
                    representations,
                });
            }
        }
        Ok(Self {
            record: page.portable_record(),
            diagnostics: page.diagnostics().to_vec(),
            diagnostic_offset: page.execution_diagnostic_offset(),
            slots,
        })
    }
}

impl Snapshot {
    /// Copy an executed workspace, retaining all output alternatives and evidence.
    ///
    /// Sources and staged assets are rechecked before this portable owner is
    /// returned. The execution owner may then discard its staging directory.
    pub fn from_executed(
        executed: &ExecutedWorkspace,
        resolved: &ResolvedWorkspace,
    ) -> Result<Self, SnapshotError> {
        executed.sources().revalidate()?;
        resolved.revalidate()?;
        let mut workspace = executed.workspace().clone();
        workspace.diagnostics = resolved.diagnostics().clone();
        let mut snapshot = Self {
            workspace,
            presentation: executed.sources().configuration().presentation.clone(),
            documents: resolved.records().to_vec(),
            assets: resolved.assets().clone(),
            producer: env!("CARGO_PKG_VERSION").into(),
            executions: executed
                .executed_pages()
                .iter()
                .map(|(id, page)| Ok((id.clone(), StoredPage::capture(page.validated())?)))
                .collect::<Result<_, SnapshotError>>()?,
            validated_outputs: BTreeMap::new(),
        };
        snapshot.validated_outputs = snapshot.validate()?;
        Ok(snapshot)
    }

    /// Outputs revalidated against current policies and snapshot-owned bytes.
    pub fn executed_page(&self, page: &str) -> Option<&ValidatedPage> {
        self.validated_outputs.get(page)
    }

    pub(super) fn restore_outputs(&self) -> Result<BTreeMap<String, ValidatedPage>, SnapshotError> {
        use super::validation::require;
        let mut restored = BTreeMap::new();
        for (id, stored) in &self.executions {
            let page = self
                .workspace
                .pages
                .get(id)
                .ok_or(SnapshotError::Invalid("execution page identity"))?;
            let PageKind::Authored { collection } = &page.kind else {
                return Err(SnapshotError::Invalid("execution on non-authored page"));
            };
            let declaration = self
                .workspace
                .content_collections
                .get(collection)
                .ok_or(SnapshotError::Invalid("execution collection"))?;
            require(
                stored.record.page.collection == *collection
                    && page.document.source_location.as_ref() == Some(&stored.record.page.source)
                    && stored.record.page.mode == declaration.execution.mode
                    && stored.record.page.format == declaration.format,
                "execution page association",
            )?;
            let raw = page
                .document
                .raw_source
                .as_ref()
                .ok_or(SnapshotError::Invalid("missing authored source"))?;
            let configuration = crate::configuration::ContentConfiguration {
                id: collection.clone(),
                owner: "project".into(),
                repository: declaration.repository.clone(),
                path: ".".into(),
                mount: declaration.mount.clone(),
                format: declaration.format,
                execution: crate::configuration::ExecutionConfiguration {
                    mode: declaration.execution.mode,
                    engine: declaration.execution.engine,
                    kernel: declaration.execution.kernel.clone(),
                    declared_environment_inputs: Vec::new(),
                },
            };
            let parsed = prepare_collection_document(raw, &configuration)
                .map_err(|_| SnapshotError::Invalid("execution configuration"))?;
            let preparation = parsed
                .preparation
                .ok_or(SnapshotError::Invalid("authored preparation"))?;
            let context = AuthoredOutputContext::new(
                stored.record.page.source.clone(),
                collection.clone(),
                preparation.authored_anchors,
            );
            let request = PageExecutionRequest {
                page: stored.record.page.clone(),
                kernel: declaration
                    .execution
                    .kernel
                    .clone()
                    .ok_or(SnapshotError::Invalid("missing kernel declaration"))?,
                defaults: preparation.defaults,
                cells: preparation.cells,
                declared_environment_inputs: Vec::new(),
            };
            let prepared = PreparedExecution::checked(request, raw.as_bytes().to_vec(), context)
                .map_err(|_| SnapshotError::Invalid("prepared execution inputs"))?;
            let mut projected = parsed.parsed.document;
            let mut index = 0;
            project(&mut projected.blocks, &stored.record.cells, &mut index)?;
            require(
                index == stored.record.cells.len() && projected == page.document.document,
                "execution document projection",
            )?;
            let mut verified = VerifiedAssets::new();
            for asset in &stored.record.assets {
                let bytes = self
                    .assets
                    .get(&asset.reference.fingerprint.value)
                    .ok_or(SnapshotError::Invalid("missing execution asset"))?;
                require(
                    bytes.media_type == asset.media_type,
                    "execution asset media",
                )?;
                verified
                    .verify_bytes(prepared.context(), asset, &bytes.bytes)
                    .map_err(|_| SnapshotError::Invalid("execution asset bytes"))?;
            }
            let mut slots = Vec::new();
            for slot in &stored.slots {
                let mut representations = Vec::new();
                for value in &slot.representations {
                    representations.push(match value {
                        StoredRepresentation::Text { text } => {
                            OwnedRepresentation::Text(text.clone())
                        }
                        StoredRepresentation::Markdown { content, origin } => {
                            OwnedRepresentation::Markdown {
                                value: Box::new(
                                    restore_markdown(
                                        content.clone(),
                                        origin,
                                        prepared.context(),
                                        &verified,
                                    )
                                    .map_err(|_| {
                                        SnapshotError::Invalid("generated Markdown policy")
                                    })?,
                                ),
                                origin: origin.clone(),
                            }
                        }
                        StoredRepresentation::Html { content } => OwnedRepresentation::Html {
                            value: restore_html_content(
                                content.clone(),
                                prepared.context(),
                                &verified,
                            )
                            .map_err(|_| SnapshotError::Invalid("generated HTML policy"))?,
                        },
                        StoredRepresentation::Asset { asset } => {
                            OwnedRepresentation::Asset(asset.clone())
                        }
                    });
                }
                slots.push(SlotEvidence {
                    owning_cell: slot.owning_cell,
                    slot: slot.slot,
                    origin: slot.origin.clone(),
                    representations,
                });
            }
            let end = stored
                .diagnostic_offset
                .checked_add(stored.diagnostics.len())
                .ok_or(SnapshotError::Invalid("diagnostic indices"))?;
            require(end <= stored.record.diagnostics.len(), "diagnostic indices")?;
            let diagnostics = DiagnosticEvidence {
                before: stored.record.diagnostics[..stored.diagnostic_offset].to_vec(),
                execution: stored.diagnostics.clone(),
                after: stored.record.diagnostics[end..].to_vec(),
            };
            let checked = ValidatedPage::restored(
                &prepared,
                stored.record.clone(),
                slots,
                diagnostics,
                verified,
            )
            .map_err(|_| SnapshotError::Invalid("execution record association"))?;
            restored.insert(id.clone(), checked);
        }
        for (id, page) in &self.workspace.pages {
            if !restored.contains_key(id) {
                require(
                    no_outputs(&page.document.document.blocks),
                    "outputs lack validation evidence",
                )?;
            }
        }
        for package in self.workspace.packages.values() {
            for item in package.items.values() {
                if let Some(document) = &item.documentation {
                    require(
                        no_outputs(&document.document.blocks),
                        "execution in API documentation",
                    )?;
                }
            }
        }
        for concept in self.workspace.concepts.values() {
            if let Some(document) = &concept.documentation {
                require(
                    no_outputs(&document.document.blocks),
                    "execution in concept documentation",
                )?;
            }
        }
        Ok(restored)
    }
}

fn cells(blocks: &[Block], visit: &mut impl FnMut(&CodeCell)) {
    for block in blocks {
        match block {
            Block::CodeCell(cell) => visit(cell),
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                cells(blocks, visit)
            }
            Block::List { items, .. } => {
                for item in items {
                    cells(&item.blocks, visit);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        cells(&cell.blocks, visit);
                    }
                }
            }
            _ => {}
        }
    }
}
fn no_outputs(blocks: &[Block]) -> bool {
    let mut empty = true;
    cells(blocks, &mut |cell| empty &= cell.outputs.is_empty());
    empty
}
fn project(
    blocks: &mut [Block],
    records: &[crate::execution::CellExecutionResult],
    ordinal: &mut usize,
) -> Result<(), SnapshotError> {
    for block in blocks {
        match block {
            Block::CodeCell(cell) => {
                let record = records
                    .get(*ordinal)
                    .ok_or(SnapshotError::Invalid("execution cell count"))?;
                cell.outputs = record
                    .outputs
                    .iter()
                    .map(|output| output.output.clone())
                    .collect();
                *ordinal += 1;
            }
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                project(blocks, records, ordinal)?
            }
            Block::List { items, .. } => {
                for item in items {
                    project(&mut item.blocks, records, ordinal)?;
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        project(&mut cell.blocks, records, ordinal)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
