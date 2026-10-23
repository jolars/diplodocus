//! Site routes and presentation models built solely from a validated snapshot.

use std::collections::{BTreeMap, BTreeSet};

use crate::configuration::{
    CanonicalUrlStyle, ExecutionMode, PackageVisibility, PresentationDefaults,
};
use crate::diagnostics::DiagnosticPath;
use crate::documents::prepare_collection_document;
use crate::execution::{PreparedCell, ValidatedPage};
use crate::ir::*;
use crate::snapshots::Snapshot;
use crate::validation::{ContentAsset, DocumentIdentity, ReferenceTarget, ResolvedDocument};

#[path = "site_urls.rs"]
mod urls;
pub use urls::UrlMap;

/// Site construction, rendering, or publication failed.
#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    /// Two semantic entities claim one route, or a route is unsafe.
    #[error("site route is invalid or conflicts with another page: {0}")]
    Route(String),
    /// A portable document lacks complete presentation evidence.
    #[error("site presentation evidence is incomplete")]
    Evidence,
    /// Crawler metadata cannot be generated from the supplied presentation settings.
    #[error("site crawler metadata is invalid: {0}")]
    CrawlerMetadata(String),
    /// Homepage and navigation links must identify existing, visible documentation pages.
    #[error("site presentation settings are invalid: {0}")]
    Presentation(String),
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
    pub(crate) presentation: &'a PresentationDefaults,
    pub(crate) logo: Option<&'a AssetReference>,
    pub(crate) open_graph: Option<&'a AssetReference>,
    pub(crate) pages: BTreeMap<String, PageModel<'a>>,
    pub(crate) embedded_pages: BTreeMap<ItemReference, PageModel<'a>>,
    pub(crate) routes: BTreeMap<DocumentIdentity, String>,
    pub(crate) overload_anchors: BTreeMap<ItemReference, String>,
    pub(crate) assets: &'a BTreeMap<String, ContentAsset>,
    pub(crate) base_url: Option<url::Url>,
    pub(crate) canonical_urls: BTreeMap<String, String>,
    pub(crate) url_map: Option<UrlMap>,
}
pub(crate) struct PageModel<'a> {
    pub title: String,
    pub anchor_prefix: Option<String>,
    pub owner: Option<String>,
    pub document: Option<&'a SourcedDocument>,
    pub resolved: Option<&'a ResolvedDocument>,
    pub cells: Vec<PreparedCell>,
    pub mode: ExecutionMode,
    pub executed: Option<&'a ValidatedPage>,
    pub item: Option<&'a Item>,
    pub concept: Option<&'a Concept>,
    pub visible: bool,
    pub navigation: bool,
    pub reference_index: bool,
    pub public_name: Option<String>,
    pub python_class: Option<crate::rendering::python::PythonClass<'a>>,
    pub rust_page: Option<crate::rendering::rust::RustPage<'a>>,
}
impl PageModel<'_> {
    fn empty(title: String, owner: Option<String>, visible: bool) -> Self {
        Self {
            title,
            anchor_prefix: None,
            owner,
            document: None,
            resolved: None,
            cells: Vec::new(),
            mode: ExecutionMode::Never,
            executed: None,
            item: None,
            concept: None,
            visible,
            navigation: true,
            reference_index: false,
            public_name: None,
            python_class: None,
            rust_page: None,
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
            presentation: snapshot.presentation(),
            logo: snapshot.logo(),
            open_graph: snapshot.open_graph(),
            pages: BTreeMap::new(),
            embedded_pages: BTreeMap::new(),
            routes: BTreeMap::new(),
            overload_anchors: BTreeMap::new(),
            assets: snapshot.assets(),
            base_url: snapshot
                .presentation()
                .site_base_url()
                .map_err(|message| SiteError::CrawlerMetadata(message.into()))?,
            canonical_urls: BTreeMap::new(),
            url_map: None,
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
                model.navigation = match collection.navigation {
                    crate::configuration::ContentNavigation::All => true,
                    crate::configuration::ContentNavigation::Index => stem == "index",
                    crate::configuration::ContentNavigation::None => false,
                };
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
                    navigation: collection.navigation,
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
            } else if let PageKind::PackageDocumentation { role } = &page.kind {
                let package = owner.as_ref().ok_or(SiteError::Evidence)?;
                format!(
                    "packages/{}/{}",
                    workspace.packages[package].slug,
                    role.filename()
                )
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
            let rust_owners = crate::rendering::rust::member_owners(package);
            let python_owners = if package.ecosystem == "python" {
                crate::rendering::python::member_owners(package)
            } else {
                BTreeMap::new()
            };
            let private_modules: Vec<_> = package
                .items
                .values()
                .filter(|item| {
                    item.kind == ItemKind::Module
                        && item
                            .qualified_name
                            .split('.')
                            .skip(1)
                            .any(|part| part.starts_with('_'))
                })
                .map(|item| item.qualified_name.as_str())
                .collect();
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
            let r_routes = if package.ecosystem == "r" {
                let reference = format!("{base}/reference/index.html");
                if package.items.values().any(Item::discoverable)
                    && !site.pages.contains_key(&reference)
                {
                    let mut model = PageModel::empty(
                        "Reference".into(),
                        Some(id.clone()),
                        package.visibility != PackageVisibility::Hidden,
                    );
                    model.reference_index = true;
                    site.insert(reference, model)?;
                }
                crate::rendering::r_topics::routes(package)
            } else {
                BTreeMap::new()
            };
            for (item_id, item) in &package.items {
                let identity = DocumentIdentity::Item {
                    item: ItemReference {
                        package: id.clone(),
                        item: item_id.clone(),
                    },
                };
                if let Some(family) = python_overload_family(item) {
                    let owner = python_owners.get(&family.item).unwrap_or(&family.item);
                    site.routes.insert(
                        identity,
                        format!(
                            "packages/{}/reference/{}.html",
                            workspace.packages[&family.package].slug,
                            crate::provenance::fingerprint_bytes(owner.as_bytes()).value
                        ),
                    );
                    site.overload_anchors.insert(
                        ItemReference {
                            package: id.clone(),
                            item: item_id.clone(),
                        },
                        python_overload_anchor(item_id),
                    );
                    continue;
                }
                if let Some(owner) = python_owners.get(item_id) {
                    site.routes.insert(
                        identity,
                        format!(
                            "{base}/reference/{}.html",
                            crate::provenance::fingerprint_bytes(owner.as_bytes()).value
                        ),
                    );
                    site.overload_anchors.insert(
                        ItemReference {
                            package: id.clone(),
                            item: item_id.clone(),
                        },
                        crate::rendering::python::member_anchor(item),
                    );
                    continue;
                }
                let route = r_routes.get(item_id).cloned().unwrap_or_else(|| {
                    format!(
                        "{base}/reference/{}.html",
                        crate::provenance::fingerprint_bytes(item_id.as_bytes()).value
                    )
                });
                let title = if package.ecosystem == "python" {
                    python_display_name(item, &private_modules).to_owned()
                } else if package.ecosystem == "julia" && item.kind == ItemKind::Method {
                    item.signatures
                        .first()
                        .map(|signature| {
                            crate::rendering::signature_title(
                                &item.qualified_name,
                                &signature.signature,
                                "julia",
                            )
                        })
                        .unwrap_or_else(|| item.qualified_name.clone())
                } else if package.ecosystem == "r" {
                    crate::rendering::r_topics::page_title(item)
                        .unwrap_or_else(|| item.qualified_name.clone())
                } else if let Some(data) = crate::rendering::rust::data(item) {
                    data.public_path.clone()
                } else {
                    item.qualified_name.clone()
                };
                let mut model = PageModel::empty(
                    title,
                    Some(id.clone()),
                    package.visibility != PackageVisibility::Hidden && item.discoverable(),
                );
                model.document = item.documentation.as_ref();
                model.resolved = records.get(&identity).copied();
                model.item = Some(item);
                model.public_name = Some(if package.ecosystem == "python" {
                    python_display_name(item, &private_modules).to_owned()
                } else if let Some(data) = crate::rendering::rust::data(item) {
                    data.public_path.clone()
                } else {
                    item.qualified_name.clone()
                });
                if package.ecosystem == "python" {
                    model.python_class =
                        crate::rendering::python::class(id, package, item_id, &records);
                }
                if package.ecosystem == "rust" {
                    if let Some(owner) = rust_owners.get(item_id) {
                        let reference = ItemReference {
                            package: id.clone(),
                            item: item_id.clone(),
                        };
                        let anchor = crate::rendering::rust::member_anchor(package, item_id);
                        site.routes.insert(
                            identity,
                            format!(
                                "{base}/reference/{}.html",
                                crate::provenance::fingerprint_bytes(owner.as_bytes()).value
                            ),
                        );
                        site.overload_anchors
                            .insert(reference.clone(), anchor.clone());
                        model.anchor_prefix = Some(format!("{anchor}-documentation"));
                        site.embedded_pages.insert(reference, model);
                        continue;
                    }
                    model.anchor_prefix = Some("documentation".into());
                    model.rust_page = crate::rendering::rust::page(id, package, item_id, &records);
                }
                if let Some(ItemLanguageData::Julia(data)) = &item.language_data
                    && let JuliaDeclaration::Callable {
                        role: JuliaCallableRole::Method { family, .. },
                        ..
                    } = &data.declaration
                {
                    let reference = ItemReference {
                        package: id.clone(),
                        item: item_id.clone(),
                    };
                    site.routes.insert(
                        identity,
                        format!(
                            "packages/{}/reference/{}.html",
                            workspace.packages[&family.package].slug,
                            crate::provenance::fingerprint_bytes(family.item.as_bytes()).value
                        ),
                    );
                    site.overload_anchors.insert(
                        reference.clone(),
                        format!(
                            "method-{}",
                            crate::provenance::fingerprint_bytes(item_id.as_bytes()).value
                        ),
                    );
                    model.anchor_prefix = site.overload_anchors.get(&reference).cloned();
                    site.embedded_pages.insert(reference, model);
                    continue;
                }
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
                PageModel::empty(site.title().to_owned(), None, true),
            )?;
        }
        if let Some(home) = &site.presentation.home {
            for path in std::iter::once(&home.guide)
                .chain(home.actions.iter().map(|action| &action.path))
                .chain(home.features.iter().map(|feature| &feature.path))
            {
                if path.as_str() == "index.html"
                    || !site
                        .pages
                        .get(path.as_str())
                        .is_some_and(|page| page.visible)
                {
                    return Err(SiteError::Presentation(format!(
                        "homepage destination must name a visible documentation page: {}",
                        path.as_str()
                    )));
                }
            }
            if site.pages[home.guide.as_str()].owner.is_some() {
                return Err(SiteError::Presentation(
                    "presentation.home.guide must name a project documentation page".into(),
                ));
            }
        }
        site.assign_canonical_urls()?;
        let mut section_prefixes: Vec<&str> = Vec::new();
        for section in &site.presentation.sections {
            let prefix = section.prefix();
            if section.label.trim().is_empty()
                || !section.path.as_str().ends_with("/index.html")
                || prefix.is_empty()
                || site.guide_route().starts_with(prefix)
                || !site.pages.get(section.path.as_str()).is_some_and(|page| {
                    page.visible && page.owner.is_none() && page.document.is_some()
                })
                || section_prefixes
                    .iter()
                    .any(|other| prefix.starts_with(other) || other.starts_with(prefix))
            {
                return Err(SiteError::Presentation(format!(
                    "navigation sections need a label and a visible project index page in a distinct directory: {}",
                    section.path.as_str()
                )));
            }
            section_prefixes.push(prefix);
        }
        Ok(site)
    }

    fn assign_canonical_urls(&mut self) -> Result<(), SiteError> {
        let mut assigned = BTreeSet::new();
        for route in self.pages.keys() {
            let public_route = match self.presentation.canonical_url_style {
                CanonicalUrlStyle::File => route.as_str(),
                CanonicalUrlStyle::Clean => {
                    let public_route = clean_route(route);
                    let file_alias = format!("{}.html", public_route.trim_end_matches('/'));
                    let index_alias = format!("{}/index.html", public_route.trim_end_matches('/'));
                    if [file_alias, index_alias]
                        .iter()
                        .any(|alias| alias != route && self.pages.contains_key(alias))
                        || matches!(
                            public_route.trim_end_matches('/'),
                            "sitemap.xml" | "robots.txt"
                        )
                    {
                        return Err(SiteError::Route(route.clone()));
                    }
                    public_route
                }
            };
            let Some(base) = &self.base_url else {
                continue;
            };
            let url = base
                .join(&encode_url_path(public_route))
                .map_err(|error| SiteError::CrawlerMetadata(error.to_string()))?;
            if !assigned.insert(url.clone()) {
                return Err(SiteError::Route(route.clone()));
            }
            self.canonical_urls.insert(route.clone(), url.into());
        }
        Ok(())
    }

    /// Map generated document and asset routes into an existing website.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown artifact paths, unsafe URLs, or colliding
    /// destinations. Unmapped paths retain their default relative URLs.
    pub fn with_url_map(mut self, map: UrlMap) -> Result<Self, SiteError> {
        map.validate(&self)?;
        self.url_map = Some(map);
        Ok(self)
    }

    pub(crate) fn url(&self, from: &str, to: &str) -> String {
        if let Some(map) = &self.url_map {
            return map.url(from, to);
        }
        let public = |route| {
            if self.presentation.canonical_url_style == CanonicalUrlStyle::Clean
                && self.pages.contains_key(route)
            {
                clean_route(route)
            } else {
                route
            }
        };
        let (path, fragment) = to.split_once('#').map_or((to, None), |(p, f)| (p, Some(f)));
        let mut url = relative_url(public(from), public(path));
        if url.is_empty() {
            url.push_str("./");
        }
        if let Some(fragment) = fragment {
            url.push('#');
            url.push_str(fragment);
        }
        url
    }
    pub(crate) fn title(&self) -> &str {
        self.presentation
            .title
            .as_deref()
            .unwrap_or(&self.workspace.name)
    }
    pub(crate) fn home(&self, route: &str) -> Option<&crate::configuration::HomeDefaults> {
        (route == "index.html")
            .then_some(self.presentation.home.as_ref())
            .flatten()
    }
    pub(crate) fn guide_route(&self) -> &str {
        self.presentation
            .home
            .as_ref()
            .map_or("index.html", |home| home.guide.as_str())
    }
    pub(crate) fn project_section(
        &self,
        route: &str,
    ) -> Option<&crate::configuration::NavigationSection> {
        self.presentation.sections.iter().find(|section| {
            self.pages[route].owner.is_none() && route.starts_with(section.prefix())
        })
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
            ReferenceTarget::Repository { .. } => {
                return crate::validation::repository_target_url(self.workspace, target)
                    .ok_or(SiteError::Evidence);
            }
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
                self.overload_anchors.get(item).map(String::as_str),
            ),
            ReferenceTarget::Asset { asset, fragment } => (
                self.asset_route(&asset.fingerprint.value)?,
                fragment.as_deref(),
            ),
        };
        let mut url = self.url(from, &route);
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

fn clean_route(route: &str) -> &str {
    if route == "index.html" {
        ""
    } else if route.ends_with("/index.html") {
        route.strip_suffix("index.html").unwrap()
    } else {
        route.strip_suffix(".html").unwrap_or(route)
    }
}

fn python_overload_family(item: &Item) -> Option<&ItemReference> {
    if let Some(ItemLanguageData::Python(data)) = &item.language_data
        && let PythonDeclaration::Callable {
            role: PythonCallableRole::Overload { family },
            ..
        } = &data.declaration
    {
        Some(family)
    } else {
        None
    }
}

fn python_display_name<'a>(item: &'a Item, private_modules: &[&str]) -> &'a str {
    std::iter::once(item.qualified_name.as_str())
        .chain(
            item.aliases
                .iter()
                .filter(|alias| alias.kind == ItemAliasKind::PythonReexport)
                .map(|alias| alias.qualified_name.as_str()),
        )
        .min_by_key(|name| {
            // Class and constructor names may start with underscores even on
            // an explicitly exported API, so only source module paths rank lower.
            let private = private_modules.iter().any(|module| {
                *name == *module
                    || name
                        .strip_prefix(*module)
                        .is_some_and(|suffix| suffix.starts_with('.'))
            });
            (
                private,
                name.split('.').count(),
                *name != item.qualified_name.as_str(),
                *name,
            )
        })
        .expect("the canonical name is always a candidate")
}

pub(crate) fn python_overload_anchor(item_id: &str) -> String {
    format!(
        "overload-{}",
        crate::provenance::fingerprint_bytes(item_id.as_bytes()).value
    )
}

fn encode_url_path(path: &str) -> String {
    path.split('/').map(encode).collect::<Vec<_>>().join("/")
}

pub(crate) fn encode(value: &str) -> String {
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(value, SEGMENT).to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overload_references_target_signatures_on_the_family_page() {
        let root = tempfile::tempdir().unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/python-overloads");
        for path in [
            "diplodocus.yaml",
            "python/pyproject.toml",
            "python/demo/__init__.py",
            "python/demo/_api.py",
            "python/demo/native.pyi",
        ] {
            let destination = root.path().join(path);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(fixture.join(path), destination).unwrap();
        }
        let sources =
            crate::assembly::assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let resolved = crate::validation::resolve_workspace(&sources).unwrap();
        let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
        let site = Site::new(&snapshot).unwrap();
        let rendered = crate::rendering::render_site(&site).unwrap();
        for (id, item) in &site.workspace.packages["python"].items {
            let Some(family) = python_overload_family(item) else {
                continue;
            };
            let reference = ItemReference {
                package: "python".into(),
                item: id.clone(),
            };
            let route = &site.routes[&DocumentIdentity::Item {
                item: family.clone(),
            }];
            let target = site
                .target("index.html", &ReferenceTarget::Item { item: reference })
                .unwrap();
            let (destination, fragment) = target.split_once('#').unwrap();
            assert_eq!(destination, relative_url("index.html", route));
            let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
            let fragment = percent_encoding::percent_decode_str(fragment)
                .decode_utf8()
                .unwrap();
            assert!(html.contains(&format!("id=\"{fragment}\"")));
            let family_target = site
                .target(
                    "index.html",
                    &ReferenceTarget::Item {
                        item: family.clone(),
                    },
                )
                .unwrap();
            let expected = if let Some(anchor) = site.overload_anchors.get(family) {
                format!("{}#{}", relative_url("index.html", route), encode(anchor))
            } else {
                relative_url("index.html", route)
            };
            assert_eq!(family_target, expected);
        }
        assert_eq!(site.overload_anchors.len(), 8);
    }
}
