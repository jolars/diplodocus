//! Native owners retain local verification handles; only their semantic result
//! crosses the common adapter boundary. Native supervision remains unchanged.

use crate::extractors::adapter::{AdapterEvidence, AdapterOutput};
use crate::ir::PackageMetadata;

impl From<super::rust_native::RustNativeExtraction> for AdapterOutput {
    fn from(result: super::rust_native::RustNativeExtraction) -> Self {
        Self {
            metadata: Some(PackageMetadata::Rust(result.metadata)),
            version: result.extraction.version,
            items: result.extraction.items,
            diagnostics: result.extraction.diagnostics,
            evidence: AdapterEvidence {
                extraction: result.provenance,
                completed: result.doctests.into_iter().collect(),
            },
        }
    }
}

impl From<super::doxygen_native::DoxygenNativeExtraction> for AdapterOutput {
    fn from(result: super::doxygen_native::DoxygenNativeExtraction) -> Self {
        Self {
            version: result.metadata.version.clone(),
            metadata: Some(PackageMetadata::Cpp(result.metadata)),
            items: result.extraction.items,
            diagnostics: result.extraction.diagnostics,
            evidence: AdapterEvidence::new(result.provenance),
        }
    }
}
