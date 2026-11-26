use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::assembly::WorkspaceSources;
use crate::configuration::{ContentConfiguration, ExecutionMode};
use crate::documents::{AuthoredFormat, document_title, prepare_collection_document};
use crate::paths::ResolvedRepositoryPaths;
use crate::rendering::{self, ImageCache, RenderedFile, RenderedSite};
use crate::site::{Site, SiteError};
use crate::validation::{self, DocumentIdentity, ResolvedWorkspace};

pub(super) struct PreviewSources {
    pages: BTreeMap<PathBuf, SourcePage>,
    pub roots: BTreeSet<PathBuf>,
}

struct SourcePage {
    identity: DocumentIdentity,
    relative: String,
    collection: ContentConfiguration,
    repository: ResolvedRepositoryPaths,
}

impl PreviewSources {
    pub(super) fn new(sources: &WorkspaceSources, resolved: &ResolvedWorkspace) -> Self {
        let mut seen = BTreeSet::new();
        let mut pages = BTreeMap::new();
        let mut shared = BTreeSet::new();
        for prepared in sources.prepared_pages().values() {
            for include in &prepared.source_ranges {
                if include.include_chain.is_empty() {
                    continue;
                }
                if let Some(repository) = sources
                    .paths()
                    .repositories
                    .iter()
                    .find(|repository| repository.id == include.original.repository)
                {
                    shared.insert(repository.path.join(include.original.path.as_str()));
                }
            }
            if !seen.insert(prepared.path.clone()) {
                shared.insert(prepared.path.clone());
            }
        }
        let shared_targets: BTreeSet<_> = resolved.shared_file_targets().collect();
        for (id, prepared) in sources.prepared_pages() {
            let collection = &sources.configuration().content[prepared.collection];
            let paths = &sources.paths().content[prepared.collection];
            let repository = &sources.paths().repositories[paths.repository_index];
            if collection.format == AuthoredFormat::Gfm
                && collection.execution.mode == ExecutionMode::Never
                && prepared
                    .source_ranges
                    .iter()
                    .all(|range| range.include_chain.is_empty())
                && paths.source_root.join(prepared.relative.as_str()) == prepared.path
                && !shared.contains(&prepared.path)
                && !shared_targets.contains(prepared.path.as_path())
                && !resolved
                    .public_directory_paths()
                    .any(|dir| prepared.path.starts_with(dir))
                && sources
                    .configuration()
                    .content
                    .iter()
                    .all(|content| content.incremental.extra_inputs.is_empty())
            {
                pages.insert(
                    prepared.path.clone(),
                    SourcePage {
                        identity: DocumentIdentity::Page { page: id.clone() },
                        relative: prepared.relative.as_str().to_owned(),
                        collection: collection.clone(),
                        repository: repository.clone(),
                    },
                );
            }
        }
        let roots = sources
            .paths()
            .content
            .iter()
            .map(|paths| paths.path.as_path())
            .filter(|path| path.is_dir())
            .chain(resolved.public_directory_paths())
            .map(Path::to_path_buf)
            .collect();
        Self { pages, roots }
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.pages.contains_key(path)
    }

    pub(super) fn render(
        &self,
        site: &Site<'_>,
        path: &Path,
        source: String,
        baseline: &RenderedSite,
        images: &ImageCache,
    ) -> Result<Option<(String, rendering::PreviewPage)>, SiteError> {
        let Some(input) = self.pages.get(path) else {
            return Ok(None);
        };
        // Include edits can introduce new subscriptions or change other pages.
        if source.contains("<!--@include")
            || !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
            || !input
                .repository
                .source_location(path, None)
                .is_ok_and(|location| input.repository.path.join(location.path.as_str()) == path)
        {
            return Ok(None);
        }
        let route = &site.routes[&input.identity];
        let page = &site.pages[route];
        let old = page.document.expect("authored page");
        let record = page.resolved.expect("validated authored page");
        let parsed = prepare_collection_document(&source, &input.collection)
            .map_err(|_| SiteError::Evidence)?
            .parsed;
        if !parsed.diagnostics.is_empty()
            || document_title(&parsed.document)
                .as_deref()
                .unwrap_or(&input.relative)
                != page.title
            || parsed.document.frontmatter != old.document.frontmatter
            || validation::document_anchors(&parsed.document.blocks) != record.anchors
        {
            return Ok(None);
        }

        // Previously resolved targets are still valid against this generation.
        // A changed reference list needs resolution and a new input inventory.
        let mut previous = record.references.iter();
        let mut stable = true;
        validation::document_references(
            &parsed.document.blocks,
            None,
            &mut |kind, spelling, _, generated| {
                stable &= !generated
                    && previous.next().is_some_and(|reference| {
                        reference.kind == kind && reference.spelling == spelling
                    });
            },
        );
        if !stable || previous.next().is_some() {
            return Ok(None);
        }
        let components = site.components.keys().map(String::as_str).collect();
        if crate::components::validate_blocks(&parsed.document.blocks, &components).is_err() {
            return Ok(None);
        }

        let mut document = old.clone();
        document.document = parsed.document;
        document.raw_source = Some(source);
        let page = page.with_authored_document(&document, record);
        Ok(
            rendering::render_preview_page(site, route, &page, baseline, images)?
                .map(|page| (route.clone(), page)),
        )
    }
}

const SEARCH_PATH: &str = "assets/search.json";

/// Keep serialized entries so a save only indexes one document.
#[derive(Default)]
pub(super) struct SearchEntries {
    positions: BTreeMap<String, usize>,
    entries: Vec<Vec<u8>>,
}

impl SearchEntries {
    pub(super) fn load(baseline: &RenderedSite) -> Result<Self, SiteError> {
        let mut index = Self::default();
        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&baseline.files()[SEARCH_PATH].bytes)
                .map_err(|_| SiteError::Evidence)?;
        for entry in entries {
            if entry.get("api").and_then(|value| value.as_bool()) == Some(false)
                && let Some(path) = entry.get("path").and_then(|value| value.as_str())
            {
                index.positions.insert(path.into(), index.entries.len());
            }
            index
                .entries
                .push(serde_json::to_vec(&entry).map_err(|_| SiteError::Evidence)?);
        }
        Ok(index)
    }

    pub(super) fn replace(
        &mut self,
        entry: serde_json::Value,
        changes: &mut BTreeMap<String, Arc<RenderedFile>>,
    ) -> Result<(), SiteError> {
        let position = entry
            .get("path")
            .and_then(|value| value.as_str())
            .and_then(|path| self.positions.get(path))
            .ok_or(SiteError::Evidence)?;
        self.entries[*position] = serde_json::to_vec(&entry).map_err(|_| SiteError::Evidence)?;
        let size = self.entries.iter().map(Vec::len).sum::<usize>() + self.entries.len() + 2;
        let mut bytes = Vec::with_capacity(size);
        bytes.push(b'[');
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                bytes.push(b',');
            }
            bytes.extend(entry);
        }
        bytes.push(b']');
        changes.insert(
            SEARCH_PATH.into(),
            Arc::new(RenderedFile {
                bytes,
                media_type: "application/json".into(),
            }),
        );
        Ok(())
    }
}
