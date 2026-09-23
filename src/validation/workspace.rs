use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{references, relationships};
use crate::assembly::{AssemblyError, ExecutedWorkspace, WorkspaceSources};
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};
use crate::execution::assets::{validate_authored_image_bytes, validate_image_bytes};
use crate::ir::*;
use crate::provenance::fingerprint_bytes;

mod walk;

/// A document's semantic owner, independent of its rendered URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentIdentity {
    /// An authored or assembled page.
    Page {
        /// Workspace page ID.
        page: String,
    },
    /// Documentation attached to an API item.
    Item {
        /// Package-scoped item identity.
        item: ItemReference,
    },
    /// Documentation attached to an explicit concept.
    Concept {
        /// Workspace concept ID.
        concept: String,
    },
}

/// Portable destination of a resolved authored reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferenceTarget {
    /// Canonical API item, including callable-family identity.
    Item {
        /// Package-scoped identity.
        item: ItemReference,
    },
    /// Another declared page or an anchor in it.
    Page {
        /// Semantic page ID.
        page: String,
        /// Decoded authored anchor.
        fragment: Option<String>,
    },
    /// An anchor within the containing document.
    Anchor {
        /// Decoded authored anchor.
        fragment: String,
    },
    /// Bytes retained in the snapshot, without a checkout dependency.
    Asset {
        /// Content-addressed portable asset.
        asset: AssetReference,
        /// Optional decoded fragment, such as a PDF page.
        fragment: Option<String>,
    },
    /// An explicitly allowed external hyperlink, never fetched.
    External {
        /// Original URL accepted by the URL parser.
        url: String,
    },
}

/// Reference syntax determines how a generator presents its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceKind {
    /// Code-only semantic reference.
    Semantic,
    /// Ordinary or automatic hyperlink.
    Link,
    /// Inline image.
    Image,
}

/// One reference in document traversal order, including generated Markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedReference {
    /// Authored syntax category.
    pub kind: ReferenceKind,
    /// Literal target retained in the document tree.
    pub spelling: String,
    /// Portable destination.
    pub target: ReferenceTarget,
}

/// Resolved metadata for a document, kept separate from its unchanged syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDocument {
    /// Semantic document owner.
    pub document: DocumentIdentity,
    /// Selected collection-relative filename, independent of canonical source aliases.
    pub collection_path: Option<DiagnosticPath>,
    /// Authored identifiers only; generated headings never add anchors.
    pub anchors: BTreeSet<String>,
    /// References in depth-first document order.
    pub references: Vec<ResolvedReference>,
}

/// Exact local bytes available for a later snapshot transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentAsset {
    /// Digest of the exact stored bytes.
    pub fingerprint: Fingerprint,
    /// Validated image type, or `application/octet-stream` for a download.
    pub media_type: String,
    /// Owned bytes; generation need never open a checkout.
    pub bytes: Vec<u8>,
}

/// Reference and asset observations; this is not an HTML rendering trust token.
#[derive(Debug)]
pub struct ResolvedWorkspace {
    records: Vec<ResolvedDocument>,
    assets: BTreeMap<String, ContentAsset>,
    diagnostics: BTreeSet<Diagnostic>,
    inputs: BTreeMap<PathBuf, AssetObservation>,
}

#[derive(Debug, PartialEq, Eq)]
struct AssetObservation {
    path: PathBuf,
    boundary: PathBuf,
    fingerprint: Fingerprint,
}
impl ResolvedWorkspace {
    /// Portable records in semantic document order.
    pub fn records(&self) -> &[ResolvedDocument] {
        &self.records
    }
    /// Deduplicated bytes keyed by SHA-256 digest.
    pub fn assets(&self) -> &BTreeMap<String, ContentAsset> {
        &self.assets
    }
    /// All source and resolution warnings in deterministic order.
    pub fn diagnostics(&self) -> &BTreeSet<Diagnostic> {
        &self.diagnostics
    }
    /// Local assets consumed by this resolution, for declared-input watching.
    pub fn input_paths(&self) -> impl Iterator<Item = &Path> {
        self.inputs.keys().map(PathBuf::as_path)
    }
    /// Check selected asset containment and bytes again before publication.
    pub fn revalidate(&self) -> Result<(), AssemblyError> {
        for (selected, observation) in &self.inputs {
            let AssetObservation {
                path,
                boundary,
                fingerprint,
            } = observation;
            if fs::canonicalize(selected)? != *path {
                return Err(AssemblyError::InputsChanged);
            }
            let resolved = crate::paths::resolve_input_file(
                boundary,
                "asset",
                path.strip_prefix(boundary)
                    .map_err(|_| AssemblyError::InputsChanged)?,
                boundary,
            )?;
            if resolved != *path || fingerprint_bytes(&fs::read(path)?) != *fingerprint {
                return Err(AssemblyError::InputsChanged);
            }
        }
        Ok(())
    }
}

/// Resolution errors, retaining all deterministic source diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    /// Input observations changed or became unavailable.
    #[error(transparent)]
    Inputs(#[from] AssemblyError),
    /// At least one reference or relationship is invalid.
    #[error("workspace reference validation failed")]
    Diagnostics(Vec<Diagnostic>),
}
impl ResolutionError {
    /// Portable diagnostics, when the failure came from reference validation.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics(diagnostics) => diagnostics,
            _ => &[],
        }
    }
}

/// Resolve static documents and collect checked-in assets without creating files.
///
/// This operation never discovers a kernel, runs authored code, reads an
/// execution cache, or publishes output. It checks only outputs already present.
pub fn resolve_workspace(sources: &WorkspaceSources) -> Result<ResolvedWorkspace, ResolutionError> {
    resolve(sources, BTreeMap::new())
}

/// Resolve executed documents and copy their verified final staged asset bytes.
///
/// The caller retains the execution owner until this operation and subsequent
/// publication finish. Serialized output remains untrusted on future loading.
pub fn resolve_executed_workspace(
    executed: &ExecutedWorkspace,
) -> Result<ResolvedWorkspace, ResolutionError> {
    let mut generated = BTreeMap::new();
    for result in executed.executed_pages().values() {
        for staged in result.staged_assets() {
            let asset = result
                .validated()
                .referenced_assets()
                .find(|asset| asset.reference == staged.reference)
                .ok_or(AssemblyError::InputsChanged)?;
            let bytes = fs::read(&staged.path).map_err(AssemblyError::from)?;
            if fingerprint_bytes(&bytes) != asset.reference.fingerprint
                || bytes.len() as u64 != asset.byte_size
                || validate_image_bytes(&asset.media_type, &bytes).is_err()
            {
                return Err(AssemblyError::InputsChanged.into());
            }
            generated.insert(
                asset.reference.path.clone(),
                ContentAsset {
                    fingerprint: asset.reference.fingerprint.clone(),
                    media_type: asset.media_type.clone(),
                    bytes,
                },
            );
        }
    }
    resolve(executed.sources(), generated)
}

struct DocumentContext<'a> {
    identity: DocumentIdentity,
    owner: Option<&'a str>,
    document: &'a SourcedDocument,
    path: Option<PathBuf>,
    entity: DiagnosticEntity,
}

fn resolve(
    sources: &WorkspaceSources,
    generated: BTreeMap<DiagnosticPath, ContentAsset>,
) -> Result<ResolvedWorkspace, ResolutionError> {
    sources.revalidate()?;
    let workspace = sources.workspace();
    let mut documents = Vec::new();
    let mut pages: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let path_for = |source: &SourceLocation| {
        sources
            .paths()
            .repositories
            .iter()
            .find(|r| r.id == source.repository)
            .map(|r| r.path.join(source.path.as_str()))
    };
    for (id, page) in &workspace.pages {
        let path = page.document.source_location.as_ref().and_then(path_for);
        if let Some(path) = &path {
            pages.entry(path.clone()).or_default().push(id.clone());
        }
        let prepared = sources.prepared_pages().get(id);
        let entity =
            if let (PageKind::Authored { collection }, Some(prepared)) = (&page.kind, prepared) {
                DiagnosticEntity::Document {
                    collection: collection.clone(),
                    path: prepared.relative.clone(),
                }
            } else {
                DiagnosticEntity::Project
            };
        documents.push(DocumentContext {
            identity: DocumentIdentity::Page { page: id.clone() },
            owner: match &page.owner {
                ContentOwner::Project => None,
                ContentOwner::Package { package } => Some(package),
            },
            document: &page.document,
            path,
            entity,
        });
    }
    for (package, record) in &workspace.packages {
        for (item, record) in &record.items {
            if let Some(document) = &record.documentation {
                documents.push(DocumentContext {
                    identity: DocumentIdentity::Item {
                        item: ItemReference {
                            package: package.clone(),
                            item: item.clone(),
                        },
                    },
                    owner: Some(package),
                    document,
                    path: document.source_location.as_ref().and_then(path_for),
                    entity: DiagnosticEntity::Item {
                        package: package.clone(),
                        id: item.clone(),
                    },
                });
            }
        }
    }
    for (concept, record) in &workspace.concepts {
        if let Some(document) = &record.documentation {
            documents.push(DocumentContext {
                identity: DocumentIdentity::Concept {
                    concept: concept.clone(),
                },
                owner: None,
                document,
                path: document.source_location.as_ref().and_then(path_for),
                entity: DiagnosticEntity::Concept {
                    id: concept.clone(),
                },
            });
        }
    }
    documents.sort_by(|a, b| a.identity.cmp(&b.identity));
    let anchors: BTreeMap<_, _> = documents
        .iter()
        .map(|d| {
            (
                d.identity.clone(),
                walk::anchors(&d.document.document.blocks),
            )
        })
        .collect();
    let mut resolution = Resolver {
        sources,
        pages,
        anchors,
        generated,
        result: ResolvedWorkspace {
            records: vec![],
            assets: BTreeMap::new(),
            diagnostics: workspace.diagnostics.clone(),
            inputs: BTreeMap::new(),
        },
    };
    resolution
        .result
        .diagnostics
        .extend(relationships::validate(workspace));
    for context in documents {
        let mut references = Vec::new();
        walk::references(
            &context.document.document.blocks,
            None,
            &mut |kind, spelling, span, generated| match resolution
                .reference(&context, kind, spelling, generated)
            {
                Ok(target) => references.push(ResolvedReference {
                    kind,
                    spelling: spelling.into(),
                    target,
                }),
                Err(code) => {
                    let mut diagnostic = Diagnostic::new(
                        code,
                        Severity::Error,
                        format!(
                            "Cannot resolve {kind:?} target `{spelling}` unambiguously and safely."
                        ),
                    )
                    .with_entity(context.entity.clone());
                    diagnostic.span = Some(span);
                    diagnostic.source = context.document.source_location.as_ref().map(|s| {
                        DiagnosticSource::Repository {
                            repository: s.repository.clone(),
                            path: s.path.clone(),
                        }
                    });
                    resolution.result.diagnostics.insert(diagnostic);
                }
            },
        );
        resolution.result.records.push(ResolvedDocument {
            collection_path: match &context.identity {
                DocumentIdentity::Page { page } => sources
                    .prepared_pages()
                    .get(page)
                    .map(|p| p.relative.clone()),
                _ => None,
            },
            anchors: resolution
                .anchors
                .get(&context.identity)
                .cloned()
                .unwrap_or_default(),
            document: context.identity,
            references,
        });
    }
    // Rich alternatives can own assets without an inline Markdown reference.
    for asset in resolution.generated.values() {
        insert_asset(&mut resolution.result.assets, asset.clone())
            .map_err(|_| AssemblyError::InputsChanged)?;
    }
    if resolution
        .result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(ResolutionError::Diagnostics(
            resolution.result.diagnostics.into_iter().collect(),
        ));
    }
    sources.revalidate()?;
    resolution.result.revalidate()?;
    Ok(resolution.result)
}

struct Resolver<'a> {
    sources: &'a WorkspaceSources,
    pages: BTreeMap<PathBuf, Vec<String>>,
    anchors: BTreeMap<DocumentIdentity, BTreeSet<String>>,
    generated: BTreeMap<DiagnosticPath, ContentAsset>,
    result: ResolvedWorkspace,
}

impl Resolver<'_> {
    fn reference(
        &mut self,
        context: &DocumentContext<'_>,
        kind: ReferenceKind,
        spelling: &str,
        generated: bool,
    ) -> Result<ReferenceTarget, DiagnosticCode> {
        let invalid = DiagnosticCode::UnresolvedDocumentReference;
        if kind == ReferenceKind::Semantic {
            return references::resolve_item(self.sources.workspace(), context.owner, spelling)
                .map(|item| ReferenceTarget::Item { item });
        }
        if spelling.is_empty() || spelling.chars().any(|c| c.is_control() || c == '\\') {
            return Err(invalid);
        }
        let decoded = percent_encoding::percent_decode_str(spelling)
            .decode_utf8()
            .map_err(|_| invalid)?;
        if decoded.chars().any(|c| c.is_control() || c == '\\') {
            return Err(invalid);
        }
        if let Ok(url) = url::Url::parse(spelling) {
            if kind != ReferenceKind::Image
                && matches!(url.scheme(), "https" | "http" | "mailto")
                && url.username().is_empty()
                && url.password().is_none()
            {
                return Ok(ReferenceTarget::External {
                    url: spelling.into(),
                });
            }
            return Err(invalid);
        }
        if decoded.starts_with('/') || spelling.contains('?') || url::Url::parse(&decoded).is_ok() {
            return Err(invalid);
        }
        let (file, fragment) = spelling
            .split_once('#')
            .map_or((spelling, None), |(f, anchor)| (f, Some(anchor)));
        let file = percent_encoding::percent_decode_str(file)
            .decode_utf8()
            .map_err(|_| invalid)?;
        let fragment = fragment
            .map(|f| {
                percent_encoding::percent_decode_str(f)
                    .decode_utf8()
                    .map(|s| s.into_owned())
            })
            .transpose()
            .map_err(|_| invalid)?;
        if file.is_empty() {
            let fragment = fragment.filter(|f| !f.is_empty()).ok_or(invalid)?;
            if kind == ReferenceKind::Image
                || !self
                    .anchors
                    .get(&context.identity)
                    .is_some_and(|a| a.contains(&fragment))
            {
                return Err(invalid);
            }
            return Ok(ReferenceTarget::Anchor { fragment });
        }
        if generated {
            let path = DiagnosticPath::try_from(file.as_ref()).map_err(|_| invalid)?;
            let asset = self.generated.get(&path).ok_or(invalid)?;
            if fragment.is_some() {
                return Err(invalid);
            }
            return Ok(ReferenceTarget::Asset {
                asset: AssetReference {
                    path,
                    fingerprint: asset.fingerprint.clone(),
                },
                fragment: None,
            });
        }
        let source = context.path.as_ref().ok_or(invalid)?;
        let candidate = source.parent().ok_or(invalid)?.join(file.as_ref());
        let canonical = fs::canonicalize(&candidate).map_err(|_| invalid)?;
        if kind != ReferenceKind::Image
            && let Some(ids) = self.pages.get(&canonical)
        {
            let [page] = ids.as_slice() else {
                return Err(invalid);
            };
            if let Some(fragment) = &fragment
                && !fragment.is_empty()
                && !self
                    .anchors
                    .get(&DocumentIdentity::Page { page: page.clone() })
                    .is_some_and(|a| a.contains(fragment))
            {
                return Err(invalid);
            }
            return Ok(ReferenceTarget::Page {
                page: page.clone(),
                fragment,
            });
        }
        // Unselected source documents cannot silently become downloadable pages.
        if matches!(
            canonical.extension().and_then(|s| s.to_str()),
            Some("md" | "qmd")
        ) {
            return Err(invalid);
        }
        let boundary = self
            .sources
            .paths()
            .content
            .iter()
            .map(|c| &c.path)
            .chain(self.sources.paths().packages.iter().map(|p| &p.path))
            .filter(|root| canonical.starts_with(root))
            .max_by_key(|root| root.components().count())
            .ok_or(invalid)?;
        let path = crate::paths::resolve_input_file(
            boundary,
            "asset",
            canonical.strip_prefix(boundary).map_err(|_| invalid)?,
            boundary,
        )
        .map_err(|_| invalid)?;
        let bytes = fs::read(&path).map_err(|_| invalid)?;
        let media_type = match path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpeg" | "jpg") => "image/jpeg",
            Some("svg") => "image/svg+xml",
            _ => "application/octet-stream",
        };
        if (kind == ReferenceKind::Image || media_type.starts_with("image/"))
            && validate_authored_image_bytes(media_type, &bytes).is_err()
        {
            return Err(invalid);
        }
        let fingerprint = fingerprint_bytes(&bytes);
        let asset = AssetReference {
            path: DiagnosticPath::try_from(format!("content-assets/sha256/{}", fingerprint.value))
                .map_err(|_| invalid)?,
            fingerprint: fingerprint.clone(),
        };
        insert_asset(
            &mut self.result.assets,
            ContentAsset {
                fingerprint: fingerprint.clone(),
                media_type: media_type.into(),
                bytes,
            },
        )?;
        let observation = AssetObservation {
            path,
            boundary: boundary.clone(),
            fingerprint,
        };
        if self
            .result
            .inputs
            .get(&candidate)
            .is_some_and(|old| old != &observation)
        {
            return Err(invalid);
        }
        self.result.inputs.insert(candidate, observation);
        Ok(ReferenceTarget::Asset { asset, fragment })
    }
}

fn insert_asset(
    assets: &mut BTreeMap<String, ContentAsset>,
    asset: ContentAsset,
) -> Result<(), DiagnosticCode> {
    if let Some(previous) = assets.get_mut(&asset.fingerprint.value) {
        if previous.bytes != asset.bytes {
            return Err(DiagnosticCode::UnresolvedDocumentReference);
        }
        if previous.media_type == "application/octet-stream" {
            previous.media_type = asset.media_type;
        } else if asset.media_type != "application/octet-stream"
            && previous.media_type != asset.media_type
        {
            return Err(DiagnosticCode::UnresolvedDocumentReference);
        }
    } else {
        assets.insert(asset.fingerprint.value.clone(), asset);
    }
    Ok(())
}
