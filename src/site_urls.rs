use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{Site, SiteError, relative_url};

/// Public URLs for embedding generated documents and assets in another website.
///
/// Keys retain the original relative artifact paths. Mapping changes links,
/// never the paths written to the fragment bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlMap {
    /// Mapping envelope version. The supported version is `1`.
    pub format_version: u32,
    /// HTML artifact paths mapped to public document URLs.
    pub documents: BTreeMap<String, String>,
    /// Asset artifact paths mapped to public asset URLs.
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
}

impl UrlMap {
    pub(super) fn validate(&self, site: &Site<'_>) -> Result<(), SiteError> {
        if self.format_version != 1 {
            return Err(SiteError::Route("unsupported URL map version".into()));
        }
        let mut destinations = BTreeSet::new();
        for url in self.documents.values().chain(self.assets.values()) {
            if !valid_url(url) || !destinations.insert(url) {
                return Err(SiteError::Route(format!(
                    "invalid or duplicate public URL: {url}"
                )));
            }
        }
        for path in self.documents.keys() {
            if !site.pages.contains_key(path) {
                return Err(SiteError::Route(format!(
                    "unknown document in URL map: {path}"
                )));
            }
        }
        for path in self.assets.keys() {
            if !path.starts_with("assets/") || !valid_artifact(path) {
                return Err(SiteError::Route(format!(
                    "invalid asset in URL map: {path}"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn url(&self, from: &str, to: &str) -> String {
        let (path, fragment) = to.split_once('#').map_or((to, None), |(p, f)| (p, Some(f)));
        let mut result = self
            .documents
            .get(path)
            .or_else(|| self.assets.get(path))
            .cloned()
            .unwrap_or_else(|| relative_url(from, path));
        if let Some(fragment) = fragment {
            result.push('#');
            result.push_str(fragment);
        }
        result
    }

    /// Reject asset mappings that do not name an exported artifact.
    pub(crate) fn validate_assets(
        &self,
        files: &BTreeMap<String, crate::rendering::RenderedFile>,
    ) -> Result<(), SiteError> {
        for path in self.assets.keys() {
            if !files.contains_key(path) {
                return Err(SiteError::Route(format!(
                    "unknown asset in URL map: {path}"
                )));
            }
        }
        Ok(())
    }
}

fn valid_artifact(path: &str) -> bool {
    path.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.contains(['\\', '?', '#'])
}

fn valid_url(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) || value.contains(['\\', '?', '#']) {
        return false;
    }
    let parsed = if value.starts_with('/') && !value.starts_with("//") {
        url::Url::parse(&format!("https://host.invalid{value}"))
    } else {
        url::Url::parse(value)
    };
    parsed.is_ok_and(|url| matches!(url.scheme(), "https" | "http") && url.host_str().is_some())
}
