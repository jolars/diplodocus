//! Landing page content within the shared navigation shell.

use std::fmt::Write;

use super::html::escape;
use crate::configuration::HomeDefaults;
use crate::ir::Block;
use crate::site::{Site, SiteError};

pub(super) fn render(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    home: &HomeDefaults,
) -> Result<(), SiteError> {
    // Preserve links to the authored title when the hero supplies its heading.
    let anchor = site.pages[route]
        .document
        .and_then(|document| match document.document.blocks.first() {
            Some(Block::Heading {
                level: 1,
                attributes,
                ..
            }) => attributes.identifier.as_ref(),
            _ => None,
        })
        .map(|identifier| format!(" id=\"{}\"", escape(&identifier.value)))
        .unwrap_or_default();
    html.push_str("<section class=\"home-hero\"><div class=\"home-introduction\">");
    write!(
        html,
        "<p class=\"home-name\">{}</p><h1{anchor}>{}</h1><p class=\"home-description\">{}</p>",
        escape(site.title()),
        escape(&home.tagline),
        escape(&home.description),
    )
    .unwrap();
    if !home.actions.is_empty() {
        html.push_str("<div class=\"home-actions\">");
        for action in &home.actions {
            write!(
                html,
                "<a class=\"home-action\" href=\"{}\">{}<span aria-hidden=\"true\"> &rarr;</span></a>",
                escape(&site.url(route, action.path.as_str())),
                escape(&action.label),
            )
            .unwrap();
        }
        html.push_str("</div>");
    }
    html.push_str("</div>");
    if let Some(logo) = site.logo {
        write!(
            html,
            "<div class=\"home-artwork\"><img src=\"{}\" alt=\"\" width=\"384\" height=\"384\"></div>",
            escape(&site.url(route, &site.asset_route(&logo.fingerprint.value)?)),
        )
        .unwrap();
    }
    html.push_str("</section>");
    if !home.features.is_empty() {
        html.push_str("<section class=\"home-features\" aria-label=\"Features\">");
        for feature in &home.features {
            write!(
                html,
                "<a class=\"home-feature\" href=\"{}\"><h2>{}<span aria-hidden=\"true\"> &nearr;</span></h2><p>{}</p></a>",
                escape(&site.url(route, feature.path.as_str())),
                escape(&feature.title),
                escape(&feature.description),
            )
            .unwrap();
        }
        html.push_str("</section>");
    }
    Ok(())
}
