use std::fmt::Write;

use crate::configuration::FooterLink;
use crate::site::Site;

use super::html::escape;

pub(super) fn render(html: &mut String, site: &Site<'_>) {
    let footer = &site.presentation.footer;
    let mut rows = Vec::new();
    let mut project = Vec::new();
    if let Some(version) = &footer.docs_version {
        project.push(format!("Docs: {}", escape(version)));
    }
    project.extend(
        footer
            .links
            .iter()
            .filter(|link| {
                !site.workspace.repositories.values().any(|repository| {
                    repository.canonical_url.as_deref().is_some_and(|url| {
                        url.trim_end_matches('/') == link.url.trim_end_matches('/')
                    })
                })
            })
            .map(link),
    );
    if !project.is_empty() {
        rows.push(project.join(" · "));
    }

    let mut notices = Vec::new();
    if let Some(copyright) = &footer.copyright {
        notices.push(escape(copyright));
    }
    if !footer.licenses.is_empty() {
        notices.push(
            footer
                .licenses
                .iter()
                .map(link)
                .collect::<Vec<_>>()
                .join(" or "),
        );
    }
    if footer.built_with {
        notices.push("Built with <a href=\"https://diplodocus.cc/\">Diplodocus</a>".into());
    }
    if !notices.is_empty() {
        rows.push(notices.join(" · "));
    }
    if rows.is_empty() {
        return;
    }
    html.push_str("<footer class=\"site-footer\">");
    for row in rows {
        write!(html, "<p>{row}</p>").unwrap();
    }
    html.push_str("</footer>");
}

fn link(link: &FooterLink) -> String {
    format!(
        "<a href=\"{}\">{}</a>",
        escape(&link.url),
        escape(&link.label)
    )
}
