use std::fmt::Write;

use crate::configuration::{FooterDefaults, FooterLink};

use super::html::escape;

pub(super) fn render(html: &mut String, footer: &FooterDefaults) {
    let mut rows = Vec::new();
    let mut project = Vec::new();
    if let Some(version) = &footer.docs_version {
        project.push(format!("Docs: {}", escape(version)));
    }
    project.extend(footer.links.iter().map(link));
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
    if !notices.is_empty() {
        rows.push(notices.join(" · "));
    }
    if footer.built_with {
        rows.push("Built with <a href=\"https://diplodocus.cc/\">Diplodocus</a>".into());
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
