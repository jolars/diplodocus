use std::fmt::Write;
use std::sync::OnceLock;

use crate::ir::{PackageMetadata, PackagePageRole};
use crate::site::{Site, relative_url};

use super::html::escape;

pub(super) fn is_overview(site: &Site<'_>, route: &str, owner: Option<&str>) -> bool {
    owner.is_some_and(|id| {
        let package = &site.workspace.packages[id];
        package.metadata.is_some() && route == format!("packages/{}/index.html", package.slug)
    })
}

pub(super) fn panel(site: &Site<'_>, route: &str, id: &str) -> String {
    let package = &site.workspace.packages[id];
    let Some(PackageMetadata::R(metadata)) = &package.metadata else {
        return String::new();
    };
    let description = &metadata.description;
    let maintainer = description
        .fields
        .get("Maintainer")
        .map(|field| field.value.clone())
        .or_else(|| {
            metadata
                .runtime
                .as_ref()
                .filter(|runtime| !runtime.maintainers.is_empty())
                .map(|runtime| runtime.maintainers.join(", "))
        });
    let mut html = String::from(
        "<aside class=\"package-metadata\" aria-label=\"Package information\"><h2>Package information</h2><dl>",
    );
    for (label, value) in [
        ("Package", Some(description.name.as_str())),
        ("Version", Some(description.version.as_str())),
        ("License", description.license.as_deref()),
        ("Maintainer", maintainer.as_deref()),
    ] {
        if let Some(value) = value {
            write!(html, "<dt>{}</dt><dd>{}</dd>", escape(label), escape(value)).unwrap();
        }
    }
    html.push_str("</dl><ul>");
    for role in [
        PackagePageRole::News,
        PackagePageRole::Authors,
        PackagePageRole::License,
        PackagePageRole::Community,
    ] {
        let destination = format!("packages/{}/{}", package.slug, role.filename());
        if site.pages.contains_key(&destination) {
            link(&mut html, &relative_url(route, &destination), role.title());
        }
    }
    static URLS: OnceLock<regex::Regex> = OnceLock::new();
    let urls = URLS.get_or_init(|| {
        regex::Regex::new(r"(https?://[^\s,]+)(?:\s+\(([^)]*)\))?")
            .expect("DESCRIPTION URL pattern")
    });
    for key in ["URL", "BugReports"] {
        if let Some(field) = description.fields.get(key) {
            for entry in urls.captures_iter(&field.value) {
                let url = &entry[1];
                if let Ok(parsed) = url::Url::parse(url)
                    && parsed.host_str().is_some()
                {
                    let label = entry.get(2).map(|label| label.as_str()).unwrap_or_else(|| {
                        if key == "BugReports" {
                            "Report an issue"
                        } else {
                            parsed.host_str().unwrap_or("Website")
                        }
                    });
                    link(&mut html, url, label);
                }
            }
        }
    }
    if let Some(url) = &site.workspace.repositories[&package.repository].canonical_url
        && let Ok(parsed) = url::Url::parse(url)
        && matches!(parsed.scheme(), "http" | "https")
    {
        link(&mut html, url, "Source repository");
    }
    html.push_str("</ul></aside>");
    html
}

fn link(html: &mut String, url: &str, label: &str) {
    write!(
        html,
        "<li><a href=\"{}\">{}</a></li>",
        escape(url),
        escape(label)
    )
    .unwrap();
}

pub(super) fn search_text(site: &Site<'_>, id: &str) -> String {
    let Some(PackageMetadata::R(metadata)) = &site.workspace.packages[id].metadata else {
        return String::new();
    };
    let description = &metadata.description;
    std::iter::once(description.name.as_str())
        .chain(std::iter::once(description.title.as_str()))
        .chain(description.description.as_deref())
        .chain(description.license.as_deref())
        .chain(
            description
                .fields
                .get("Author")
                .map(|field| field.value.as_str()),
        )
        .chain(
            description
                .fields
                .get("Maintainer")
                .map(|field| field.value.as_str()),
        )
        .chain(
            metadata
                .runtime
                .iter()
                .flat_map(|runtime| runtime.authors.iter().map(String::as_str)),
        )
        .collect::<Vec<_>>()
        .join("\n")
}
