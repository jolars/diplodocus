use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{RenderedFile, html::escape};
use crate::site::{Site, SiteError};

const MAX_URLS: usize = 50_000;
const MAX_BYTES: usize = 52_428_800;
const MAX_LOCATION_LENGTH: usize = 2_048;

pub(super) fn render(
    site: &Site<'_>,
    files: &mut BTreeMap<String, RenderedFile>,
) -> Result<(), SiteError> {
    let Some(base) = &site.base_url else {
        return Ok(());
    };
    let locations: BTreeSet<_> = site
        .pages
        .iter()
        .filter(|(_, page)| page.visible)
        .map(|(route, _)| site.canonical_urls[route].as_str())
        .collect();
    let sitemap = document(locations, MAX_URLS, MAX_BYTES)?;
    files.insert(
        "sitemap.xml".into(),
        RenderedFile {
            bytes: sitemap.into_bytes(),
            media_type: "application/xml; charset=utf-8".into(),
        },
    );
    let url = base
        .join("sitemap.xml")
        .map_err(|error| SiteError::CrawlerMetadata(error.to_string()))?;
    files.insert(
        "robots.txt".into(),
        RenderedFile {
            bytes: format!("User-agent: *\nAllow: /\n\nSitemap: {url}\n").into_bytes(),
            media_type: "text/plain; charset=utf-8".into(),
        },
    );
    Ok(())
}

fn document<'a>(
    locations: impl IntoIterator<Item = &'a str>,
    max_urls: usize,
    max_bytes: usize,
) -> Result<String, SiteError> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for (index, url) in locations.into_iter().enumerate() {
        if index >= max_urls {
            return Err(SiteError::CrawlerMetadata(
                "sitemap exceeds 50,000 URLs".into(),
            ));
        }
        if url.len() >= MAX_LOCATION_LENGTH {
            return Err(SiteError::CrawlerMetadata(
                "sitemap URL must be shorter than 2,048 characters".into(),
            ));
        }
        writeln!(xml, "  <url><loc>{}</loc></url>", escape(url)).unwrap();
        check_size(xml.len() + "</urlset>\n".len(), max_bytes)?;
    }
    xml.push_str("</urlset>\n");
    check_size(xml.len(), max_bytes)?;
    Ok(xml)
}

fn check_size(bytes: usize, max_bytes: usize) -> Result<(), SiteError> {
    if bytes > max_bytes {
        return Err(SiteError::CrawlerMetadata("sitemap exceeds 50 MiB".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_limits_allow_the_boundary_and_reject_the_next_entry_or_byte() {
        let url = "https://example.test/";
        let xml = document([url], 1, MAX_BYTES).unwrap();
        assert!(document([url], 1, xml.len()).is_ok());
        assert!(document([url], 1, xml.len() - 1).is_err());
        assert!(document([url, url], 1, MAX_BYTES).is_err());
        let longest = format!("{url}{}", "a".repeat(MAX_LOCATION_LENGTH - url.len() - 1));
        assert!(document([longest.as_str()], 1, MAX_BYTES).is_ok());
        assert!(document([format!("{longest}a").as_str()], 1, MAX_BYTES).is_err());
    }
}
