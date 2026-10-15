use serde::{Deserialize, Serialize};

use crate::validation::DocumentIdentity;

/// Versioned description of embeddable documents and their retained identities.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentIndex {
    /// Manifest envelope version. HTML and Markdown export uses version `2`.
    pub format_version: u32,
    /// Documents in deterministic artifact-path order.
    pub documents: Vec<FragmentDocument>,
}

/// HTML and Markdown representations of one snapshot-backed page.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentDocument {
    /// Relative HTML artifact path, independent of the mounted public URL.
    pub path: String,
    /// Relative Markdown artifact path.
    pub markdown_path: String,
    /// Public URL after applying the host's optional URL map.
    pub url: String,
    /// Human-readable page title.
    pub title: String,
    /// Owning package ID, or no package for project documentation.
    pub package: Option<String>,
    /// Extracted version of the owning package.
    pub package_version: Option<String>,
    /// Defining qualified API name, when this page documents an API item.
    pub qualified_name: Option<String>,
    /// Preferred public qualified name, independent of the presentation title.
    pub public_name: Option<String>,
    /// Whether this page belongs in public navigation and indexes.
    pub visible: bool,
    /// All semantic identities whose content belongs to this page.
    pub identities: Vec<DocumentIdentity>,
    /// Heading level, stable anchor, and plain-text title in display order.
    pub headings: Vec<(usize, String, String)>,
}
