use serde::{Deserialize, Serialize};

use crate::site::{PageModel, Site, SiteError};
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

/// Artifact paths and routing metadata available before fragment rendering.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentInventory {
    /// Inventory envelope version. The supported version is `1`.
    pub format_version: u32,
    /// Documents in deterministic artifact-path order.
    pub documents: Vec<FragmentInventoryDocument>,
    /// Sorted relative asset artifact paths accepted as URL-map keys.
    pub assets: Vec<String>,
}

/// Routing metadata for one snapshot-backed fragment document.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentInventoryDocument {
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
}

impl FragmentInventoryDocument {
    pub(super) fn new(
        site: &Site<'_>,
        route: &str,
        page: &PageModel<'_>,
        markdown_path: String,
    ) -> Self {
        let identities: Vec<_> = site
            .routes
            .iter()
            .filter(|(_, path)| path.as_str() == route)
            .map(|(identity, _)| identity.clone())
            .collect();
        Self {
            path: route.into(),
            markdown_path,
            url: site.url("index.html", route),
            title: page.title.clone(),
            package: page.owner.clone(),
            package_version: page
                .owner
                .as_ref()
                .and_then(|id| site.workspace.packages[id].version.clone()),
            qualified_name: page.item.map(|item| item.qualified_name.clone()),
            public_name: page.public_name.clone(),
            visible: page.visible,
            identities,
        }
    }

    pub(super) fn rendered(self, headings: Vec<(usize, String, String)>) -> FragmentDocument {
        FragmentDocument {
            path: self.path,
            markdown_path: self.markdown_path,
            url: self.url,
            title: self.title,
            package: self.package,
            package_version: self.package_version,
            qualified_name: self.qualified_name,
            public_name: self.public_name,
            visible: self.visible,
            identities: self.identities,
            headings,
        }
    }
}

/// Discover the exact fragment document and asset paths without rendering pages.
///
/// # Errors
/// Returns an error if the prepared site has invalid document or asset evidence.
pub fn fragment_inventory(site: &Site<'_>) -> Result<FragmentInventory, SiteError> {
    let documents = site
        .pages
        .iter()
        .map(|(route, page)| {
            Ok(FragmentInventoryDocument::new(
                site,
                route,
                page,
                markdown_path(route)?,
            ))
        })
        .collect::<Result<_, SiteError>>()?;
    let images = super::images::Images::default();
    super::fragment_images::prepare(site, &images)?;
    let mut assets =
        std::collections::BTreeSet::from([CONTENT_CSS.into(), TABSETS.into(), SEARCH_INDEX.into()]);
    assets.extend(images.into_files().map(|(path, _)| path));
    for digest in site.assets.keys() {
        assets.insert(site.asset_route(digest)?);
    }
    assets.extend(site.public_assets.keys().cloned());
    if site.pages.values().any(|page| page_uses_math(site, page)) {
        assets.extend(
            super::katex_assets::ASSETS
                .iter()
                .map(|(path, _, _)| (*path).into()),
        );
    }
    Ok(FragmentInventory {
        format_version: 1,
        documents,
        assets: assets.into_iter().collect(),
    })
}

pub(super) const CONTENT_CSS: &str = "assets/content.css";
pub(super) const TABSETS: &str = "assets/tabsets.js";
pub(super) const SEARCH_INDEX: &str = "assets/search.json";

pub(super) fn markdown_path(route: &str) -> Result<String, SiteError> {
    Ok(format!(
        "{}.md",
        route.strip_suffix(".html").ok_or(SiteError::Evidence)?
    ))
}

pub(super) fn page_uses_math(site: &Site<'_>, page: &PageModel<'_>) -> bool {
    page.document
        .is_some_and(|document| super::math::contains_math(&document.document.blocks))
        || page.julia_sections.iter().any(|section| {
            section.members.iter().any(|member| {
                site.workspace.packages[&member.package].items[&member.item]
                    .documentation
                    .as_ref()
                    .is_some_and(|document| super::math::contains_math(&document.document.blocks))
            })
        })
        || page.python_class.as_ref().is_some_and(|class| {
            class.members.iter().any(|member| {
                !member.inherited
                    && member.item.documentation.as_ref().is_some_and(|document| {
                        super::math::contains_math(&document.document.blocks)
                    })
            })
        })
        || page.api_page.as_ref().is_some_and(|rust| {
            rust.members().any(|member| {
                member
                    .item
                    .documentation
                    .as_ref()
                    .is_some_and(|document| super::math::contains_math(&document.document.blocks))
            })
        })
}
