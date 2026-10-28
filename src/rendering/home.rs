//! Landing page content within the shared navigation shell.

use std::fmt::Write;

use super::html::escape;
use super::images::{ImageRole, Images};
use crate::configuration::{HomeActionIcon, HomeDefaults};
use crate::ir::Block;
use crate::site::{Site, SiteError};

pub(super) fn render(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    home: &HomeDefaults,
    images: &Images,
    standalone: bool,
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
                "<a class=\"home-action\" href=\"{}\">",
                escape(&site.url(route, action.path.as_str())),
            )
            .unwrap();
            if let Some(icon) = action.icon {
                render_action_icon(html, icon);
            }
            write!(html, "{}</a>", escape(&action.label)).unwrap();
        }
        html.push_str("</div>");
    }
    html.push_str("</div>");
    if let Some(logo) = site.logo {
        write!(
            html,
            "<div class=\"home-artwork\"><img src=\"{}\" alt=\"\" width=\"384\" height=\"384\"{}></div>",
            escape(&site.url(route, &site.asset_route(&logo.fingerprint.value)?)),
            images.attributes(site, route, &logo.fingerprint.value, ImageRole::Hero, standalone)?,
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

fn render_action_icon(html: &mut String, icon: HomeActionIcon) {
    html.push_str("<svg class=\"home-action-icon\" aria-hidden=\"true\" focusable=\"false\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.75\" stroke-linecap=\"round\" stroke-linejoin=\"round\">");
    html.push_str(match icon {
        HomeActionIcon::ArrowRight => "<path d=\"M4 12h16m-7-7 7 7-7 7\"></path>",
        HomeActionIcon::BookOpen => {
            "<path d=\"M12 5C9 3 6 3 3 4v15c3-1 6-1 9 1 3-2 6-2 9-1V4c-3-1-6-1-9 1Zm0 0v15\"></path>"
        }
        HomeActionIcon::Code => "<path d=\"m8 6-6 6 6 6m8-12 6 6-6 6\"></path>",
        HomeActionIcon::Download => {
            "<path d=\"M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5\"></path>"
        }
        HomeActionIcon::Terminal => {
            "<rect x=\"3\" y=\"4\" width=\"18\" height=\"16\" rx=\"2\"></rect><path d=\"m7 8 4 4-4 4m7 0h3\"></path>"
        }
    });
    html.push_str("</svg>");
}
