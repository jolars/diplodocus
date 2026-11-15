//! Common, route-free handoff from a built-in API adapter to workspace assembly.
//!
//! This is an internal contract, not a plugin ABI or a second serialized IR.
//! Signatures, documents, unresolved symbolic links, and fact-level source
//! evidence remain on the existing `Item` and `PackageMetadata` records.
//! Local process, staging, watch, and input-revalidation owners stay with the
//! orchestrator. Error-bearing results are inspectable, never publishable.

use std::collections::BTreeMap;

use crate::diagnostics::{Diagnostic, DiagnosticPath};
use crate::ir::{
    ExtractionInput, Item, PackageMetadata, Provenance, ProvenanceActivity, RPackageMetadata,
    TargetReference,
};

/// Portable semantic output plus the evidence needed to assemble one target.
pub(crate) struct AdapterOutput {
    pub metadata: Option<PackageMetadata>,
    pub version: Option<String>,
    pub items: BTreeMap<String, Item>,
    pub diagnostics: Vec<Diagnostic>,
    pub evidence: AdapterEvidence,
}

/// Tool versions, parser settings, observed inputs, and completed native work.
/// These are existing IR records, without transient timings or local paths.
pub(crate) struct AdapterEvidence {
    pub extraction: Provenance,
    pub completed: Vec<Provenance>,
}

impl AdapterEvidence {
    pub(crate) fn new(extraction: Provenance) -> Self {
        Self {
            extraction,
            completed: Vec::new(),
        }
    }

    pub(crate) fn target(&self) -> &TargetReference {
        match &self.extraction.activity {
            ProvenanceActivity::Extraction { target, .. } => target,
            _ => unreachable!("built-in adapter evidence must describe extraction"),
        }
    }

    pub(crate) fn inputs(&self) -> &BTreeMap<String, BTreeMap<DiagnosticPath, ExtractionInput>> {
        match &self.extraction.activity {
            ProvenanceActivity::Extraction { inputs, .. } => inputs,
            _ => unreachable!("built-in adapter evidence must describe extraction"),
        }
    }
}

impl From<super::python::PythonExtraction> for AdapterOutput {
    fn from(result: super::python::PythonExtraction) -> Self {
        Self {
            version: result
                .metadata
                .as_ref()
                .map(|metadata| metadata.version.clone()),
            metadata: result.metadata.map(PackageMetadata::Python),
            items: result.items,
            diagnostics: result.diagnostics,
            evidence: AdapterEvidence::new(result.provenance),
        }
    }
}

impl From<super::r::RExtraction> for AdapterOutput {
    fn from(result: super::r::RExtraction) -> Self {
        Self {
            version: result
                .metadata
                .as_ref()
                .map(|metadata| metadata.version.clone()),
            metadata: result.metadata.map(|description| {
                PackageMetadata::R(RPackageMetadata {
                    description,
                    help_links: result.help_links,
                    runtime: None,
                })
            }),
            items: result.items,
            diagnostics: result.diagnostics,
            evidence: AdapterEvidence::new(result.provenance),
        }
    }
}

impl From<super::julia::JuliaExtraction> for AdapterOutput {
    fn from(result: super::julia::JuliaExtraction) -> Self {
        Self {
            version: result
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.version.clone()),
            metadata: result.metadata.map(PackageMetadata::Julia),
            items: result.items,
            diagnostics: result.diagnostics,
            evidence: AdapterEvidence::new(result.provenance),
        }
    }
}
