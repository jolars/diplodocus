//! Public file declarations shared by extraction and detached generation.

use std::collections::BTreeSet;
use std::path::Path;

use crate::configuration::{PresentationDefaults, SiteIconRel};
use crate::site::{Site, SiteError};

pub(crate) fn media_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|value| value.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        Some("webmanifest") => "application/manifest+json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("pdf") => "application/pdf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => crate::components::media_type(path),
    }
}

pub(crate) fn validate_declarations(presentation: &PresentationDefaults) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for asset in &presentation.assets {
        let path = asset.publish.as_str();
        if !crate::components::safe_path(asset.path.as_str()) || !crate::components::safe_path(path)
        {
            return Err(format!(
                "public asset paths must be normalized relative file paths: {path:?}"
            ));
        }
        if !paths.insert(path) {
            return Err(format!("duplicate public asset publication path: {path:?}"));
        }
    }
    for path in &paths {
        if paths
            .iter()
            .any(|other| path != other && overlaps(path, other))
        {
            return Err(format!(
                "public asset file and directory paths conflict: {path:?}"
            ));
        }
    }
    let mut links = BTreeSet::new();
    for icon in &presentation.icons {
        let path = icon.path.as_str();
        if !paths.contains(path) {
            return Err(format!(
                "icon reference {path:?} must name a publication path in presentation.assets"
            ));
        }
        let media = media_type(path);
        let valid_type = match icon.rel {
            SiteIconRel::Manifest => {
                matches!(media, "application/manifest+json" | "application/json")
            }
            SiteIconRel::Icon | SiteIconRel::AppleTouchIcon => media.starts_with("image/"),
        };
        if !valid_type || icon.media_type.as_deref().is_some_and(|hint| hint != media) {
            return Err(format!(
                "icon reference {path:?} has an incompatible relationship or MIME type"
            ));
        }
        if let Some(sizes) = &icon.sizes
            && (icon.rel == SiteIconRel::Manifest || !valid_sizes(sizes))
        {
            return Err(format!("icon reference {path:?} has invalid sizes"));
        }
        if !links.insert((icon.rel.as_str(), path, icon.sizes.as_deref())) {
            return Err(format!("duplicate icon reference: {path:?}"));
        }
    }
    Ok(())
}

fn valid_sizes(sizes: &str) -> bool {
    sizes == "any"
        || (!sizes.is_empty()
            && sizes.split(' ').all(|size| {
                size.split_once('x').is_some_and(|(width, height)| {
                    [width, height].iter().all(|dimension| {
                        dimension.bytes().all(|byte| byte.is_ascii_digit())
                            && dimension.parse::<u32>().is_ok_and(|value| value > 0)
                    })
                })
            }))
}

fn overlaps(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn validate_routes(site: &Site<'_>) -> Result<(), SiteError> {
    validate_declarations(site.presentation).map_err(SiteError::Presentation)?;
    let mut occupied: BTreeSet<String> = [
        "assets",
        "__diplodocus",
        ".diplodocus-site",
        "_redirects",
        "sitemap.xml",
        "robots.txt",
        "documents.json",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let mut aliases = BTreeSet::new();
    for route in site.pages.keys() {
        occupied.insert(route.clone());
        occupied.insert(format!("{}.md", route.strip_suffix(".html").unwrap()));
        // Preview accepts both file URLs and their clean equivalents.
        let clean = crate::site::clean_route(route);
        if !clean.is_empty() {
            aliases.insert(clean.trim_end_matches('/').to_owned());
        }
    }
    for source in site.redirects.keys() {
        let decoded = percent_encoding::percent_decode_str(source)
            .decode_utf8()
            .map_err(|_| SiteError::Evidence)?;
        aliases.insert(decoded.trim_matches('/').to_owned());
    }
    for path in site.public_assets.keys() {
        if occupied.iter().any(|other| overlaps(path, other))
            || aliases.iter().any(|other| {
                other == path
                    || other
                        .strip_prefix(path)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        {
            return Err(SiteError::Presentation(format!(
                "public asset {path:?} conflicts with a generated page, redirect, or reserved output"
            )));
        }
    }
    Ok(())
}
