//! Incremental page output reduction with slot-owned validation evidence.
//!
//! The session integration must accept each completed cell before submitting the
//! next one. Validators can capture mutable asset staging in their closure. Raw
//! bundles and display IDs never enter the returned portable records.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::FailureSource;
use super::discovery::normalize_language;
use super::execution::{CellEvent, MimeBundle};
use crate::diagnostics::Diagnostic;
use crate::execution::output_safety::{
    AuthoredOutputContext, DiagnosticAttribution, ExecutionDiagnostic, OutputOrigin,
};
use crate::execution::validated::{OwnedRepresentation, SlotEvidence};
use crate::execution::{
    CellExecutionResult, CellOutcome, ExecutionAsset, ExecutionFailure, ExecutionFailureKind,
    ExecutionOutput, ExecutionPage, OutputVisibility, PreparedCell, RepresentationEvidence,
    validate_figure_options,
};
use crate::ir::{
    CellOutput, CellOutputKind, Fingerprint, OutputRepresentation, Provenance, StreamName,
};
use crate::provenance::fingerprint_bytes;

mod errors;
pub(super) mod images;
pub(super) mod text;
pub(super) use errors::ErrorContext;
#[cfg(test)]
mod tests;

const MIME_PREFERENCE: [&str; 6] = [
    "image/svg+xml",
    "image/png",
    "image/jpeg",
    "text/markdown",
    "text/html",
    "text/plain",
];

/// Local validation input, including the current producer rather than a replaced
/// slot's original owner. Metadata is untrusted and never copied into the IR.
pub(super) struct OutputCandidate<'a> {
    pub page: &'a ExecutionPage,
    pub cell: &'a PreparedCell,
    pub slot: usize,
    pub media_type: &'a str,
    pub data: &'a Value,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Kernel metadata is available to validators but ignored by the active policy."
        )
    )]
    pub metadata: &'a Map<String, Value>,
    pub context: &'a AuthoredOutputContext,
    pub fragment_ordinal: usize,
}

impl OutputCandidate<'_> {
    pub fn origin(&self) -> OutputOrigin {
        OutputOrigin {
            cell: self.cell.ordinal,
            slot: self.slot,
            cell_span: self.cell.cell.span,
            fragment: None,
        }
    }

    pub fn attribution(&self) -> DiagnosticAttribution {
        DiagnosticAttribution::output(self.context, &self.origin(), None)
    }

    pub fn failure(&self, kind: ExecutionFailureKind, message: &str) -> ExecutionFailure {
        source(self.page, self.cell).failure(kind, message)
    }
}

/// Only validators supply accepted content and its fingerprint. Rejections may
/// carry specific warnings; fatal failures use the enclosing `Result` instead.
pub(super) struct CandidateValidation {
    pub accepted: Option<AcceptedRepresentation>,
    pub diagnostics: Vec<ExecutionDiagnostic>,
}

pub(super) struct AcceptedRepresentation {
    pub representation: OutputRepresentation,
    pub content_fingerprint: Fingerprint,
    pub policy: Option<String>,
    pub provenance: Vec<Provenance>,
    pub value: OwnedRepresentation,
}

/// A reduction result is not a publishable page or a rendering trust token.
/// The engine still owns cleanup, provenance, and validated asset retention.
pub(super) struct ReducedPage {
    pub cells: Vec<CellExecutionResult>,
    pub diagnostics: Vec<Diagnostic>,
    #[cfg(test)]
    pub retained_assets: Vec<crate::ir::AssetReference>,
    pub assets: Vec<ExecutionAsset>,
    pub slots: Vec<SlotEvidence>,
    pub execution_diagnostics: Vec<ExecutionDiagnostic>,
}

pub(super) struct OutputReducer {
    page: ExecutionPage,
    errors: ErrorContext,
    cells: Vec<CellExecutionResult>,
    diagnostics: Vec<ExecutionDiagnostic>,
    displays: BTreeMap<String, Vec<(usize, usize)>>,
    slots: BTreeMap<(usize, usize), SlotEvidence>,
    context: AuthoredOutputContext,
    next_fragment: usize,
    failed: bool,
}

impl OutputReducer {
    #[cfg(test)]
    pub fn new(page: ExecutionPage, errors: ErrorContext) -> Self {
        let context = AuthoredOutputContext::new(
            page.source.clone(),
            page.collection.clone(),
            Default::default(),
        );
        Self::with_context(page, errors, context)
    }

    pub fn with_context(
        page: ExecutionPage,
        errors: ErrorContext,
        context: AuthoredOutputContext,
    ) -> Self {
        Self {
            page,
            errors,
            cells: Vec::new(),
            diagnostics: Vec::new(),
            displays: BTreeMap::new(),
            slots: BTreeMap::new(),
            context,
            next_fragment: 0,
            failed: false,
        }
    }

    /// Accept one completed cell. A failed reducer can never be finalized, even
    /// if its caller mistakenly catches a fatal validation error and continues.
    pub fn accept_cell(
        &mut self,
        prepared: &PreparedCell,
        outcome: CellOutcome,
        events: Vec<CellEvent>,
        validator: &mut impl FnMut(OutputCandidate<'_>) -> Result<CandidateValidation, ExecutionFailure>,
    ) -> Result<(), ExecutionFailure> {
        let protocol_warnings: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                CellEvent::Warning(diagnostic) => Some(diagnostic.clone()),
                _ => None,
            })
            .collect();
        let first_diagnostic = self.diagnostics.len();
        let result = self.accept(prepared, outcome, events, validator);
        if let Err(mut failure) = result {
            let visited = self.diagnostics[first_diagnostic..]
                .iter()
                .filter(|diagnostic| {
                    matches!(diagnostic, ExecutionDiagnostic::KernelMessageIgnored { .. })
                })
                .count();
            self.diagnostics
                .extend(protocol_warnings.into_iter().skip(visited));
            self.failed = true;
            failure.diagnostics.splice(
                0..0,
                self.diagnostics
                    .iter()
                    .map(|d| d.to_diagnostic(&self.page.collection)),
            );
            return Err(failure);
        }
        Ok(())
    }

    fn accept(
        &mut self,
        prepared: &PreparedCell,
        outcome: CellOutcome,
        events: Vec<CellEvent>,
        validator: &mut impl FnMut(OutputCandidate<'_>) -> Result<CandidateValidation, ExecutionFailure>,
    ) -> Result<(), ExecutionFailure> {
        let skipped = matches!(outcome, CellOutcome::Skipped { .. });
        if self.failed
            || self.context.source() != &self.page.source
            || self.context.collection() != self.page.collection
            || prepared.ordinal != self.cells.len()
            || !prepared.cell.outputs.is_empty()
            || (skipped && !events.is_empty())
        {
            return Err(source(&self.page, prepared).failure(
                ExecutionFailureKind::OutputValidation,
                "Invalid cell sequence for output reduction.",
            ));
        }
        self.cells.push(CellExecutionResult {
            ordinal: prepared.ordinal,
            language: prepared.cell.language.as_deref().map(normalize_language),
            span: prepared.cell.span,
            source_segments: prepared.cell.source_segments.clone(),
            submitted_source_fingerprint: (!skipped)
                .then(|| fingerprint_bytes(prepared.cell.source.as_bytes())),
            options: prepared.options.clone(),
            outcome,
            outputs: Vec::new(),
        });
        let mut next_slot = 0;
        self.next_fragment = 0;
        let mut pending_clear = false;
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            if let CellEvent::Warning(diagnostic) = event {
                self.diagnostics.push(diagnostic);
                continue;
            }
            if let CellEvent::Clear { wait } = event {
                pending_clear = wait;
                if !wait {
                    self.clear(prepared.ordinal);
                }
                continue;
            }
            if pending_clear {
                self.clear(prepared.ordinal);
                pending_clear = false;
            }
            let mut output = ExecutionOutput {
                owning_cell: prepared.ordinal,
                producing_cell: prepared.ordinal,
                updating_cell: None,
                slot: next_slot,
                output: CellOutput {
                    kind: CellOutputKind::Display,
                    representations: Vec::new(),
                    provenance: Vec::new(),
                },
                offered_mime_types: Default::default(),
                selected_mime_type: None,
                representations: Vec::new(),
                diagnostic_indices: Vec::new(),
            };
            let mut register = None;
            let mut evidence = SlotEvidence {
                owning_cell: prepared.ordinal,
                slot: next_slot,
                origin: OutputOrigin {
                    cell: prepared.ordinal,
                    slot: next_slot,
                    cell_span: prepared.cell.span,
                    fragment: None,
                }
                .into(),
                representations: Vec::new(),
            };
            match event {
                CellEvent::Stream {
                    name: StreamName::Stdout,
                    mut text,
                } if prepared.options.execution.output.value == OutputVisibility::AsIs => {
                    // Transport chunks need not align with Markdown syntax. Only
                    // uninterrupted stdout belongs to the same fragment.
                    let mut warnings = Vec::new();
                    while matches!(
                        events.peek(),
                        Some(
                            CellEvent::Stream {
                                name: StreamName::Stdout,
                                ..
                            } | CellEvent::Warning(_)
                        )
                    ) {
                        match events.next().unwrap() {
                            CellEvent::Stream { text: next, .. } => text.push_str(&next),
                            CellEvent::Warning(diagnostic) => warnings.push(diagnostic),
                            _ => unreachable!(),
                        }
                    }
                    output.output.kind = CellOutputKind::Stream {
                        stream: StreamName::Stdout,
                    };
                    let result = self.rich_output(
                        prepared,
                        &mut output,
                        &mut evidence,
                        MimeBundle {
                            data: Value::Object(Map::from_iter([(
                                "text/markdown".into(),
                                Value::String(text.clone()),
                            )])),
                            metadata: Map::new(),
                        },
                        validator,
                    );
                    self.diagnostics.extend(warnings);
                    result?;
                    if output.output.representations.is_empty() {
                        // Rejected fragments still have a faithful escaped-text
                        // fallback; streams cannot be display placeholders.
                        evidence
                            .representations
                            .push(OwnedRepresentation::Text(text.clone()));
                        plain_stream(&mut output, prepared.ordinal, text);
                    }
                }
                CellEvent::Stream { name, text } => {
                    output.output.kind = CellOutputKind::Stream { stream: name };
                    evidence
                        .representations
                        .push(OwnedRepresentation::Text(text.clone()));
                    plain_stream(&mut output, prepared.ordinal, text);
                }
                CellEvent::Error {
                    name,
                    message,
                    traceback,
                } => {
                    output.output.kind = self.errors.normalize(name, message, traceback);
                }
                CellEvent::Display { bundle, display_id }
                | CellEvent::Result { bundle, display_id } => {
                    self.rich_output(prepared, &mut output, &mut evidence, bundle, validator)?;
                    register = display_id;
                }
                CellEvent::UpdateDisplay { bundle, display_id } => {
                    // Validate even an unregistered update: a fatal asset failure
                    // cannot be hidden by a missing or previously cleared ID.
                    self.rich_output(prepared, &mut output, &mut evidence, bundle, validator)?;
                    let slots = display_id.as_ref().and_then(|id| self.displays.get(id));
                    if let Some(slots) = slots.filter(|slots| !slots.is_empty()) {
                        for &(owner, slot) in slots {
                            let previous = self.cells[owner]
                                .outputs
                                .iter_mut()
                                .find(|output| output.slot == slot)
                                .expect("display registrations identify live slots");
                            output.owning_cell = previous.owning_cell;
                            output.producing_cell = previous.producing_cell;
                            output.slot = previous.slot;
                            output.updating_cell = Some(prepared.ordinal);
                            *previous = output.clone();
                            evidence.owning_cell = owner;
                            evidence.slot = slot;
                            self.slots.insert((owner, slot), evidence.clone());
                        }
                    } else {
                        self.diagnostics
                            .push(ExecutionDiagnostic::UnknownDisplayUpdate {
                                attribution: self.attribution(prepared, next_slot),
                            });
                    }
                    continue;
                }
                CellEvent::Clear { .. } | CellEvent::Warning(_) => unreachable!(),
            }
            if let Some(id) = register {
                self.displays
                    .entry(id)
                    .or_default()
                    .push((prepared.ordinal, next_slot));
            }
            self.cells[prepared.ordinal].outputs.push(output);
            self.slots.insert((prepared.ordinal, next_slot), evidence);
            next_slot += 1;
        }
        Ok(())
    }

    fn clear(&mut self, owner: usize) {
        self.cells[owner].outputs.clear();
        self.slots.retain(|(cell, _), _| *cell != owner);
        self.displays.retain(|_, slots| {
            slots.retain(|&(cell, _)| cell != owner);
            !slots.is_empty()
        });
    }

    fn rich_output(
        &mut self,
        prepared: &PreparedCell,
        output: &mut ExecutionOutput,
        evidence: &mut SlotEvidence,
        bundle: MimeBundle,
        validator: &mut impl FnMut(OutputCandidate<'_>) -> Result<CandidateValidation, ExecutionFailure>,
    ) -> Result<(), ExecutionFailure> {
        let first_diagnostic = self.diagnostics.len();
        if let Some(data) = bundle.data.as_object() {
            output.offered_mime_types.extend(data.keys().cloned());
            for media_type in MIME_PREFERENCE {
                let Some(data) = data.get(media_type) else {
                    continue;
                };
                let fragment_ordinal = self.next_fragment;
                if media_type == "text/markdown" {
                    self.next_fragment += 1;
                }
                let validated = validator(OutputCandidate {
                    page: &self.page,
                    cell: prepared,
                    slot: output.slot,
                    media_type,
                    data,
                    metadata: &bundle.metadata,
                    context: &self.context,
                    fragment_ordinal,
                })?;
                self.diagnostics.extend(validated.diagnostics);
                if let Some(accepted) = validated.accepted {
                    if representation_media_type(&accepted.representation) != media_type {
                        return Err(source(&self.page, prepared).failure(
                            ExecutionFailureKind::OutputValidation,
                            "The output validator returned a different media type.",
                        ));
                    }
                    output
                        .selected_mime_type
                        .get_or_insert_with(|| media_type.into());
                    output.representations.push(RepresentationEvidence {
                        content_fingerprint: accepted.content_fingerprint,
                        producing_cell: prepared.ordinal,
                        policy: accepted.policy,
                    });
                    output.output.representations.push(accepted.representation);
                    output.output.provenance.extend(accepted.provenance);
                    evidence.representations.push(accepted.value);
                }
            }
        } else {
            self.diagnostics
                .push(ExecutionDiagnostic::InvalidMimeBundle {
                    attribution: self.attribution(prepared, output.slot),
                });
        }
        if output.output.representations.is_empty() && self.diagnostics.len() == first_diagnostic {
            self.diagnostics
                .push(ExecutionDiagnostic::NoSupportedRepresentation {
                    attribution: self.attribution(prepared, output.slot),
                    mime_types: output.offered_mime_types.iter().cloned().collect(),
                });
        }
        output
            .diagnostic_indices
            .extend(first_diagnostic..self.diagnostics.len());
        Ok(())
    }

    pub fn finish(self) -> Result<ReducedPage, ExecutionFailure> {
        if self.failed {
            return Err(FailureSource {
                collection: self.page.collection.clone(),
                source: self.page.source.clone(),
            }
            .failure(
                ExecutionFailureKind::OutputValidation,
                "Output reduction previously failed.",
            ));
        }
        validate_figure_options(&self.page, &self.cells).map_err(|mut failure| {
            failure.diagnostics.splice(
                0..0,
                self.diagnostics
                    .iter()
                    .map(|d| d.to_diagnostic(&self.page.collection)),
            );
            failure
        })?;
        let mut retained = BTreeMap::new();
        for slot in self.slots.values() {
            for representation in &slot.representations {
                let assets: Vec<_> = match representation {
                    OwnedRepresentation::Text(_) => vec![],
                    OwnedRepresentation::Asset(asset) => vec![asset],
                    OwnedRepresentation::Markdown { value, .. } => {
                        value.referenced_assets().collect()
                    }
                    OwnedRepresentation::Html { value } => value.referenced_assets().collect(),
                };
                for asset in assets {
                    if let Some(previous) =
                        retained.insert(asset.reference.fingerprint.value.clone(), asset.clone())
                        && previous != *asset
                    {
                        let mut failure = FailureSource {
                            collection: self.page.collection.clone(),
                            source: self.page.source.clone(),
                        }
                        .failure(
                            ExecutionFailureKind::AssetCollision,
                            "Surviving execution assets have conflicting metadata.",
                        );
                        failure.diagnostics.splice(
                            0..0,
                            self.diagnostics
                                .iter()
                                .map(|d| d.to_diagnostic(&self.page.collection)),
                        );
                        return Err(failure);
                    }
                }
            }
        }
        Ok(ReducedPage {
            cells: self.cells,
            diagnostics: self
                .diagnostics
                .iter()
                .map(|d| d.to_diagnostic(&self.page.collection))
                .collect(),
            execution_diagnostics: self.diagnostics,
            #[cfg(test)]
            retained_assets: retained
                .values()
                .map(|asset| asset.reference.clone())
                .collect(),
            assets: retained.into_values().collect(),
            slots: self.slots.into_values().collect(),
        })
    }

    fn attribution(&self, prepared: &PreparedCell, slot: usize) -> DiagnosticAttribution {
        DiagnosticAttribution::output(
            &self.context,
            &OutputOrigin {
                cell: prepared.ordinal,
                slot,
                cell_span: prepared.cell.span,
                fragment: None,
            },
            None,
        )
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics
            .iter()
            .map(|d| d.to_diagnostic(&self.page.collection))
            .collect()
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

fn source(page: &ExecutionPage, cell: &PreparedCell) -> FailureSource {
    FailureSource {
        collection: page.collection.clone(),
        source: page.source.clone(),
    }
    .for_cell(cell)
}

fn representation_media_type(representation: &OutputRepresentation) -> &str {
    match representation {
        OutputRepresentation::PlainText { media_type, .. }
        | OutputRepresentation::MarkdownBlocks { media_type, .. }
        | OutputRepresentation::Asset { media_type, .. }
        | OutputRepresentation::HtmlCandidate { media_type, .. } => media_type,
    }
}

fn plain_stream(output: &mut ExecutionOutput, producer: usize, text: String) {
    output.offered_mime_types.insert("text/plain".into());
    output.selected_mime_type = Some("text/plain".into());
    output.representations.push(RepresentationEvidence {
        content_fingerprint: fingerprint_bytes(text.as_bytes()),
        producing_cell: producer,
        policy: None,
    });
    output
        .output
        .representations
        .push(OutputRepresentation::PlainText {
            media_type: "text/plain".into(),
            text,
        });
}
