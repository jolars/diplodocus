use std::fmt::Write;
use std::io::Cursor;

use crate::site::{Site, SiteError};

use super::html::escape;

pub(super) fn render(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    page_title: &str,
) -> Result<(), SiteError> {
    let (Some(declaration), Some(image)) = (&site.presentation.open_graph, site.open_graph) else {
        return Ok(());
    };
    let asset = site
        .assets
        .get(&image.fingerprint.value)
        .ok_or(SiteError::Evidence)?;
    let image_route = site.asset_route(&image.fingerprint.value)?;
    let image_url = site
        .base_url
        .as_ref()
        .ok_or(SiteError::Evidence)?
        .join(&image_route)
        .map_err(|error| SiteError::CrawlerMetadata(error.to_string()))?;
    let (width, height) = image::ImageReader::new(Cursor::new(&asset.bytes))
        .with_guessed_format()
        .map_err(|_| SiteError::Evidence)?
        .into_dimensions()
        .map_err(|_| SiteError::Evidence)?;
    let title = if page_title == site.title() {
        page_title.to_owned()
    } else {
        format!("{page_title} · {}", site.title())
    };
    for (property, content) in [
        ("og:type", "website"),
        ("og:title", title.as_str()),
        ("og:site_name", site.title()),
        (
            "og:url",
            site.canonical_urls
                .get(route)
                .ok_or(SiteError::Evidence)?
                .as_str(),
        ),
        ("og:image", image_url.as_str()),
        ("og:image:type", asset.media_type.as_str()),
        ("og:image:width", width.to_string().as_str()),
        ("og:image:height", height.to_string().as_str()),
        ("og:image:alt", declaration.alt.as_str()),
    ] {
        write!(
            html,
            "<meta property=\"{property}\" content=\"{}\">",
            escape(content)
        )
        .unwrap();
    }
    if let Some(description) = &site.presentation.description {
        write!(
            html,
            "<meta property=\"og:description\" content=\"{}\">",
            escape(description)
        )
        .unwrap();
        write!(
            html,
            "<meta name=\"twitter:description\" content=\"{}\">",
            escape(description)
        )
        .unwrap();
    }
    for (name, content) in [
        ("twitter:card", "summary_large_image"),
        ("twitter:title", title.as_str()),
        ("twitter:image", image_url.as_str()),
        ("twitter:image:alt", declaration.alt.as_str()),
    ] {
        write!(
            html,
            "<meta name=\"{name}\" content=\"{}\">",
            escape(content)
        )
        .unwrap();
    }
    Ok(())
}
