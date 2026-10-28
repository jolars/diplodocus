//! Explicit local browser components and their portable bundles.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::assembly::WorkspaceSources;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{Attributes, Block};
use crate::provenance::fingerprint_bytes;
use crate::validation::ContentAsset;

/// An author-supplied browser component, built before documentation extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ComponentConfiguration {
    /// Identifier used by authored component blocks.
    pub id: String,
    /// Declared repository containing the bundle.
    pub repository: String,
    /// Repository-relative directory containing the complete built bundle.
    pub path: PathBuf,
    /// Bundle-relative ES module exporting `mount`.
    pub module: String,
    /// Bundle-relative stylesheets loaded before mounting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stylesheets: Vec<String>,
}

/// Snapshot-owned component assets with no machine-local paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentBundle {
    /// Bundle-relative module path.
    pub module: String,
    /// Bundle-relative stylesheet paths.
    pub stylesheets: Vec<String>,
    /// Bundle-relative filenames mapped to retained SHA-256 asset digests.
    pub files: BTreeMap<String, String>,
}

pub(crate) fn name(attributes: &Attributes) -> Option<&str> {
    attributes
        .key_values
        .iter()
        .find(|a| a.key.value == "name")
        .map(|a| a.value.value.as_str())
}

pub(crate) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '?', '#', '%', ':'])
        && !path.chars().any(char::is_control)
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && Path::new(path).to_str() == Some(path)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

pub(crate) fn media_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json" | "map") => "application/json",
        _ => "application/octet-stream",
    }
}

fn diagnostic(message: impl Into<String>) -> Box<Diagnostic> {
    Box::new(Diagnostic::new(
        DiagnosticCode::InvalidComponent,
        Severity::Error,
        message,
    ))
}

type Collected = (
    BTreeMap<String, ComponentBundle>,
    BTreeMap<String, ContentAsset>,
    Vec<(PathBuf, PathBuf)>,
);

pub(crate) fn collect(sources: &WorkspaceSources) -> Result<Collected, Box<Diagnostic>> {
    let mut bundles = BTreeMap::new();
    let mut assets = BTreeMap::new();
    let mut inputs = Vec::new();
    for declaration in &sources.configuration().components {
        if declaration.id.is_empty()
            || !declaration
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || bundles.contains_key(&declaration.id)
        {
            return Err(diagnostic(
                "Component IDs must be unique nonempty ASCII names.",
            ));
        }
        let repository = sources
            .paths()
            .repositories
            .iter()
            .find(|r| r.id == declaration.repository)
            .ok_or_else(|| {
                diagnostic(format!(
                    "Unknown component repository: {}",
                    declaration.repository
                ))
            })?;
        let relative = declaration
            .path
            .to_str()
            .filter(|p| safe_path(p))
            .ok_or_else(|| {
                diagnostic("Component directories must be normalized repository-relative paths.")
            })?;
        let root = repository.path.join(relative);
        let mut files = BTreeMap::new();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            // Reject links before traversal, including a linked bundle root.
            let mut prefix = repository.path.clone();
            for part in directory
                .strip_prefix(&repository.path)
                .map_err(|_| diagnostic("Component path escapes its repository."))?
                .components()
            {
                prefix.push(part);
                let metadata = std::fs::symlink_metadata(&prefix)
                    .map_err(|e| diagnostic(format!("Cannot read component directory: {e}")))?;
                if metadata.file_type().is_symlink() {
                    return Err(diagnostic("Component bundles cannot contain symlinks."));
                }
            }
            let entries = std::fs::read_dir(&directory)
                .map_err(|e| diagnostic(format!("Cannot read component directory: {e}")))?;
            for entry in entries {
                let entry = entry.map_err(|e| diagnostic(e.to_string()))?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|e| diagnostic(e.to_string()))?;
                if kind.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !kind.is_file() {
                    return Err(diagnostic(
                        "Component bundles require regular files without symlinks.",
                    ));
                }
                let relative = path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .filter(|p| safe_path(p))
                    .ok_or_else(|| diagnostic("Invalid component filename."))?;
                let checked = crate::paths::resolve_input_file(
                    &repository.path,
                    "component",
                    path.strip_prefix(&repository.path).unwrap(),
                    &repository.path,
                )
                .map_err(|e| diagnostic(e.to_string()))?;
                let bytes = std::fs::read(&checked).map_err(|e| diagnostic(e.to_string()))?;
                let fingerprint = fingerprint_bytes(&bytes);
                files.insert(relative.to_owned(), fingerprint.value.clone());
                // Media types belong to bundle paths; shared bytes remain inert downloads.
                assets.insert(
                    fingerprint.value.clone(),
                    ContentAsset {
                        fingerprint,
                        media_type: "application/octet-stream".into(),
                        bytes,
                    },
                );
                inputs.push((checked, repository.path.clone()));
            }
        }
        if !safe_path(&declaration.module)
            || !matches!(
                Path::new(&declaration.module)
                    .extension()
                    .and_then(|s| s.to_str()),
                Some("js" | "mjs")
            )
            || !files.contains_key(&declaration.module)
            || declaration
                .stylesheets
                .iter()
                .any(|s| !safe_path(s) || !s.ends_with(".css") || !files.contains_key(s))
        {
            return Err(diagnostic(format!(
                "Component {} has a missing or invalid module or stylesheet.",
                declaration.id
            )));
        }
        bundles.insert(
            declaration.id.clone(),
            ComponentBundle {
                module: declaration.module.clone(),
                stylesheets: declaration.stylesheets.clone(),
                files,
            },
        );
    }
    let known = bundles.keys().map(String::as_str).collect();
    for page in sources.workspace().pages.values() {
        validate_blocks(&page.document.document.blocks, &known)?;
    }
    for package in sources.workspace().packages.values() {
        for item in package.items.values() {
            if let Some(document) = &item.documentation {
                validate_blocks(&document.document.blocks, &known)?;
            }
        }
    }
    for concept in sources.workspace().concepts.values() {
        if let Some(document) = &concept.documentation {
            validate_blocks(&document.document.blocks, &known)?;
        }
    }
    Ok((bundles, assets, inputs))
}

pub(crate) fn validate_blocks(
    blocks: &[Block],
    known: &BTreeSet<&str>,
) -> Result<(), Box<Diagnostic>> {
    for block in blocks {
        match block {
            Block::Component {
                attributes, blocks, ..
            } => {
                if !name(attributes).is_some_and(|n| known.contains(n)) {
                    return Err(diagnostic(
                        "Component blocks must name a declared component.",
                    ));
                }
                validate_blocks(blocks, known)?;
            }
            Block::Container { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::Callout { blocks, .. }
            | Block::BlockQuote { blocks, .. } => validate_blocks(blocks, known)?,
            Block::List { items, .. } => {
                for item in items {
                    validate_blocks(&item.blocks, known)?;
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        validate_blocks(&cell.blocks, known)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl ComponentBundle {
    pub(crate) fn prefix(&self, id: &str) -> String {
        let digest = fingerprint_bytes(&serde_json::to_vec(self).expect("serializable bundle"));
        format!("assets/components/{id}/{}/", digest.value)
    }
}
