//! Immutable prepared inputs and checked, slot-owned output evidence.
//!
//! Construction is reserved for the engine and cache adapters. This boundary
//! checks associations and active content; it does not execute pages or prove
//! kernel cleanup, input freshness after execution, or command authorization.

use std::collections::BTreeMap;

use super::assets::PageAssetStore;
use super::identity::{CanonicalError, CanonicalValue};
use super::output_safety::*;
use super::*;
use crate::ir::{CellOutputKind, Fingerprint, OutputRepresentation};
use crate::provenance::fingerprint_bytes;

mod canonical;
mod check;
mod prepared;
#[cfg(test)]
mod tests;

pub use canonical::{RepresentationContent, RepresentationProjection, project_representation};
pub use prepared::PreparedExecution;

/// A rejected association between portable evidence and actively validated values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecordValidationError {
    /// Portable records disagree with prepared inputs or validated values.
    #[error("Execution evidence does not match its prepared inputs or validated content.")]
    Association,
    /// Active staging failed; callers preserve the asset failure and roll back.
    #[error(transparent)]
    Asset(#[from] super::assets::AssetError),
}
impl From<CanonicalError> for RecordValidationError {
    fn from(_: CanonicalError) -> Self {
        Self::Association
    }
}

/// Final, immutable page evidence and the validated values owned by its slots.
///
/// A portable clone carries no validation authority.
///
/// Immutable access and portable serialization remain available:
///
/// ```
/// use diplodocus::execution::{ValidatedPage, PreparedExecution, PageExecutionResult, PageExecutionRecord};
/// fn views(page: &ValidatedPage, prepared: &PreparedExecution, result: &PageExecutionResult) {
///     let mut portable = page.portable_record();
///     portable.cells.clear();
///     let _ = (prepared.request(), prepared.source(), prepared.context());
///     let _ = (result.validated(), result.staged_assets());
///     let json = serde_json::to_string(page.record()).unwrap();
///     let _: PageExecutionRecord = serde_json::from_str(&json).unwrap();
/// }
/// ```
///
/// ```compile_fail
/// use diplodocus::execution::{ValidatedPage, PageExecutionRecord};
/// fn construct(record: PageExecutionRecord) -> ValidatedPage {
///     ValidatedPage { record, slots: Default::default(), diagnostics: vec![], diagnostic_offset: 0 }
/// }
/// ```
/// ```compile_fail
/// use diplodocus::execution::ValidatedPage;
/// fn mutate(page: &mut ValidatedPage) { page.record().cells.clear(); }
/// ```
/// ```compile_fail
/// use diplodocus::execution::ValidatedPage;
/// fn serialize<T: serde::Serialize>() {}
/// serialize::<ValidatedPage>();
/// ```
/// ```compile_fail
/// use diplodocus::execution::ValidatedPage;
/// let _: ValidatedPage = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPage {
    record: PageExecutionRecord,
    slots: BTreeMap<(usize, usize), SlotEvidence>,
    diagnostics: Vec<ExecutionDiagnostic>,
    diagnostic_offset: usize,
}

/// A read-only representation at a final owning cell, slot, and alternative.
#[derive(Debug, Clone, Copy)]
pub enum ValidatedRepresentationRef<'a> {
    /// Literal text, which publishers escape.
    Text(&'a str),
    /// Inert Markdown with complete image bindings.
    Markdown(&'a ValidatedMarkdown),
    /// Allowlisted HTML with complete image bindings.
    Html(&'a ValidatedHtml),
    /// Metadata checked against active staged image bytes.
    Asset(&'a ExecutionAsset),
}

// Every final slot owns its values. Producer identities can repeat after updates.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "The production engine is supported only on Linux."
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnedRepresentation {
    Text(String),
    Markdown {
        value: Box<ValidatedMarkdown>,
        origin: OutputOrigin,
    },
    Html {
        value: ValidatedHtml,
    },
    Asset(ExecutionAsset),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotEvidence {
    pub owning_cell: usize,
    pub slot: usize,
    pub origin: OutputProducer,
    pub representations: Vec<OwnedRepresentation>,
}

/// Current payload attribution; constructing this metadata grants no trust.
///
/// A cache record can omit a producer slot for non-Markdown output. The carrier
/// preserves that absence and merges only known wrapper and diagnostic facts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputProducer {
    /// Current producing cell, after any display update.
    pub cell: usize,
    /// Producer slot when supplied live or retained in persisted evidence.
    pub slot: Option<usize>,
    /// Authored range of the current producing cell.
    pub cell_span: crate::ir::SourceSpan,
}
impl From<OutputOrigin> for OutputProducer {
    fn from(origin: OutputOrigin) -> Self {
        Self {
            cell: origin.cell,
            slot: Some(origin.slot),
            cell_span: origin.cell_span,
        }
    }
}

/// Explicit producing ledger and current-build diagnostics supplied by the adapter.
#[derive(Debug, Default)]
pub(crate) struct DiagnosticEvidence {
    pub before: Vec<crate::diagnostics::Diagnostic>,
    pub execution: Vec<ExecutionDiagnostic>,
    pub after: Vec<crate::diagnostics::Diagnostic>,
}
impl From<Vec<ExecutionDiagnostic>> for DiagnosticEvidence {
    fn from(execution: Vec<ExecutionDiagnostic>) -> Self {
        Self {
            execution,
            ..Self::default()
        }
    }
}

impl ValidatedPage {
    /// Borrow portable evidence without granting mutable access to the carrier.
    pub fn record(&self) -> &PageExecutionRecord {
        &self.record
    }
    /// Clone untrusted portable evidence for workspace projection or inspection.
    pub fn portable_record(&self) -> PageExecutionRecord {
        self.record.clone()
    }
    /// Look up a surviving alternative by its final owner, retaining slot gaps.
    pub fn representation(
        &self,
        cell: usize,
        slot: usize,
        index: usize,
    ) -> Option<ValidatedRepresentationRef<'_>> {
        self.slots
            .get(&(cell, slot))?
            .representations
            .get(index)
            .map(|value| match value {
                OwnedRepresentation::Text(value) => ValidatedRepresentationRef::Text(value),
                OwnedRepresentation::Markdown { value, .. } => {
                    ValidatedRepresentationRef::Markdown(value)
                }
                OwnedRepresentation::Html { value, .. } => ValidatedRepresentationRef::Html(value),
                OwnedRepresentation::Asset(value) => ValidatedRepresentationRef::Asset(value),
            })
    }
    /// Complete final assets, unique and sorted by digest.
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset> {
        self.record.assets.iter()
    }
    /// Typed producing warnings in the same order as their portable projection.
    pub fn diagnostics(&self) -> &[ExecutionDiagnostic] {
        &self.diagnostics
    }
    /// Start of the typed execution ledger in the portable diagnostic vector.
    ///
    /// Cache encoding subtracts this offset from output diagnostic indices and
    /// serializes only [`Self::diagnostics`], never current-build diagnostics.
    pub fn execution_diagnostic_offset(&self) -> usize {
        self.diagnostic_offset
    }
    /// Current payload's producer, distinct from the final owning cell and slot.
    ///
    /// Fragment identity belongs to each Markdown alternative, not this slot.
    pub fn output_origin(&self, cell: usize, slot: usize) -> Option<&OutputProducer> {
        self.slots.get(&(cell, slot)).map(|slot| &slot.origin)
    }
    /// Markdown's exact fragment context, including ordinal and original length.
    /// Other representations use [`Self::output_origin`] and may lack a slot.
    pub fn representation_origin(
        &self,
        cell: usize,
        slot: usize,
        index: usize,
    ) -> Option<&OutputOrigin> {
        match self.slots.get(&(cell, slot))?.representations.get(index)? {
            OwnedRepresentation::Markdown { origin, .. } => Some(origin),
            _ => None,
        }
    }
    /// Shared canonical content, including synthetic error and unsupported forms.
    ///
    /// Errors and unsupported displays expose their synthetic representation at
    /// index zero, although their portable workspace representation vector is empty.
    pub fn canonical_representation(
        &self,
        cell: usize,
        slot: usize,
        index: usize,
    ) -> Result<Option<RepresentationProjection>, CanonicalError> {
        let Some(output) = self
            .record
            .cells
            .iter()
            .find(|c| c.ordinal == cell)
            .and_then(|c| c.outputs.iter().find(|o| o.slot == slot))
        else {
            return Ok(None);
        };
        if let Some(value) = self.representation(cell, slot, index) {
            return project_representation(match value {
                ValidatedRepresentationRef::Text(value) => RepresentationContent::Text(value),
                ValidatedRepresentationRef::Markdown(value) => {
                    RepresentationContent::Markdown(value.canonical_content())
                }
                ValidatedRepresentationRef::Html(value) => {
                    RepresentationContent::Html(value.canonical_content())
                }
                ValidatedRepresentationRef::Asset(value) => {
                    RepresentationContent::Asset(AssetUse::from(value))
                }
            })
            .map(Some);
        }
        if index != 0 {
            return Ok(None);
        }
        match &output.output.kind {
            CellOutputKind::Error {
                name,
                message,
                traceback,
            } => project_representation(RepresentationContent::Error {
                name,
                value: message,
                traceback,
            })
            .map(Some),
            _ if output.unsupported_placeholder().is_some() => project_representation(
                RepresentationContent::Unsupported(&output.offered_mime_types),
            )
            .map(Some),
            _ => Ok(None),
        }
    }
}

/// A validated page and the staging handles transferred after successful cleanup.
///
/// Only the engine owner may retain staging after cleanup and input revalidation.
/// These operations are caller obligations; the constructor does not run them.
///
/// ```compile_fail
/// use diplodocus::execution::{PageExecutionResult, ValidatedPage};
/// fn construct(validated: ValidatedPage) -> PageExecutionResult {
///     PageExecutionResult { validated, staged_assets: vec![] }
/// }
/// ```
/// ```compile_fail
/// use diplodocus::execution::PageExecutionResult;
/// fn mutate(result: &mut PageExecutionResult) { result.staged_assets()[0].path.clear(); }
/// ```
/// ```compile_fail
/// use diplodocus::execution::PageExecutionResult;
/// fn serialize<T: serde::Serialize>() {}
/// serialize::<PageExecutionResult>();
/// ```
/// ```compile_fail
/// use diplodocus::execution::PageExecutionResult;
/// let _: PageExecutionResult = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct PageExecutionResult {
    validated: ValidatedPage,
    staged_assets: Vec<StagedExecutionAsset>,
}
impl PageExecutionResult {
    /// Validated output evidence suitable for trusted consumers.
    pub fn validated(&self) -> &ValidatedPage {
        &self.validated
    }
    /// Borrow retained files; publication or disposal remains the caller's duty.
    pub fn staged_assets(&self) -> &[StagedExecutionAsset] {
        &self.staged_assets
    }

    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "The production engine is supported only on Linux."
        )
    )]
    pub(crate) fn retain(
        validated: ValidatedPage,
        mut store: PageAssetStore,
    ) -> Result<Self, ExecutionFailure> {
        if let Err(error) = store.verify_assets(&validated.record.page, &validated.record.assets) {
            let kind = error
                .failure_kind()
                .unwrap_or(ExecutionFailureKind::OutputValidation);
            let mut failure = ExecutionFailure {
                kind,
                diagnostics: vec![kind.to_diagnostic(
                    &validated.record.page.collection,
                    validated.record.page.source.clone(),
                )],
                cleanup_diagnostics: vec![],
            };
            if store.rollback().is_err() {
                failure
                    .cleanup_diagnostics
                    .push(ExecutionFailureKind::Cleanup.to_diagnostic(
                        &validated.record.page.collection,
                        validated.record.page.source.clone(),
                    ));
            }
            return Err(failure);
        }
        let references: Vec<_> = validated
            .record
            .assets
            .iter()
            .map(|a| a.reference.clone())
            .collect();
        let retained = store.retain(&references)?;
        // All fallible checks precede retention, which rechecks these same bytes.
        Ok(Self {
            validated,
            staged_assets: retained.staged_assets,
        })
    }
}
