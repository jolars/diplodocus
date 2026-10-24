use std::fmt::Write;
use std::sync::OnceLock;

use crate::ir::{PackageMetadata, PackagePageRole};
use crate::site::Site;

use super::html::escape;

pub(super) fn is_overview(site: &Site<'_>, route: &str, owner: Option<&str>) -> bool {
    owner.is_some_and(|id| {
        route == format!("packages/{}/index.html", site.workspace.packages[id].slug)
    })
}

struct Summary {
    fields: Vec<(&'static str, String)>,
    links: Vec<(String, String)>,
    license_url: Option<String>,
}

impl Summary {
    fn field(&mut self, label: &'static str, value: Option<&str>) {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            self.fields.push((label, value.to_owned()));
        }
    }

    fn people(&mut self, label: &'static str, people: &[String]) {
        self.field(label, Some(&people.join(", ")));
    }

    fn external(&mut self, label: &str, destination: &str) {
        if let Ok(url) = url::Url::parse(destination)
            && matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
        {
            let link = (label.to_owned(), destination.to_owned());
            if !self.links.contains(&link) {
                self.links.push(link);
            }
        }
    }
}

fn summary(site: &Site<'_>, route: &str, id: &str) -> Summary {
    let package = &site.workspace.packages[id];
    let mut summary = Summary {
        fields: Vec::new(),
        links: Vec::new(),
        license_url: None,
    };
    summary.field(
        "Package",
        Some(
            package
                .metadata
                .as_ref()
                .map_or(package.name.as_str(), PackageMetadata::name),
        ),
    );
    summary.field(
        "Language",
        Some(match package.ecosystem.as_str() {
            "r" => "R",
            "python" => "Python",
            "julia" => "Julia",
            "rust" => "Rust",
            other => other,
        }),
    );
    summary.field("Version", package.version.as_deref());
    let license_page = format!("packages/{}/license.html", package.slug);
    summary.license_url = site
        .pages
        .contains_key(&license_page)
        .then(|| site.url(route, &license_page));
    let license = package
        .metadata
        .as_ref()
        .and_then(PackageMetadata::license)
        .or_else(|| summary.license_url.as_ref().map(|_| "View license"));
    summary.field("License", license);
    match &package.metadata {
        Some(PackageMetadata::R(metadata)) => {
            let description = &metadata.description;
            if let Some(maintainer) = description.fields.get("Maintainer") {
                summary.field("Maintainer", Some(&maintainer.value));
            } else if let Some(runtime) = &metadata.runtime {
                if runtime.maintainers.is_empty() {
                    summary.people("Authors", &runtime.authors);
                } else {
                    summary.people("Maintainer", &runtime.maintainers);
                }
            } else {
                summary.field(
                    "Authors",
                    description
                        .fields
                        .get("Author")
                        .map(|field| field.value.as_str()),
                );
            }
            static URLS: OnceLock<regex::Regex> = OnceLock::new();
            let urls = URLS.get_or_init(|| {
                regex::Regex::new(r"(https?://[^\s,]+)(?:\s+\(([^)]*)\))?")
                    .expect("DESCRIPTION URL pattern")
            });
            for key in ["URL", "BugReports"] {
                if let Some(field) = description.fields.get(key) {
                    for entry in urls.captures_iter(&field.value) {
                        let label = entry.get(2).map(|label| label.as_str()).unwrap_or(
                            if key == "BugReports" {
                                "Report an issue"
                            } else {
                                "Website"
                            },
                        );
                        summary.external(label, &entry[1]);
                    }
                }
            }
        }
        Some(PackageMetadata::Python(metadata)) => {
            if metadata.maintainers.is_empty() {
                summary.people("Authors", &metadata.authors);
            } else {
                summary.people("Maintainer", &metadata.maintainers);
            }
            for (label, url) in &metadata.urls {
                summary.external(label, url);
            }
        }
        Some(PackageMetadata::Julia(metadata)) => {
            summary.people("Authors", &metadata.authors);
            for (key, label) in [("homepage", "Website"), ("repository", "Source repository")] {
                if let Some(url) = metadata
                    .fields
                    .get(key)
                    .and_then(|field| field.value.as_str())
                {
                    summary.external(label, url);
                }
            }
        }
        Some(PackageMetadata::Rust(metadata)) => {
            summary.people("Authors", &metadata.authors);
            for (label, url) in &metadata.urls {
                summary.external(label, url);
            }
        }
        None => {}
    }
    if let Some(url) = &site.workspace.repositories[&package.repository].canonical_url {
        summary.external("Source repository", url);
    }
    for role in [
        PackagePageRole::News,
        PackagePageRole::Authors,
        PackagePageRole::License,
        PackagePageRole::Community,
    ] {
        let destination = format!("packages/{}/{}", package.slug, role.filename());
        if site.pages.contains_key(&destination) {
            summary
                .links
                .push((role.title().into(), site.url(route, &destination)));
        }
    }
    summary
}

pub(super) fn panel(site: &Site<'_>, route: &str, id: &str) -> String {
    let summary = summary(site, route, id);
    let mut html = String::from(
        "<aside class=\"package-metadata\" aria-label=\"Package information\"><h2 class=\"visually-hidden\">Package information</h2><dl>",
    );
    for (label, value) in summary.fields {
        let class = if matches!(label, "Authors" | "Maintainer") {
            " class=\"package-metadata-people\""
        } else {
            ""
        };
        write!(html, "<div{class}><dt>{}</dt><dd>", escape(label),).unwrap();
        if label == "License"
            && let Some(url) = &summary.license_url
        {
            write!(html, "<a href=\"{}\">{}</a>", escape(url), escape(&value)).unwrap();
        } else {
            html.push_str(&escape(&value));
        }
        html.push_str("</dd></div>");
    }
    html.push_str("</dl>");
    if !summary.links.is_empty() {
        html.push_str("<ul>");
        for (label, url) in summary.links {
            write!(html, "<li><a href=\"{}\">", escape(&url),).unwrap();
            link_icon(&mut html, &label);
            html.push_str(&escape(&label));
            html.push_str("</a></li>");
        }
        html.push_str("</ul>");
    }
    html.push_str("</aside>");
    html
}

pub(super) fn markdown(site: &Site<'_>, route: &str, id: &str) -> String {
    let summary = summary(site, route, id);
    let mut output = String::new();
    for (label, value) in summary.fields {
        let value = super::markdown::escape(&value);
        if label == "License"
            && let Some(url) = &summary.license_url
        {
            writeln!(
                output,
                "- **{label}:** [{value}]({})",
                super::markdown::link_url(url)
            )
            .unwrap();
        } else {
            writeln!(output, "- **{label}:** {value}").unwrap();
        }
    }
    output.push('\n');
    for (label, url) in summary.links {
        writeln!(
            output,
            "- [{}]({})",
            super::markdown::escape(&label),
            super::markdown::link_url(&url)
        )
        .unwrap();
    }
    output.push('\n');
    output
}

fn link_icon(html: &mut String, label: &str) {
    let label = label.to_ascii_lowercase();
    let paths = if label.contains("issue") || label.contains("bug") {
        "<rect x=\"6\" y=\"5\" width=\"12\" height=\"15\" rx=\"6\"></rect><path d=\"m8 2 2 3m6-3-2 3M3 7l3 2m12 0 3-2M2 12h4m12 0h4M3 17l3-2m12 0 3 2M12 8v12\"></path>"
    } else if label.contains("source") || label.contains("repository") || label.contains("github") {
        "<path d=\"m8 6-6 6 6 6m8-12 6 6-6 6\"></path>"
    } else if label.contains("license") || label.contains("licence") {
        "<path d=\"M12 3v18m-5 0h10M4 7h16M6 7l-3 7h6L6 7Zm12 0-3 7h6l-3-7Z\"></path>"
    } else if label.contains("author")
        || label.contains("citation")
        || label.contains("community")
        || label.contains("contribut")
    {
        "<circle cx=\"9\" cy=\"7\" r=\"4\"></circle><path d=\"M2 21v-2a7 7 0 0 1 14 0v2m1-18a4 4 0 0 1 0 8m5 10v-2a7 7 0 0 0-4-6\"></path>"
    } else if label.contains("documentation") || label == "docs" {
        "<path d=\"M12 5C9 3 6 3 3 4v15c3-1 6-1 9 1 3-2 6-2 9-1V4c-3-1-6-1-9 1Zm0 0v15\"></path>"
    } else if label.contains("news") || label.contains("release") || label.contains("changelog") {
        "<rect x=\"4\" y=\"3\" width=\"16\" height=\"18\" rx=\"2\"></rect><path d=\"M8 7h8m-8 4h8m-8 4h5\"></path>"
    } else {
        "<circle cx=\"12\" cy=\"12\" r=\"9\"></circle><path d=\"M3 12h18m-9-9a18 18 0 0 1 0 18 18 18 0 0 1 0-18Z\"></path>"
    };
    html.push_str("<svg class=\"package-link-icon\" aria-hidden=\"true\" focusable=\"false\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.75\" stroke-linecap=\"round\" stroke-linejoin=\"round\">");
    html.push_str(paths);
    html.push_str("</svg>");
}

pub(super) fn search_text(site: &Site<'_>, id: &str) -> String {
    let mut values: Vec<_> = summary(site, "index.html", id)
        .fields
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    if let Some(metadata) = &site.workspace.packages[id].metadata {
        values.extend(metadata.description().map(str::to_owned));
        if let PackageMetadata::R(metadata) = metadata {
            values.push(metadata.description.title.clone());
            values.extend(
                metadata
                    .description
                    .fields
                    .get("Author")
                    .map(|field| field.value.clone()),
            );
            if let Some(runtime) = &metadata.runtime {
                values.extend(runtime.authors.iter().cloned());
            }
        } else if let PackageMetadata::Python(metadata) = metadata {
            values.extend(metadata.authors.iter().cloned());
        }
    }
    values.join("\n")
}
