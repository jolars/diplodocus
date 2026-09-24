//! Site routes and presentation models built solely from a validated snapshot.

use std::collections::BTreeMap;

use crate::configuration::{ExecutionMode, PackageVisibility};
use crate::diagnostics::DiagnosticPath;
use crate::documents::prepare_collection_document;
use crate::execution::{PreparedCell, ValidatedPage};
use crate::ir::*;
use crate::snapshots::Snapshot;
use crate::validation::{ContentAsset, DocumentIdentity, ReferenceTarget, ResolvedDocument};

/// Site construction, rendering, or publication failed.
#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    /// Two semantic entities claim one route, or a route is unsafe.
    #[error("site route is invalid or conflicts with another page: {0}")]
    Route(String),
    /// A portable document lacks complete presentation evidence.
    #[error("site presentation evidence is incomplete")]
    Evidence,
    /// The output directory could not be staged or replaced.
    #[error("site publication failed: {0}")]
    Io(#[from] std::io::Error),
    /// The directory contains files not owned by a previous site publication.
    #[error("output is not an empty directory or a Diplodocus site")]
    UnownedOutput,
}

/// Complete generation inputs, independent of databases and source checkouts.
pub struct Site<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) pages: BTreeMap<String, PageModel<'a>>,
    pub(crate) routes: BTreeMap<DocumentIdentity, String>,
    pub(crate) assets: &'a BTreeMap<String, ContentAsset>,
}
pub(crate) struct PageModel<'a> {
    pub title: String,
    pub owner: Option<String>,
    pub document: Option<&'a SourcedDocument>,
    pub resolved: Option<&'a ResolvedDocument>,
    pub cells: Vec<PreparedCell>,
    pub mode: ExecutionMode,
    pub executed: Option<&'a ValidatedPage>,
    pub item: Option<&'a Item>,
    pub concept: Option<&'a Concept>,
    pub visible: bool,
}
impl PageModel<'_> {
    fn empty(title: String, owner: Option<String>, visible: bool) -> Self {
        Self {
            title,
            owner,
            document: None,
            resolved: None,
            cells: Vec::new(),
            mode: ExecutionMode::Never,
            executed: None,
            item: None,
            concept: None,
            visible,
        }
    }
}

impl<'a> Site<'a> {
    /// Assign collision-checked routes and normalize authored cell presentation.
    /// No source files, database connections, or language runtimes are used.
    pub fn new(snapshot: &'a Snapshot) -> Result<Self, SiteError> {
        let workspace = snapshot.workspace();
        let mut site = Self {
            workspace,
            pages: BTreeMap::new(),
            routes: BTreeMap::new(),
            assets: snapshot.assets(),
        };
        let records: BTreeMap<_, _> = snapshot
            .documents()
            .iter()
            .map(|r| (&r.document, r))
            .collect();
        for (id, page) in &workspace.pages {
            let identity = DocumentIdentity::Page { page: id.clone() };
            let resolved = *records.get(&identity).ok_or(SiteError::Evidence)?;
            let owner = match &page.owner {
                ContentOwner::Project => None,
                ContentOwner::Package { package } => Some(package.clone()),
            };
            let visible = owner
                .as_ref()
                .is_none_or(|p| workspace.packages[p].visibility != PackageVisibility::Hidden);
            let mut model = PageModel::empty(page.title.clone(), owner.clone(), visible);
            model.document = Some(&page.document);
            model.resolved = Some(resolved);
            model.executed = snapshot.executed_page(id);
            let route = if let PageKind::Authored { collection } = &page.kind {
                let collection = &workspace.content_collections[collection];
                let path = resolved
                    .collection_path
                    .as_ref()
                    .ok_or(SiteError::Evidence)?
                    .as_str();
                let (stem, _) = path.rsplit_once('.').ok_or(SiteError::Evidence)?;
                let base = owner
                    .as_ref()
                    .map(|p| format!("packages/{}/", workspace.packages[p].slug))
                    .unwrap_or_default();
                let mount = if collection.mount.is_empty() {
                    String::new()
                } else {
                    format!("{}/", collection.mount)
                };
                model.mode = collection.execution.mode;
                let config = crate::configuration::ContentConfiguration {
                    id: match &page.kind {
                        PageKind::Authored { collection } => collection.clone(),
                        _ => unreachable!(),
                    },
                    owner: owner.clone().unwrap_or_else(|| "project".into()),
                    repository: collection.repository.clone(),
                    path: ".".into(),
                    mount: collection.mount.clone(),
                    format: collection.format,
                    execution: crate::configuration::ExecutionConfiguration {
                        mode: collection.execution.mode,
                        engine: collection.execution.engine,
                        kernel: collection.execution.kernel.clone(),
                        declared_environment_inputs: Vec::new(),
                    },
                };
                let raw = page
                    .document
                    .raw_source
                    .as_deref()
                    .ok_or(SiteError::Evidence)?;
                let prepared =
                    prepare_collection_document(raw, &config).map_err(|_| SiteError::Evidence)?;
                if prepared
                    .parsed
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == crate::diagnostics::Severity::Error)
                {
                    return Err(SiteError::Evidence);
                }
                model.cells = prepared.preparation.map(|p| p.cells).unwrap_or_default();
                format!("{base}{mount}{stem}.html")
            } else {
                format!(
                    "pages/{}.html",
                    crate::provenance::fingerprint_bytes(id.as_bytes()).value
                )
            };
            site.insert(route.clone(), model)?;
            site.routes.insert(identity, route);
        }
        for (id, package) in &workspace.packages {
            let base = format!("packages/{}", package.slug);
            let route = format!("{base}/index.html");
            if !site.pages.contains_key(&route) {
                site.insert(
                    route,
                    PageModel::empty(
                        package.name.clone(),
                        Some(id.clone()),
                        package.visibility != PackageVisibility::Hidden,
                    ),
                )?;
            }
            for (item_id, item) in &package.items {
                let identity = DocumentIdentity::Item {
                    item: ItemReference {
                        package: id.clone(),
                        item: item_id.clone(),
                    },
                };
                let route = format!(
                    "{base}/reference/{}.html",
                    crate::provenance::fingerprint_bytes(item_id.as_bytes()).value
                );
                let mut model = PageModel::empty(
                    item.qualified_name.clone(),
                    Some(id.clone()),
                    package.visibility != PackageVisibility::Hidden,
                );
                model.document = item.documentation.as_ref();
                model.resolved = records.get(&identity).copied();
                model.item = Some(item);
                site.insert(route.clone(), model)?;
                site.routes.insert(identity, route);
            }
        }
        for (id, concept) in &workspace.concepts {
            let identity = DocumentIdentity::Concept {
                concept: id.clone(),
            };
            let route = format!(
                "concepts/{}.html",
                crate::provenance::fingerprint_bytes(id.as_bytes()).value
            );
            let mut model = PageModel::empty(id.clone(), None, true);
            model.document = concept.documentation.as_ref();
            model.resolved = records.get(&identity).copied();
            model.concept = Some(concept);
            site.insert(route.clone(), model)?;
            site.routes.insert(identity, route);
        }
        if !site.pages.contains_key("index.html") {
            site.insert(
                "index.html".into(),
                PageModel::empty(workspace.name.clone(), None, true),
            )?;
        }
        Ok(site)
    }
    fn insert(&mut self, route: String, page: PageModel<'a>) -> Result<(), SiteError> {
        if DiagnosticPath::try_from(route.clone()).is_err()
            || self.pages.contains_key(&route)
            || route.starts_with("assets/")
        {
            return Err(SiteError::Route(route));
        }
        self.pages.insert(route, page);
        Ok(())
    }
    pub(crate) fn target(&self, from: &str, target: &ReferenceTarget) -> Result<String, SiteError> {
        let (route, fragment) = match target {
            ReferenceTarget::External { url } => return Ok(url.clone()),
            ReferenceTarget::Anchor { fragment } => return Ok(format!("#{}", encode(fragment))),
            ReferenceTarget::Page { page, fragment } => (
                self.routes
                    .get(&DocumentIdentity::Page { page: page.clone() })
                    .ok_or(SiteError::Evidence)?
                    .clone(),
                fragment.as_deref(),
            ),
            ReferenceTarget::Item { item } => (
                self.routes
                    .get(&DocumentIdentity::Item { item: item.clone() })
                    .ok_or(SiteError::Evidence)?
                    .clone(),
                None,
            ),
            ReferenceTarget::Asset { asset, fragment } => (
                self.asset_route(&asset.fingerprint.value)?,
                fragment.as_deref(),
            ),
        };
        let mut url = relative_url(from, &route);
        if let Some(fragment) = fragment {
            url.push('#');
            url.push_str(&encode(fragment));
        }
        Ok(url)
    }
    pub(crate) fn asset_route(&self, digest: &str) -> Result<String, SiteError> {
        let asset = self.assets.get(digest).ok_or(SiteError::Evidence)?;
        let extension = match asset.media_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/svg+xml" => "svg",
            _ => "bin",
        };
        Ok(format!("assets/{digest}.{extension}"))
    }
}

pub(crate) fn encode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}
pub(crate) fn relative_url(from: &str, to: &str) -> String {
    let a: Vec<_> = from.split('/').collect();
    let b: Vec<_> = to.split('/').collect();
    let common = a[..a.len() - 1]
        .iter()
        .zip(&b)
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = "../".repeat(a.len() - 1 - common);
    result.push_str(
        &b[common..]
            .iter()
            .map(|s| encode(s))
            .collect::<Vec<_>>()
            .join("/"),
    );
    result
}
