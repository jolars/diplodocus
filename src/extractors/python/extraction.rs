//! Portable Python extraction results.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::ir::{Item, Provenance, SchemaVersion, TargetReference};

use super::PythonMetadata;

/// Portable result of one Python target, ready for workspace fragment merging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonExtraction {
    /// Version of the shared portable IR vocabulary.
    pub schema_version: SchemaVersion,
    /// Workspace package whose canonical item IDs scope this fragment.
    pub package: String,
    /// Explicit extraction target.
    pub target: TargetReference,
    /// Static distribution metadata, absent when invalid or dynamic.
    pub metadata: Option<PythonMetadata>,
    /// Canonical API items with structured signatures and documentation.
    pub items: BTreeMap<String, Item>,
    /// Source, semantic, and documentation diagnostics in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Actual parser observations and fingerprints for the complete extraction.
    pub provenance: Provenance,
}
