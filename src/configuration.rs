//! Typed parsing and loading of workspace configuration.
//!
//! These types retain declarations before semantic validation. Parsing checks
//! YAML syntax, field types, required fields, supported enum values, and coherent
//! collection execution settings. It does not resolve filesystem paths or
//! references, validate document execution authority, or discover inputs.
//! Use [`crate::configuration_validation::validate_configuration`] for workspace
//! identity and reference validation.
//! Use [`crate::paths::resolve_workspace_paths`] for explicit filesystem validation.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{Error as _, SeqAccess, Visitor, value::SeqAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticPath, DiagnosticSource, Severity,
};
use crate::documents::AuthoredFormat;
use crate::ir::SourceSpan;

/// The declarations in a root `diplodocus.yaml` file, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "WorkspaceDeclaration")]
pub struct WorkspaceConfiguration {
    /// Project-wide identity.
    pub project: ProjectConfiguration,
    /// Portable defaults used when generating a site from the snapshot.
    #[serde(default, skip_serializing_if = "PresentationDefaults::is_default")]
    pub presentation: PresentationDefaults,
    /// Explicit local source repositories; omission declares none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositoryConfiguration>,
    /// Explicit bundles for browser components in authored pages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<crate::components::ComponentConfiguration>,
    /// Explicit packages and internal components to document; omission declares none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageConfiguration>,
    /// Portable API bundles to import.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package_bundles: Vec<PackageBundleConfiguration>,
    /// Authored content collections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentConfiguration>,
    /// Explicit conceptual API groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concepts: Vec<ConceptConfiguration>,
    /// Implementation and release relationships between packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<RelationshipConfiguration>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct WorkspaceDeclaration {
    #[serde(default, deserialize_with = "deserialize_declared")]
    project: Option<ProjectConfiguration>,
    #[serde(default)]
    presentation: PresentationDefaults,
    #[serde(default)]
    repositories: Vec<RepositoryConfiguration>,
    #[serde(default)]
    components: Vec<crate::components::ComponentConfiguration>,
    #[serde(default, deserialize_with = "deserialize_declared")]
    packages: Option<Vec<PackageConfiguration>>,
    #[serde(default)]
    package_bundles: Vec<PackageBundleConfiguration>,
    #[serde(default, deserialize_with = "deserialize_content")]
    content: Vec<ContentConfiguration>,
    #[serde(default)]
    concepts: Vec<ConceptConfiguration>,
    #[serde(default)]
    relationships: Vec<RelationshipConfiguration>,
}

fn deserialize_declared<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl TryFrom<WorkspaceDeclaration> for WorkspaceConfiguration {
    type Error = String;

    fn try_from(declaration: WorkspaceDeclaration) -> Result<Self, Self::Error> {
        let mut workspace = Self {
            project: declaration.project.ok_or("missing field `project`")?,
            presentation: declaration.presentation,
            repositories: declaration.repositories,
            components: declaration.components,
            packages: declaration.packages.unwrap_or_default(),
            package_bundles: declaration.package_bundles,
            content: declaration.content,
            concepts: declaration.concepts,
            relationships: declaration.relationships,
        };
        for package in &mut workspace.packages {
            for authored in std::mem::take(&mut package.content) {
                workspace
                    .content
                    .push(authored.into_package_content(package)?);
            }
        }
        Ok(workspace)
    }
}

const LOCAL_REPOSITORY: &str = "@local";

pub(crate) fn local_repository_id() -> String {
    LOCAL_REPOSITORY.into()
}

pub(crate) fn is_local_repository(id: &str) -> bool {
    id == LOCAL_REPOSITORY
}

impl WorkspaceConfiguration {
    pub(crate) fn implicit_repository(&self) -> Option<RepositoryConfiguration> {
        let presentation = &self.presentation;
        let used = self.project.repository_url.is_some()
            || self
                .packages
                .iter()
                .any(|p| is_local_repository(&p.repository))
            || self
                .content
                .iter()
                .any(|c| is_local_repository(&c.repository))
            || self
                .components
                .iter()
                .any(|c| is_local_repository(&c.repository))
            || presentation
                .assets
                .iter()
                .any(|a| is_local_repository(&a.repository))
            || presentation
                .asset_directories
                .iter()
                .any(|a| is_local_repository(&a.repository))
            || presentation.logo_images().iter().any(|(_, image)| {
                image.is_some_and(|(repository, _)| is_local_repository(repository))
            })
            || presentation
                .open_graph
                .as_ref()
                .is_some_and(|image| is_local_repository(&image.repository));

        used.then(|| RepositoryConfiguration {
            id: local_repository_id(),
            path: ".".into(),
            url: self.project.repository_url.clone(),
            source_link_template: None,
            revision: None,
        })
    }
}

/// A package bundle source and its expected package identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageBundleConfiguration {
    /// Package ID declared by the producer.
    pub package: String,
    /// Artifact path relative to the configuration directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Public GitHub release asset and its pins.
    #[serde(
        rename = "github-release",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub github_release: Option<GitHubBundleRelease>,
    /// Site-relative package root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
    /// Use the explicitly declared local package.
    #[serde(rename = "local-override", default, skip_serializing_if = "is_false")]
    pub local_override: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// Coordinates and integrity pins for a public GitHub release asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubBundleRelease {
    /// GitHub owner and repository.
    pub repository: String,
    /// Explicit release tag.
    pub tag: String,
    /// Uploaded asset filename.
    pub asset: String,
    /// Full producer source commit.
    pub revision: String,
    /// SHA-256 of the SQLite file.
    pub sha256: String,
}

pub(crate) fn valid_package_mount(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    }) && !matches!(
        path.split('/').next(),
        Some("assets" | "__diplodocus" | "_redirects")
    )
}

/// Project-wide documentation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProjectConfiguration {
    /// Display name of the project.
    pub name: String,
    /// Optional upstream URL for sources beside the configuration file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
}

/// Portable site metadata, independent of extraction and execution policy.
///
/// The built-in theme escapes text and uses validated branding images.
/// No local theme paths, output directories, or execution authority belong here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct PresentationDefaults {
    /// Public files retained even when no authored document references them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<SiteAsset>,
    /// Repository directories published recursively as public files.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asset_directories: Vec<SiteAssetDirectory>,
    /// Icon and web manifest links included in every complete HTML page.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<SiteIcon>,
    /// Public stylesheet paths in cascade order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stylesheets: Vec<DiagnosticPath>,
    /// Public script paths in head order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<DiagnosticPath>,
    /// Home screen title for Apple devices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apple_mobile_web_app_title: Option<String>,
    /// Public R package whose overview serves as the default homepage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_package: Option<String>,
    /// Site title and branding; `None` uses the workspace's project name.
    pub title: Option<String>,
    /// Default HTML and social description, overridden by page metadata.
    pub description: Option<String>,
    /// Metadata overrides keyed by generated site-relative HTML path.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pages: BTreeMap<String, PageMetadata>,
    /// Optional wide, centered homepage, with the same navbar as documentation pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<HomeDefaults>,
    /// Additional project sections shown after Guide in the navbar.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<NavigationSection>,
    /// Repository links shown in the navbar.
    #[serde(skip_serializing_if = "HeaderDefaults::is_default")]
    pub header: HeaderDefaults,
    /// Authored navigation trees; omission keeps automatic directory grouping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<SidebarConfiguration>,
    /// Optional homepage hero image and fallback navbar logo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<LogoDefaults>,
    /// Optional site-wide image and accessible description for link previews.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_graph: Option<OpenGraphDefaults>,
    /// Public site root, including any hosting prefix; `None` omits crawler metadata.
    #[serde(
        deserialize_with = "deserialize_site_url",
        skip_serializing_if = "Option::is_none"
    )]
    pub site_url: Option<String>,
    /// Public URL spelling used by page links, search, and crawler metadata.
    #[serde(skip_serializing_if = "CanonicalUrlStyle::is_default")]
    pub canonical_url_style: CanonicalUrlStyle,
    /// Permanent redirects from old site-root URL paths to generated HTML paths.
    /// Destinations follow `canonical_url_style`; sources omit the hosting prefix.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub redirects: BTreeMap<String, DiagnosticPath>,
    /// Legacy fragment IDs attached to headings or API targets on generated HTML pages.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fragment_aliases: BTreeMap<String, BTreeMap<String, FragmentAliasTarget>>,
    /// Analytics providers for complete HTML pages.
    #[serde(skip_serializing_if = "AnalyticsDefaults::is_default")]
    pub analytics: AnalyticsDefaults,
    /// Optional chat endpoint for the built-in Ask AI widget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_ai: Option<AskAiConfiguration>,
    /// Optional notices and links shown at the bottom of every page.
    #[serde(skip_serializing_if = "FooterDefaults::is_default")]
    pub footer: FooterDefaults,
}

/// Browser endpoint for a site's documentation assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AskAiConfiguration {
    /// Origin-relative path or absolute HTTPS URL serving AI SDK UI message streams.
    #[serde(deserialize_with = "deserialize_ask_ai_endpoint")]
    pub endpoint: String,
}

fn deserialize_ask_ai_endpoint<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    let relative = value.starts_with('/') && !value.starts_with("//");
    let https = value
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
        && validate_external_url(&value).is_ok();
    if !(relative || https)
        || value.contains(['\\', '#'])
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(D::Error::custom(
            "Ask AI endpoint must be an origin-relative path or an absolute HTTPS URL without credentials, fragments, or whitespace",
        ));
    }
    Ok(value)
}

/// Optional analytics providers for the site.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct AnalyticsDefaults {
    /// Umami tracker settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub umami: Option<UmamiConfiguration>,
}

impl AnalyticsDefaults {
    fn is_default(&self) -> bool {
        self.umami.is_none()
    }
}

/// Values copied from a website's Umami tracking code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct UmamiConfiguration {
    /// Absolute URL of the tracker script.
    #[serde(deserialize_with = "deserialize_external_url")]
    pub script_url: String,
    /// Website ID assigned by Umami.
    #[serde(deserialize_with = "deserialize_umami_website_id")]
    pub website_id: String,
}

fn deserialize_umami_website_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(D::Error::custom(
            "Umami website ID must be nonempty and contain no whitespace or control characters",
        ));
    }
    Ok(value)
}

/// Portable header navigation settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeaderDefaults {
    /// Group multiple packages under API by default, or show separate links.
    #[serde(skip_serializing_if = "ApiNavigation::is_default")]
    pub api: ApiNavigation,
    /// Repository IDs in display order; omission includes all public repositories.
    /// An empty list hides repository links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<String>>,
    /// Optional navbar logo that takes precedence over the main logo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<LogoDefaults>,
}

impl HeaderDefaults {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// How the built-in navbar displays package API links.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiNavigation {
    /// Show each visible package as a separate navbar link.
    Links,
    /// Group multiple visible packages in an API dropdown.
    #[default]
    Dropdown,
}

impl ApiNavigation {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A maintained destination for a legacy fragment ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum FragmentAliasTarget {
    /// Exact plain-text heading, with whitespace collapsed.
    Heading(String),
    /// Existing canonical heading or API anchor, without URL encoding.
    Anchor(String),
}

/// Head metadata for a generated page, independent of its visible content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct PageMetadata {
    /// Complete browser and social card title, without an automatic site suffix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Page description; omission inherits the site description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Comma-separated directives for the HTML robots meta tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots: Option<String>,
}

impl PageMetadata {
    pub(crate) fn excludes_from_index(&self) -> bool {
        self.robots.as_deref().is_some_and(|robots| {
            robots.split(',').map(str::trim).any(|directive| {
                directive.eq_ignore_ascii_case("noindex") || directive.eq_ignore_ascii_case("none")
            })
        })
    }
}

/// A public file copied from a declared repository to an explicit site path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SiteAsset {
    /// Repository containing the source file.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Repository-relative source file path.
    pub path: DiagnosticPath,
    /// Site-relative publication path, independent of the hosting prefix.
    pub publish: DiagnosticPath,
}

/// A directory copied from a repository to a site-relative destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteAssetDirectory {
    /// Repository containing the directory.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Repository-relative source directory.
    pub path: DiagnosticPath,
    /// Destination prefix. An empty string publishes at the site root.
    pub publish: String,
}

/// A link to a declared public icon or web manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SiteIcon {
    /// Publication path of a file in `presentation.assets`.
    pub path: DiagnosticPath,
    /// Link relationship; omission selects `icon`.
    #[serde(default)]
    pub rel: SiteIconRel,
    /// Optional MIME type hint, checked against the publication extension.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Optional icon dimensions, such as `any` or `180x180`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<String>,
}

/// Supported public icon and manifest link relationships.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SiteIconRel {
    /// Browser tab or shortcut icon.
    #[default]
    Icon,
    /// Home screen icon on Apple devices.
    AppleTouchIcon,
    /// Web application manifest.
    Manifest,
}

impl SiteIconRel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Icon => "icon",
            Self::AppleTouchIcon => "apple-touch-icon",
            Self::Manifest => "manifest",
        }
    }
}

/// An explicit sidebar for every page, or sidebars selected by URL directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SidebarConfiguration {
    /// The same authored navigation on every page.
    Single(Vec<SidebarItem>),
    /// The longest matching site-root directory prefix selects the sidebar.
    Multiple(BTreeMap<String, Vec<SidebarItem>>),
}

impl SidebarConfiguration {
    pub(crate) fn for_route(&self, route: &str) -> Option<(&str, &[SidebarItem])> {
        match self {
            Self::Single(items) => Some(("/", items)),
            Self::Multiple(sidebars) => sidebars
                .iter()
                .filter(|(prefix, _)| route.starts_with(prefix.trim_start_matches('/')))
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(prefix, items)| (prefix.as_str(), items.as_slice())),
        }
    }
}

/// A sidebar link or a group containing further links and groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SidebarItem {
    /// Visible label as plain text. Defaults to the linked page title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Repository-relative Markdown source path or generated site-relative HTML route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<DiagnosticPath>,
    /// Children in display order; groups may also link to a landing page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SidebarItem>,
    /// Enable collapse controls, initially closed when true and open when false.
    /// Omission renders a permanently expanded group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
}

/// A project navigation section containing pages beneath its landing page's directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct NavigationSection {
    /// Visible navbar and sidebar heading, without Markdown or HTML.
    pub label: String,
    /// Generated route of a visible project page, such as `reference/index.html`.
    pub path: DiagnosticPath,
}

impl NavigationSection {
    pub(crate) fn prefix(&self) -> &str {
        self.path.as_str().strip_suffix("index.html").unwrap_or("")
    }
}

/// Content for the built-in landing page at the site root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HomeDefaults {
    /// Main heading, rendered as plain text; newlines separate lines.
    pub tagline: String,
    /// Introduction below the main heading, rendered as plain text.
    pub description: String,
    /// Generated HTML route opened by the navbar's Guide link.
    pub guide: DiagnosticPath,
    /// Call-to-action links in display order; the first receives emphasis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<HomeAction>,
    /// Linked feature descriptions in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<HomeFeature>,
}

/// A homepage action pointing to a generated page or an external website.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct HomeAction {
    /// Visible link text, without HTML or Markdown.
    pub label: String,
    /// Exactly one generated page path or absolute HTTP or HTTPS URL.
    #[serde(flatten)]
    pub target: HomeActionTarget,
    /// Optional decorative icon shown before the link text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<HomeActionIcon>,
}

/// The destination of a homepage action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HomeActionTarget {
    /// Normalized site-relative HTML artifact path, including its extension.
    Path(DiagnosticPath),
    /// Absolute HTTP or HTTPS destination without credentials or whitespace.
    Url(#[serde(deserialize_with = "deserialize_external_url")] String),
}

impl<'de> Deserialize<'de> for HomeAction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case", deny_unknown_fields)]
        struct Declaration {
            label: String,
            path: Option<DiagnosticPath>,
            url: Option<String>,
            icon: Option<HomeActionIcon>,
        }

        let declaration = Declaration::deserialize(deserializer)?;
        let target = match (declaration.path, declaration.url) {
            (Some(path), None) => HomeActionTarget::Path(path),
            (None, Some(url)) => {
                validate_external_url(&url).map_err(D::Error::custom)?;
                HomeActionTarget::Url(url)
            }
            _ => {
                return Err(D::Error::custom(
                    "homepage actions require exactly one of path or url",
                ));
            }
        };
        Ok(Self {
            label: declaration.label,
            target,
            icon: declaration.icon,
        })
    }
}

/// A built-in SVG icon for a homepage action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HomeActionIcon {
    /// An arrow pointing right.
    ArrowRight,
    /// An open book.
    BookOpen,
    /// A pair of angle brackets.
    Code,
    /// A downward arrow above a tray.
    Download,
    /// A command prompt in a terminal window.
    Terminal,
}

/// A feature card with an optional documentation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HomeFeature {
    /// Short feature heading, without HTML or Markdown.
    pub title: String,
    /// Explanation, without HTML or Markdown.
    pub description: String,
    /// Optional site-relative HTML artifact path, including its extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<DiagnosticPath>,
}

/// A checked-in branding image within an explicitly declared repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct LogoDefaults {
    /// ID of the repository containing the image.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Normalized repository-relative path to an SVG, PNG, or JPEG file.
    pub path: DiagnosticPath,
    /// Alternative artwork used in dark mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<DarkLogoDefaults>,
}

/// Dark-mode artwork for a branding image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DarkLogoDefaults {
    /// ID of the repository containing the image.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Normalized repository-relative path to an SVG, PNG, or JPEG file.
    pub path: DiagnosticPath,
}

/// A checked-in social preview image within an explicitly declared repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OpenGraphDefaults {
    /// ID of the repository containing the image.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Normalized repository-relative path to a PNG or JPEG file.
    pub path: DiagnosticPath,
    /// Accessible description of the image, without HTML or Markdown.
    pub alt: String,
}

type LogoImage<'a> = (&'static str, Option<(&'a String, &'a DiagnosticPath)>);

impl PresentationDefaults {
    pub(crate) fn logo_images(&self) -> [LogoImage<'_>; 4] {
        [
            (
                "presentation.logo",
                self.logo
                    .as_ref()
                    .map(|logo| (&logo.repository, &logo.path)),
            ),
            (
                "presentation.logo.dark",
                self.logo
                    .as_ref()
                    .and_then(|logo| logo.dark.as_ref())
                    .map(|image| (&image.repository, &image.path)),
            ),
            (
                "presentation.header.logo",
                self.header
                    .logo
                    .as_ref()
                    .map(|logo| (&logo.repository, &logo.path)),
            ),
            (
                "presentation.header.logo.dark",
                self.header
                    .logo
                    .as_ref()
                    .and_then(|logo| logo.dark.as_ref())
                    .map(|image| (&image.repository, &image.path)),
            ),
        ]
    }

    fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn site_base_url(&self) -> Result<Option<url::Url>, &'static str> {
        self.site_url.as_deref().map(parse_site_url).transpose()
    }
}

/// Portable footer content; omitted fields do not appear in the generated site.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct FooterDefaults {
    /// Free-form documentation release or branch label, independent of package versions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_version: Option<String>,
    /// Complete copyright notice, including the symbol, years, and holder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    /// Alternative licenses for the site's content, displayed with "or" between them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub licenses: Vec<FooterLink>,
    /// Project links, in display order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<FooterLink>,
    /// Whether to show a linked "Built with Diplodocus" credit.
    #[serde(deserialize_with = "deserialize_boolean")]
    pub built_with: bool,
}

impl FooterDefaults {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn deserialize_boolean<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    struct BooleanVisitor;

    impl<'de> serde::de::Visitor<'de> for BooleanVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a boolean")
        }

        fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<bool, E> {
            Ok(value)
        }
    }

    // Preserve YAML scalar types instead of coercing quoted boolean text.
    deserializer.deserialize_any(BooleanVisitor)
}

/// A labeled external footer link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FooterLink {
    /// Link text, rendered without Markdown or HTML parsing.
    pub label: String,
    /// Absolute HTTP or HTTPS destination.
    #[serde(deserialize_with = "deserialize_external_url")]
    pub url: String,
}

fn deserialize_external_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    validate_external_url(&value).map_err(D::Error::custom)?;
    Ok(value)
}

fn validate_external_url(value: &str) -> Result<(), &'static str> {
    let invalid =
        "external links must use absolute HTTP or HTTPS URLs without credentials or whitespace";
    let url = url::Url::parse(value).map_err(|_| invalid)?;
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .ok_or(invalid)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || authority.is_empty()
        || authority.contains('@')
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(invalid);
    }
    Ok(())
}

/// How a host exposes the generated HTML files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CanonicalUrlStyle {
    /// Preserve generated filenames, including `index.html`.
    #[default]
    File,
    /// Use directory URLs for index pages and omit other `.html` suffixes.
    Clean,
}

impl CanonicalUrlStyle {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn deserialize_site_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(value) = &value {
        parse_site_url(value).map_err(D::Error::custom)?;
    }
    Ok(value)
}

fn parse_site_url(value: &str) -> Result<url::Url, &'static str> {
    let invalid = "presentation.site-url must be an absolute HTTP or HTTPS URL without credentials, query parameters, fragments, or whitespace";
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(invalid);
    }
    let mut url = url::Url::parse(value).map_err(|_| invalid)?;
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .ok_or(invalid)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || authority.is_empty()
        || authority.contains('@')
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid);
    }
    // A hosting prefix denotes a directory even when its declaration omits the slash.
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

/// An explicitly supplied local source repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RepositoryConfiguration {
    /// Stable repository identifier.
    pub id: String,
    /// Source root, relative to the configuration directory.
    pub path: PathBuf,
    /// Canonical source origin, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional forge-specific template for revision, path, and line links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_link_template: Option<String>,
    /// Explicit revision, including for sources without version-control metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// A package or internal component with explicit extraction targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PackageConfiguration {
    /// Stable package identity used by references and relationships.
    pub id: String,
    /// Display name of the package.
    pub name: String,
    /// Site-wide URL slug, independent of the ecosystem and published name.
    pub slug: String,
    /// Package ecosystem identifier.
    pub ecosystem: String,
    /// Identifier of the repository containing the package.
    #[serde(
        default = "local_repository_id",
        skip_serializing_if = "is_local_repository"
    )]
    pub repository: String,
    /// Package root, relative to its repository.
    pub path: PathBuf,
    /// Metadata file, relative to the package root.
    pub metadata_path: PathBuf,
    /// GFM overview file, relative to the package root; omission discovers README.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<PathBuf>,
    /// Tracked dependencies and reuse policy for incremental extraction.
    #[serde(default, skip_serializing_if = "IncrementalConfiguration::is_default")]
    pub incremental: IncrementalConfiguration,
    /// Native R documentation generation and supplemental watched inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r: Option<RPackageConfiguration>,
    /// Distribution or component classification; defaults to `package`.
    #[serde(default)]
    pub kind: PackageKind,
    /// Navigation and search visibility; defaults to `public`.
    #[serde(default)]
    pub visibility: PackageVisibility,
    /// Explicit extraction targets; this field is required, but may be empty.
    /// An empty list disables API extraction for this package. Targets are never
    /// inferred from the ecosystem or files.
    pub targets: Vec<ExtractionTargetConfiguration>,
    // The workspace resolves these relative to the package into its collection list.
    #[serde(default, skip_serializing)]
    content: Vec<ContentDeclaration>,
}

/// Documentation preparation for an R package, shared by its targets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct RPackageConfiguration {
    /// Select roxygen2 automatically from package metadata unless overridden.
    pub documentation: RDocumentationMode,
    /// Additional maintained inputs, relative to the package root.
    pub extra_inputs: Vec<PathBuf>,
    /// Exclusively generated custom outputs, relative to the package root.
    pub generated_outputs: Vec<PathBuf>,
}

/// Authority for a package's Rd and namespace documentation inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RDocumentationMode {
    /// Use roxygen2 when the package declares roxygen configuration.
    #[default]
    Auto,
    /// Generate documentation with the prepared roxygen2 installation.
    Roxygen2,
    /// Consume maintained Rd and namespace files without generation.
    Rd,
}

/// Declared inputs and dependencies for package or content preparation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct IncrementalConfiguration {
    /// Reuse completed results when tracked inputs still match.
    pub cache: IncrementalCache,
    /// Workspace package IDs whose inputs affect this unit.
    pub depends_on: Vec<String>,
    /// Additional files or directories, relative to the unit's repository.
    pub extra_inputs: Vec<PathBuf>,
    /// Environment variable names whose values affect this unit.
    pub environment_variables: Vec<String>,
}

impl IncrementalConfiguration {
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// Whether a unit permits reuse of completed preparation results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncrementalCache {
    /// Reuse results after checking their recorded inputs.
    #[default]
    Auto,
    /// Prepare the unit afresh on each command attempt.
    Off,
}

/// Whether a documentation unit represents a package or an internal component.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    /// A distributable package.
    #[default]
    Package,
    /// An explicitly documented component.
    Component,
}

/// Visibility of a package in generated navigation and search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageVisibility {
    /// Included in project navigation and search.
    #[default]
    Public,
    /// Addressable and searchable, but omitted from project navigation by default.
    Internal,
    /// Available as a semantic target without navigation or search entries.
    Hidden,
}

/// An explicit source target consumed by one API extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ExtractionTargetConfiguration {
    /// Target identifier within its package.
    pub id: String,
    /// Selected extractor identifier, such as `python` or `r`.
    pub extractor: String,
    /// Source path, relative to the package root.
    pub path: PathBuf,
    /// Target role, such as `public-api` or `internal-api`.
    pub role: String,
    /// Cargo feature selection for Rust targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustTargetConfiguration>,
    /// Explicit supplemental Julia extension sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub julia: Option<JuliaTargetConfiguration>,
}

/// Static source selection for one explicit Julia target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct JuliaTargetConfiguration {
    /// Extension entry files relative to the package root, each with its own module.
    pub extensions: Vec<PathBuf>,
}

/// Compiler feature selection for one explicit Rust library target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct RustTargetConfiguration {
    /// Additional Cargo features to enable.
    pub features: Vec<String>,
    /// Whether Cargo enables the crate's default features.
    #[serde(deserialize_with = "deserialize_boolean")]
    pub default_features: bool,
}

impl Default for RustTargetConfiguration {
    fn default() -> Self {
        Self {
            features: Vec::new(),
            default_features: true,
        }
    }
}

/// Which authored collection pages appear in sidebar navigation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentNavigation {
    /// Include every page in the collection.
    #[default]
    All,
    /// Include only the collection's root index page.
    Index,
    /// Omit the collection from sidebar navigation.
    None,
}

impl ContentNavigation {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::All
    }
}

/// An authored collection with independent ownership and source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ContentConfiguration {
    /// Stable collection identifier.
    pub id: String,
    /// Reserved owner `project`, or a package identifier.
    pub owner: String,
    /// Repository containing the authored sources.
    pub repository: String,
    /// Source directory or single authored file, relative to its repository.
    pub path: PathBuf,
    /// URL mount beneath the owner's documentation root.
    pub mount: String,
    /// Explicit authored input profile.
    pub format: AuthoredFormat,
    /// Sidebar inclusion; does not affect rendering, search, or sitemaps.
    #[serde(default, skip_serializing_if = "ContentNavigation::is_default")]
    pub navigation: ContentNavigation,
    /// Tracked dependencies and reuse policy for authored preparation.
    #[serde(default, skip_serializing_if = "IncrementalConfiguration::is_default")]
    pub incremental: IncrementalConfiguration,
    /// Execution defaults to native knitr for Rmd and `never` for other formats.
    #[serde(default)]
    pub execution: ExecutionConfiguration,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ExecutionDeclaration {
    mode: Option<ExecutionMode>,
    engine: Option<ExecutionEngine>,
    kernel: Option<String>,
    julia_project: Option<PathBuf>,
    #[serde(default)]
    declared_environment_inputs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ContentDeclaration {
    id: Option<String>,
    owner: Option<String>,
    repository: Option<String>,
    path: PathBuf,
    mount: String,
    #[serde(default)]
    format: AuthoredFormat,
    #[serde(default)]
    navigation: ContentNavigation,
    #[serde(default)]
    incremental: IncrementalConfiguration,
    #[serde(default)]
    execution: Option<ExecutionDeclaration>,
}

impl ContentDeclaration {
    fn into_package_content(
        mut self,
        package: &PackageConfiguration,
    ) -> Result<ContentConfiguration, String> {
        if self.owner.is_some() || self.repository.is_some() {
            return Err(format!(
                "packages.{}.content inherits its owner and repository from the package",
                package.id
            ));
        }
        self.owner = Some(package.id.clone());
        self.repository = Some(package.repository.clone());
        self.resolve(Some(&package.path))
    }

    fn resolve(mut self, package_path: Option<&Path>) -> Result<ContentConfiguration, String> {
        let owner = self.owner.unwrap_or_else(|| "project".into());
        let id = self.id.unwrap_or_else(|| {
            let path = self
                .path
                .components()
                .filter(|component| *component != std::path::Component::CurDir)
                .collect::<PathBuf>();
            let path = path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            format!(
                "content:{}:{owner}:{}:{path}:{}",
                owner.len(),
                path.len(),
                self.mount
            )
        });
        if let Some(package_path) = package_path {
            let join = |path: &Path| {
                if path.as_os_str().is_empty()
                    || path.components().any(|component| {
                        !matches!(
                            component,
                            std::path::Component::Normal(_) | std::path::Component::CurDir
                        )
                    })
                {
                    return Err(format!(
                        "content `{id}`: expected a package-relative path within the package, got `{}`",
                        path.display()
                    ));
                }
                Ok(package_path.join(path))
            };
            self.path = join(&self.path)?;
            if let Some(execution) = &mut self.execution {
                execution.julia_project =
                    execution.julia_project.as_deref().map(join).transpose()?;
                execution.declared_environment_inputs = execution
                    .declared_environment_inputs
                    .iter()
                    .map(|path| join(path))
                    .collect::<Result<_, _>>()?;
            }
        }
        let execution = self.execution.unwrap_or_default();
        let rmd = self.format == AuthoredFormat::Rmd;
        let collection = ContentConfiguration {
            id,
            owner,
            repository: self.repository.unwrap_or_else(local_repository_id),
            path: self.path,
            mount: self.mount,
            format: self.format,
            navigation: self.navigation,
            incremental: self.incremental,
            execution: ExecutionConfiguration {
                mode: execution.mode.unwrap_or(if rmd {
                    ExecutionMode::Execute
                } else {
                    ExecutionMode::Never
                }),
                engine: execution.engine.or(if rmd {
                    Some(ExecutionEngine::Knitr)
                } else {
                    None
                }),
                kernel: execution.kernel,
                julia_project: execution.julia_project,
                declared_environment_inputs: execution.declared_environment_inputs,
            },
        };
        collection
            .validate_execution()
            .map_err(|error| format!("content `{}`: {error}", collection.id))?;
        Ok(collection)
    }
}

impl<'de> Deserialize<'de> for ContentConfiguration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ContentDeclaration::deserialize(deserializer)?
            .resolve(None)
            .map_err(D::Error::custom)
    }
}

fn deserialize_content<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ContentConfiguration>, D::Error> {
    struct ContentVisitor;

    impl<'de> Visitor<'de> for ContentVisitor {
        type Value = Vec<ContentConfiguration>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a documentation path or a sequence of content collections")
        }

        fn visit_str<E: serde::de::Error>(self, path: &str) -> Result<Self::Value, E> {
            let collection = ContentDeclaration {
                path: path.into(),
                ..ContentDeclaration::default()
            }
            .resolve(None)
            .map_err(E::custom)?;
            Ok(vec![collection])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, sequence: A) -> Result<Self::Value, A::Error> {
            Vec::deserialize(SeqAccessDeserializer::new(sequence))
        }
    }

    deserializer.deserialize_any(ContentVisitor)
}

impl ContentConfiguration {
    /// Validate the profile, mode, engine, kernel, and environment declarations together.
    ///
    /// Deserialization calls this automatically. Call it again after modifying
    /// a collection programmatically. This performs no filesystem reads, kernel
    /// discovery, or execution. Paths and kernel spelling remain as declared.
    ///
    /// # Errors
    ///
    /// Returns the first contradictory or malformed setting. Environment paths
    /// must name explicit repository-relative files and be lexically distinct.
    /// [`crate::paths::resolve_workspace_paths`] checks file existence, type, and
    /// symlink containment separately. Reading file contents remains a later step.
    pub fn validate_execution(&self) -> Result<(), ExecutionConfigurationError> {
        let execution = &self.execution;
        if execution.julia_project.is_some()
            && !(self.format == AuthoredFormat::Documenter
                && execution.mode == ExecutionMode::Execute
                && execution.engine == Some(ExecutionEngine::Julia))
        {
            return Err(ExecutionConfigurationError::JuliaProject);
        }
        if self.format == AuthoredFormat::Rmd
            && (execution.mode != ExecutionMode::Execute
                || execution.engine != Some(ExecutionEngine::Knitr)
                || execution.kernel.is_some())
        {
            return Err(ExecutionConfigurationError::RmdExecution);
        }
        if execution.mode == ExecutionMode::Never {
            for (field, declared) in [
                ("engine", execution.engine.is_some()),
                ("kernel", execution.kernel.is_some()),
                (
                    "declared-environment-inputs",
                    !execution.declared_environment_inputs.is_empty(),
                ),
            ] {
                if declared {
                    return Err(ExecutionConfigurationError::InactiveSetting { field });
                }
            }
            return Ok(());
        }

        if self.format == AuthoredFormat::Gfm
            && (execution.engine != Some(ExecutionEngine::Native) || execution.kernel.is_some())
        {
            return Err(ExecutionConfigurationError::GfmExecution);
        }
        if self.format == AuthoredFormat::Documenter
            && (execution.engine != Some(ExecutionEngine::Julia)
                || execution.kernel.is_some()
                || execution.julia_project.is_none())
        {
            return Err(ExecutionConfigurationError::DocumenterExecution);
        }
        if let Some(project) = &execution.julia_project {
            environment_input_components(project)
                .map_err(|_| ExecutionConfigurationError::JuliaProject)?;
            if !matches!(
                project.file_name().and_then(|name| name.to_str()),
                Some("Project.toml" | "JuliaProject.toml")
            ) {
                return Err(ExecutionConfigurationError::JuliaProject);
            }
        }
        if self.format == AuthoredFormat::Qmd {
            if execution.engine != Some(ExecutionEngine::Jupyter) {
                return Err(ExecutionConfigurationError::MissingEngine);
            }
            let kernel = execution
                .kernel
                .as_deref()
                .ok_or(ExecutionConfigurationError::MissingKernel)?;
            if !valid_kernel_selector(kernel) {
                return Err(ExecutionConfigurationError::InvalidKernel);
            }
        }

        let mut inputs = HashMap::new();
        for (index, path) in execution.declared_environment_inputs.iter().enumerate() {
            let components = environment_input_components(path).map_err(|reason| {
                ExecutionConfigurationError::InvalidEnvironmentInput { index, reason }
            })?;
            if let Some(first_index) = inputs.insert(components, index) {
                return Err(ExecutionConfigurationError::DuplicateEnvironmentInput {
                    index,
                    first_index,
                });
            }
        }
        Ok(())
    }
}

pub(crate) fn valid_kernel_selector(kernel: &str) -> bool {
    !kernel.is_empty()
        && !matches!(kernel, "." | "..")
        && kernel
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

fn environment_input_components(path: &Path) -> Result<Vec<&str>, &'static str> {
    let path = path.to_str().ok_or("paths must be valid UTF-8")?;
    if path.is_empty() {
        return Err("paths must not be empty");
    }
    if path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err("paths must be relative to the collection's repository");
    }
    if path.contains('\\') {
        return Err("paths must use forward-slash separators");
    }
    if path.contains('\0') {
        return Err("paths must not contain NUL bytes");
    }
    if path.contains(['*', '?', '[', ']', '{', '}']) {
        return Err("declare individual files, not glob patterns");
    }
    if matches!(path.rsplit('/').next(), Some("" | "." | "..")) {
        return Err("declare a file, not a directory reference");
    }

    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err("paths must not escape the collection's repository");
                }
            }
            component => components.push(component),
        }
    }
    Ok(components)
}

/// A contradictory or malformed collection execution declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionConfigurationError {
    /// R Markdown always uses native knitr execution.
    #[error("R Markdown requires `mode: execute`, `engine: knitr`, and no kernel")]
    RmdExecution,
    /// A display-only collection has execution-specific settings.
    #[error("`{field}` requires `mode: execute`")]
    InactiveSetting {
        /// The contradictory execution field.
        field: &'static str,
    },
    /// A GFM collection requested an incompatible execution engine.
    #[error("GFM execution requires `engine: native` and no kernel")]
    GfmExecution,
    /// Documenter execution requires a prepared native Julia environment.
    #[error(
        "Documenter execution requires `engine: julia`, `julia-project: path/to/Project.toml`, and no kernel"
    )]
    DocumenterExecution,
    /// A Julia environment selector is absent, misplaced, or malformed.
    #[error(
        "`julia-project` must name a repository-relative Project.toml or JuliaProject.toml for Documenter execution with `engine: julia`"
    )]
    JuliaProject,
    /// Execution has no explicitly selected Jupyter engine.
    #[error("`mode: execute` requires `engine: jupyter`")]
    MissingEngine,
    /// Execution has no explicitly selected kernel.
    #[error("`mode: execute` requires an explicit kernel")]
    MissingKernel,
    /// The kernel selector is empty, a path, or outside the supported ASCII syntax.
    #[error(
        "invalid kernel selector: use ASCII letters, digits, `-`, `.`, or `_`, excluding empty names, `.` and `..`"
    )]
    InvalidKernel,
    /// An environment input does not declare an explicit repository-relative file.
    #[error("invalid `declared-environment-inputs[{index}]`: {reason}")]
    InvalidEnvironmentInput {
        /// Zero-based index of the invalid declaration.
        index: usize,
        /// Explanation of the violated path rule.
        reason: &'static str,
    },
    /// Two environment declarations normalize to the same relative path.
    #[error("`declared-environment-inputs[{index}]` duplicates input {first_index}")]
    DuplicateEnvironmentInput {
        /// Zero-based index of the duplicate declaration.
        index: usize,
        /// Zero-based index of the first declaration of that path.
        first_index: usize,
    },
}

impl ExecutionConfigurationError {
    /// Identify the collection whose programmatically constructed execution
    /// settings failed validation. No source path or span is invented.
    /// Callers with source context can attach it to the returned diagnostic.
    pub fn to_diagnostic(&self, collection: impl Into<String>) -> Diagnostic {
        Diagnostic::new(
            DiagnosticCode::InvalidExecutionConfiguration,
            Severity::Error,
            self.to_string(),
        )
        .with_entity(DiagnosticEntity::Content {
            id: collection.into(),
        })
    }
}

/// Declared execution settings, validated in their owning collection's context.
///
/// Standalone deserialization checks field types. Collection deserialization also
/// checks their consistency through [`ContentConfiguration::validate_execution`].
/// Document authority and filesystem validation remain necessary before execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ExecutionConfiguration {
    /// Requested mode; defaults to `never`.
    #[serde(default)]
    pub mode: ExecutionMode,
    /// Selected execution engine, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<ExecutionEngine>,
    /// Explicit kernel selector, such as `python3` or `ir`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    /// Prepared native Julia environment's repository-relative project file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub julia_project: Option<PathBuf>,
    /// Environment manifests or lockfiles, relative to the collection's repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_environment_inputs: Vec<PathBuf>,
}

/// Workspace-level authored execution mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    /// Keep cells display-only.
    #[default]
    Never,
    /// Request execution subject to the collection and document policies.
    Execute,
}

/// Supported authored execution engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionEngine {
    /// Native package environments for runnable Markdown fences.
    Native,
    /// A page-scoped Jupyter kernel session.
    Jupyter,
    /// A native R session using knitr.
    Knitr,
    /// A native Julia process executing Documenter examples.
    Julia,
}

/// An explicitly declared group of corresponding APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConceptConfiguration {
    /// Stable concept identifier.
    pub id: String,
    /// How the member APIs correspond.
    pub kind: ConceptKind,
    /// Member declarations in source order, resolved during semantic validation.
    pub members: Vec<ConceptMember>,
}

/// How closely the APIs in a concept correspond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConceptKind {
    /// Members represent the same API.
    Equivalent,
    /// Members provide comparable capabilities.
    Analogous,
    /// Members have an explicitly declared association.
    Related,
}

/// An unresolved item reference in a conceptual API group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConceptMember {
    /// Package containing the member.
    pub package: String,
    /// Semantic item ID or a qualified name that must resolve unambiguously.
    pub item: String,
}

/// An implementation or release relationship, distinct from an API concept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RelationshipConfiguration {
    /// Source workspace package ID or external ecosystem-qualified coordinate.
    pub from: String,
    /// Destination workspace package ID or external ecosystem-qualified coordinate.
    pub to: String,
    /// Typed implementation or release relationship.
    pub kind: RelationshipKind,
    /// Declared version constraint, retained in its ecosystem's syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_constraint: Option<String>,
    /// Origin of the declaration, such as `explicit` or package metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

/// Supported implementation and release relationships between packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationshipKind {
    /// The source depends on the destination package.
    DependsOn,
    /// The source provides bindings to the destination package.
    Binds,
    /// The source wraps the destination package.
    Wraps,
    /// The source is generated from the destination package.
    GeneratedFrom,
}

/// A configuration load failure with its source path and underlying cause.
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// The configuration file could not be read as UTF-8 text.
    #[error("could not read configuration `{}`: {source}", path.display())]
    Read {
        /// Configuration path supplied by the caller.
        path: PathBuf,
        /// Underlying filesystem or UTF-8 error.
        source: std::io::Error,
    },
    /// The configuration is malformed or does not match the input schema.
    #[error("could not parse configuration `{}`: {source}", path.display())]
    Parse {
        /// Configuration path supplied by the caller.
        path: PathBuf,
        /// Underlying YAML error, including its source range when available.
        source: Box<serde_saphyr::Error>,
    },
}

impl ConfigurationError {
    /// Convert a load failure to a portable diagnostic using a caller-supplied
    /// configuration-relative path, retaining the YAML byte range when available.
    ///
    /// Runtime paths remain in this error for local troubleshooting. Filesystem
    /// causes contribute only their error kind because custom I/O messages can
    /// embed machine paths. Parser messages describe the input declarations.
    pub fn to_diagnostic(&self, path: DiagnosticPath) -> Diagnostic {
        let mut diagnostic = match self {
            Self::Read { source, .. } => Diagnostic::new(
                DiagnosticCode::ConfigurationReadFailed,
                Severity::Error,
                format!("could not read configuration: {}", source.kind()),
            ),
            Self::Parse { source, .. } => {
                let mut diagnostic = Diagnostic::new(
                    DiagnosticCode::InvalidConfiguration,
                    Severity::Error,
                    source.without_snippet().to_string(),
                );
                diagnostic.span = source.location().and_then(|location| {
                    let span = location.span();
                    let start = usize::try_from(span.byte_offset()?).ok()?;
                    let length = usize::try_from(span.byte_len()?).ok()?;
                    Some(SourceSpan {
                        start,
                        end: start.checked_add(length)?,
                    })
                });
                diagnostic
            }
        };
        diagnostic.source = Some(DiagnosticSource::Configuration { path });
        diagnostic.related_entity = Some(DiagnosticEntity::Configuration);
        diagnostic
    }
}

/// Parse declarations from a single YAML document without reading paths or executing code.
///
/// Omitted repository, package, and content collections are empty. Each declared
/// package requires a `targets` list; `targets: []` declares no API extraction.
/// Package kind, package visibility, and execution mode use their documented
/// defaults. Field names use kebab-case. Unknown fields are rejected in every
/// mapping. Paths and unresolved references retain their declared spelling.
///
/// ```
/// use diplodocus::configuration::parse_configuration;
///
/// let config = parse_configuration("project:\n  name: \"Foo\"\n")?;
/// assert_eq!(config.project.name, "Foo");
/// assert!(config.packages.is_empty());
/// # Ok::<(), Box<serde_saphyr::Error>>(())
/// ```
///
/// # Errors
///
/// Returns a YAML error for malformed syntax, duplicate keys, multiple documents,
/// missing required fields, unknown fields, incorrect field types, unsupported enum
/// values, or incoherent collection execution settings. Filesystem, identity,
/// relationship, and document-authority validation remain separate steps.
pub fn parse_configuration(
    source: &str,
) -> Result<WorkspaceConfiguration, Box<serde_saphyr::Error>> {
    // Keep field types explicit rather than coercing numbers and booleans to strings.
    serde_saphyr::from_str_with_options(
        source,
        serde_saphyr::options! {
            no_schema: true,
            strict_booleans: true,
        },
    )
    .map_err(Box::new)
}

/// Read and parse one explicitly supplied workspace configuration file.
///
/// Only the configuration file is read. Declared paths remain unresolved, and
/// parsing does not discover repositories, packages, targets, or kernels.
/// Use [`crate::paths::resolve_workspace_paths`] to resolve and validate declared
/// filesystem inputs after loading.
///
/// # Errors
///
/// Returns the source path and underlying error if reading or parsing fails.
pub fn load_configuration(
    path: impl AsRef<Path>,
) -> Result<WorkspaceConfiguration, ConfigurationError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| ConfigurationError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse_configuration(&source).map_err(|source| ConfigurationError::Parse {
        path: path.to_owned(),
        source,
    })
}

pub(crate) const DEFAULT_WORKSPACE: &str = "./diplodocus.yaml";

pub(crate) fn resolve_configuration(
    path: &Path,
    allow_missing: bool,
) -> Result<(Option<String>, WorkspaceConfiguration), ConfigurationError> {
    let read_error = |source| ConfigurationError::Read {
        path: path.into(),
        source,
    };
    let invalid = |message: &str| {
        read_error(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        ))
    };
    let source = match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(read_error(error)),
    };
    let mut declarations: WorkspaceDeclaration = serde_saphyr::from_str_with_options(
        source.as_deref().unwrap_or("{}"),
        serde_saphyr::options! { no_schema: true, strict_booleans: true },
    )
    .map_err(|source| ConfigurationError::Parse {
        path: path.into(),
        source: Box::new(source),
    })?;
    if declarations.packages.is_none() {
        let root = path.parent().unwrap_or(Path::new("."));
        let descriptor = root.join("DESCRIPTION");
        match fs::read_to_string(&descriptor) {
            Ok(text) => {
                let location = crate::ir::SourceLocation {
                    repository: local_repository_id(),
                    path: "DESCRIPTION".try_into().expect("portable path"),
                    span: None,
                };
                let mut diagnostics = Vec::new();
                let metadata =
                    crate::extractors::r::validate_metadata(&text, &location, &mut diagnostics)
                        .filter(|metadata| {
                            metadata
                                .fields
                                .get("Type")
                                .is_none_or(|field| field.value == "Package")
                        })
                        .ok_or_else(|| {
                            invalid("root DESCRIPTION must describe a valid R package")
                        })?;
                let id = metadata.name;
                declarations
                    .project
                    .get_or_insert_with(|| ProjectConfiguration {
                        name: id.clone(),
                        repository_url: None,
                    });
                declarations
                    .presentation
                    .primary_package
                    .get_or_insert_with(|| id.clone());
                let overview = root
                    .join("docs/index.md")
                    .is_file()
                    .then(|| PathBuf::from("docs/index.md"));
                declarations.packages = Some(vec![PackageConfiguration {
                    id: id.clone(),
                    name: id.clone(),
                    slug: id,
                    ecosystem: "r".into(),
                    repository: local_repository_id(),
                    path: ".".into(),
                    metadata_path: "DESCRIPTION".into(),
                    overview,
                    incremental: Default::default(),
                    r: None,
                    kind: Default::default(),
                    visibility: Default::default(),
                    content: Vec::new(),
                    targets: vec![ExtractionTargetConfiguration {
                        id: "api".into(),
                        extractor: "r".into(),
                        path: ".".into(),
                        role: "public-api".into(),
                        rust: None,
                        julia: None,
                    }],
                }]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(read_error(error)),
        }
    }
    let config = WorkspaceConfiguration::try_from(declarations).map_err(|message| {
        invalid(if source.is_none() {
            "expected diplodocus.yaml or a root R package DESCRIPTION"
        } else {
            &message
        })
    })?;
    Ok((source, config))
}
