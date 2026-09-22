//! Incremental page output reduction; MIME validation remains a separate boundary.
//!
//! The session integration must accept each completed cell before submitting the
//! next one. Validators can capture mutable asset staging in their closure. Raw
//! bundles and display IDs never enter the returned portable records.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::FailureSource;
use super::discovery::normalize_language;
use super::execution::{CellEvent, MimeBundle};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::execution::{
    CellExecutionResult, CellOutcome, ExecutionFailure, ExecutionFailureKind, ExecutionOutput,
    ExecutionPage, OutputVisibility, PreparedCell, RepresentationEvidence, validate_figure_options,
};
use crate::ir::{
    AssetReference, CellOutput, CellOutputKind, Fingerprint, OutputRepresentation, Provenance,
    StreamName,
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
    pub metadata: &'a Map<String, Value>,
}

impl OutputCandidate<'_> {
    pub fn warning(&self, code: DiagnosticCode, message: &str) -> Diagnostic {
        warning(self.page, self.cell, code, message)
    }

    pub fn failure(&self, kind: ExecutionFailureKind, message: &str) -> ExecutionFailure {
        source(self.page, self.cell).failure(kind, message)
    }
}

/// Only validators supply accepted content and its fingerprint. Rejections may
/// carry specific warnings; fatal failures use the enclosing `Result` instead.
pub(super) struct CandidateValidation {
    pub accepted: Option<AcceptedRepresentation>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(super) struct AcceptedRepresentation {
    pub representation: OutputRepresentation,
    pub content_fingerprint: Fingerprint,
    pub policy: Option<String>,
    pub provenance: Vec<Provenance>,
}

/// A reduction result is not a publishable page or a rendering trust token.
/// The engine still owns cleanup, provenance, and validated asset retention.
pub(super) struct ReducedPage {
    pub cells: Vec<CellExecutionResult>,
    pub diagnostics: Vec<Diagnostic>,
    pub retained_assets: Vec<AssetReference>,
}

pub(super) struct OutputReducer {
    page: ExecutionPage,
    errors: ErrorContext,
    cells: Vec<CellExecutionResult>,
    diagnostics: Vec<Diagnostic>,
    displays: BTreeMap<String, Vec<(usize, usize)>>,
    failed: bool,
}

impl OutputReducer {
    pub fn new(page: ExecutionPage, errors: ErrorContext) -> Self {
        Self {
            page,
            errors,
            cells: Vec::new(),
            diagnostics: Vec::new(),
            displays: BTreeMap::new(),
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
        let result = self.accept(prepared, outcome, events, validator);
        if let Err(mut failure) = result {
            self.failed = true;
            failure.diagnostics.splice(0..0, self.diagnostics.clone());
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
        let mut pending_clear = false;
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
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
            match event {
                CellEvent::Stream {
                    name: StreamName::Stdout,
                    mut text,
                } if prepared.options.execution.output.value == OutputVisibility::AsIs => {
                    // Transport chunks need not align with Markdown syntax. Only
                    // uninterrupted stdout belongs to the same fragment.
                    while let Some(CellEvent::Stream {
                        name: StreamName::Stdout,
                        ..
                    }) = events.peek()
                    {
                        let Some(CellEvent::Stream { text: next, .. }) = events.next() else {
                            unreachable!()
                        };
                        text.push_str(&next);
                    }
                    output.output.kind = CellOutputKind::Stream {
                        stream: StreamName::Stdout,
                    };
                    self.rich_output(
                        prepared,
                        &mut output,
                        MimeBundle {
                            data: Value::Object(Map::from_iter([(
                                "text/markdown".into(),
                                Value::String(text.clone()),
                            )])),
                            metadata: Map::new(),
                        },
                        validator,
                    )?;
                    if output.output.representations.is_empty() {
                        // Rejected fragments still have a faithful escaped-text
                        // fallback; streams cannot be display placeholders.
                        plain_stream(&mut output, prepared.ordinal, text);
                    }
                }
                CellEvent::Stream { name, text } => {
                    output.output.kind = CellOutputKind::Stream { stream: name };
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
                    self.rich_output(prepared, &mut output, bundle, validator)?;
                    register = display_id;
                }
                CellEvent::UpdateDisplay { bundle, display_id } => {
                    // Validate even an unregistered update: a fatal asset failure
                    // cannot be hidden by a missing or previously cleared ID.
                    self.rich_output(prepared, &mut output, bundle, validator)?;
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
                        }
                    } else {
                        self.diagnostics.push(warning(
                            &self.page,
                            prepared,
                            DiagnosticCode::UnsupportedCellOutput,
                            "A display update has no surviving registered output.",
                        ));
                    }
                    continue;
                }
                CellEvent::Clear { .. } => unreachable!(),
            }
            if let Some(id) = register {
                self.displays
                    .entry(id)
                    .or_default()
                    .push((prepared.ordinal, next_slot));
            }
            self.cells[prepared.ordinal].outputs.push(output);
            next_slot += 1;
        }
        Ok(())
    }

    fn clear(&mut self, owner: usize) {
        self.cells[owner].outputs.clear();
        self.displays.retain(|_, slots| {
            slots.retain(|&(cell, _)| cell != owner);
            !slots.is_empty()
        });
    }

    fn rich_output(
        &mut self,
        prepared: &PreparedCell,
        output: &mut ExecutionOutput,
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
                let validated = validator(OutputCandidate {
                    page: &self.page,
                    cell: prepared,
                    slot: output.slot,
                    media_type,
                    data,
                    metadata: &bundle.metadata,
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
                }
            }
        } else {
            self.diagnostics.push(warning(
                &self.page,
                prepared,
                DiagnosticCode::InvalidCellOutput,
                "A MIME bundle must be an object.",
            ));
        }
        if output.output.representations.is_empty() && self.diagnostics.len() == first_diagnostic {
            self.diagnostics.push(warning(
                &self.page,
                prepared,
                DiagnosticCode::UnsupportedCellOutput,
                "The output has no supported representation.",
            ));
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
            failure.diagnostics.splice(0..0, self.diagnostics.clone());
            failure
        })?;
        let mut retained = BTreeMap::new();
        for cell in &self.cells {
            for output in &cell.outputs {
                for representation in &output.output.representations {
                    if let OutputRepresentation::Asset { asset, .. } = representation {
                        retained.insert(asset.path.as_str().to_owned(), asset.clone());
                    }
                }
            }
        }
        Ok(ReducedPage {
            cells: self.cells,
            diagnostics: self.diagnostics,
            retained_assets: retained.into_values().collect(),
        })
    }
}

fn source(page: &ExecutionPage, cell: &PreparedCell) -> FailureSource {
    FailureSource {
        collection: page.collection.clone(),
        source: page.source.clone(),
    }
    .for_cell(cell)
}

fn warning(
    page: &ExecutionPage,
    cell: &PreparedCell,
    code: DiagnosticCode,
    message: &str,
) -> Diagnostic {
    let mut diagnostic = source(page, cell)
        .failure(ExecutionFailureKind::OutputValidation, message)
        .diagnostics
        .remove(0);
    diagnostic.code = code;
    diagnostic.severity = Severity::Warning;
    diagnostic
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
