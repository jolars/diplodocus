//! Shared page navigation and package API trees.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::escape;
use crate::ir::{ItemReference, Package};
use crate::site::Site;
use crate::validation::DocumentIdentity;

pub(super) fn repositories(html: &mut String, site: &Site<'_>) {
    let mut seen = BTreeSet::new();
    for (id, repository) in &site.workspace.repositories {
        let Some(url) = repository.canonical_url.as_deref() else {
            continue;
        };
        if !url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
            || !seen.insert(url.trim_end_matches('/'))
        {
            continue;
        }
        let label = site
            .presentation
            .footer
            .links
            .iter()
            .find(|link| link.url.trim_end_matches('/') == url.trim_end_matches('/'))
            .map_or(id.as_str(), |link| link.label.as_str());
        if url::Url::parse(url).is_ok_and(|url| url.host_str() == Some("github.com")) {
            write!(html, "<a class=\"repository-link\" href=\"{}\" aria-label=\"{}\" title=\"{}\"><svg aria-hidden=\"true\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"currentColor\"><path d=\"M12 .5C5.37.5 0 5.87 0 12.5c0 5.3 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.043-1.61-4.043-1.61-.546-1.387-1.333-1.756-1.333-1.756-1.09-.745.083-.73.083-.73 1.205.085 1.838 1.237 1.838 1.237 1.07 1.835 2.809 1.305 3.493.998.108-.776.418-1.305.762-1.605-2.665-.3-5.467-1.333-5.467-5.93 0-1.31.468-2.38 1.235-3.22-.124-.303-.535-1.523.117-3.176 0 0 1.008-.322 3.3 1.23a11.51 11.51 0 0 1 3.005-.404c1.02.005 2.045.138 3.005.404 2.29-1.552 3.297-1.23 3.297-1.23.654 1.653.243 2.873.12 3.176.77.84 1.233 1.91 1.233 3.22 0 4.61-2.807 5.625-5.48 5.922.43.372.823 1.102.823 2.222 0 1.606-.015 2.9-.015 3.293 0 .322.216.694.825.576C20.565 22.295 24 17.797 24 12.5c0-6.63-5.373-12-12-12z\"></path></svg></a>", escape(url), escape(label), escape(label)).unwrap();
        } else {
            write!(html, "<a class=\"repository-link\" href=\"{}\">{}<svg aria-hidden=\"true\" width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"><path d=\"M7 7h10v10M7 17 17 7\"></path></svg></a>", escape(url), escape(label)).unwrap();
        }
    }
}

pub(super) fn sections(html: &mut String, site: &Site<'_>, route: &str, class: &str) {
    write!(
        html,
        "<nav class=\"global-nav {class}\" aria-label=\"Sections\"><ul>"
    )
    .unwrap();
    let owner = site.pages[route].owner.as_deref();
    section_link(
        html,
        site,
        route,
        site.guide_route(),
        "Guide",
        owner.is_none() && site.project_section(route).is_none() && site.home(route).is_none(),
    );
    for section in &site.presentation.sections {
        section_link(
            html,
            site,
            route,
            section.path.as_str(),
            &section.label,
            site.project_section(route) == Some(section),
        );
    }
    let mut packages: Vec<_> = site
        .workspace
        .packages
        .iter()
        .filter(|(_, package)| {
            package.visibility != crate::configuration::PackageVisibility::Hidden
        })
        .collect();
    packages.sort_by_key(|(id, package)| {
        let language = match package.ecosystem.as_str() {
            "python" => 0,
            "r" => 1,
            "julia" => 2,
            _ => 3,
        };
        (language, &package.name, *id)
    });
    for (id, package) in packages {
        section_link(
            html,
            site,
            route,
            &format!("packages/{}/index.html", package.slug),
            package_label(site, package),
            owner == Some(id.as_str()),
        );
    }
    html.push_str("</ul></nav>");
}

fn section_link(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    destination: &str,
    label: &str,
    active: bool,
) {
    // The current section stays selected even when its landing page is not open.
    let current = if active { " aria-current=\"true\"" } else { "" };
    write!(
        html,
        "<li><a href=\"{}\"{current}>{}</a></li>",
        escape(&site.url(route, destination)),
        escape(label)
    )
    .unwrap();
}

fn package_label<'a>(site: &Site<'_>, package: &'a Package) -> &'a str {
    let unique_ecosystem = site
        .workspace
        .packages
        .values()
        .filter(|other| {
            other.visibility != crate::configuration::PackageVisibility::Hidden
                && other.ecosystem == package.ecosystem
        })
        .count()
        == 1;
    if unique_ecosystem {
        match package.ecosystem.as_str() {
            "python" => "Python",
            "r" => "R",
            "julia" => "Julia",
            "rust" => "Rust",
            _ => &package.name,
        }
    } else {
        &package.name
    }
}

pub(super) fn contextual(html: &mut String, site: &Site<'_>, route: &str) {
    if let Some(owner) = site.pages[route].owner.as_deref() {
        package_pages(html, site, route, owner);
    } else if let Some(section) = site.project_section(route) {
        document_groups(html, site, route, None, &section.label, section.prefix());
    } else {
        document_groups(html, site, route, None, "Guide", "");
        let concepts = site
            .pages
            .iter()
            .filter(|(_, page)| {
                page.visible
                    && page
                        .concept
                        .is_some_and(|concept| concept.documentation.is_some())
            })
            .map(|(destination, page)| (destination.clone(), page.title.clone()))
            .collect::<Vec<_>>();
        group(html, site, route, "Shared concepts", &concepts);
    }
}

fn document_groups(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    owner: Option<&str>,
    label: &str,
    base: &str,
) {
    let overview = if owner.is_none() {
        site.project_section(route).map_or_else(
            || site.guide_route().to_owned(),
            |section| section.path.as_str().to_owned(),
        )
    } else {
        format!("{base}index.html")
    };
    let mut root = Vec::new();
    if site.pages.get(&overview).is_none_or(|page| page.navigation) {
        root.push((overview.clone(), "Overview".to_owned()));
    }
    let mut groups: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (destination, page) in &site.pages {
        if destination == &overview
            || site.home(destination).is_some()
            || !page.visible
            || !page.navigation
            || page.owner.as_deref() != owner
            || page.item.is_some()
            || page.document.is_none()
            || page.concept.is_some()
            || (owner.is_none() && site.project_section(destination) != site.project_section(route))
        {
            continue;
        }
        let Some(path) = destination.strip_prefix(base) else {
            continue;
        };
        let path = if owner.is_none() && base.is_empty() && site.presentation.home.is_some() {
            site.guide_route()
                .rsplit_once('/')
                .and_then(|(directory, _)| path.strip_prefix(&format!("{directory}/")))
                .unwrap_or(path)
        } else {
            path
        };
        if let Some((directory, _)) = path.split_once('/') {
            groups
                .entry(directory)
                .or_default()
                .push((destination.clone(), page.title.clone()));
        } else {
            root.push((destination.clone(), page.title.clone()));
        }
    }
    group(html, site, route, label, &root);
    for (directory, mut pages) in groups {
        let overview = format!("{base}{directory}/index.html");
        let label = site
            .pages
            .get(&overview)
            .map(|page| page.title.clone())
            .unwrap_or_else(|| {
                let name = directory.replace(['-', '_'], " ");
                let mut chars = name.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            });
        pages.sort_by_key(|(path, _)| (path != &overview, path.clone()));
        for (path, title) in &mut pages {
            if path == &overview {
                *title = "Overview".into();
            }
        }
        group(html, site, route, &label, &pages);
    }
}

pub(super) fn group(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    label: &str,
    links: &[(String, String)],
) {
    if links.is_empty() {
        return;
    }
    write!(
        html,
        "<section class=\"nav-group\" role=\"group\" aria-label=\"{}\"><h2>{}</h2><ul>",
        escape(label),
        escape(label)
    )
    .unwrap();
    for (destination, title) in links {
        link(html, site, route, destination, title);
    }
    html.push_str("</ul></section>");
}

fn package_pages(html: &mut String, site: &Site<'_>, route: &str, id: &str) {
    let package = &site.workspace.packages[id];
    if package.visibility == crate::configuration::PackageVisibility::Hidden {
        return;
    }
    document_groups(
        html,
        site,
        route,
        Some(id),
        package_label(site, package),
        &format!("packages/{}/", package.slug),
    );
    if !package.items.values().any(crate::ir::Item::discoverable) {
        return;
    }
    html.push_str("<section class=\"nav-group\" role=\"group\" aria-label=\"API reference\"><h2>API reference</h2><ul>");
    if package.ecosystem == "r" {
        link(
            html,
            site,
            route,
            &format!("packages/{}/reference/index.html", package.slug),
            "Reference overview",
        );
    }
    let children: BTreeSet<_> = package
        .items
        .values()
        .flat_map(|item| &item.children)
        .collect();
    let mut visited = BTreeSet::new();
    for item_id in package.items.keys().filter(|item| !children.contains(item)) {
        html.push_str(&item_tree(site, route, id, package, item_id, &mut visited).0);
    }
    // Shared aliases and disconnected items still need one reachable entry.
    for item_id in package.items.keys() {
        html.push_str(&item_tree(site, route, id, package, item_id, &mut visited).0);
    }
    html.push_str("</ul></section>");
}

fn item_tree(
    site: &Site<'_>,
    route: &str,
    package_id: &str,
    package: &Package,
    item_id: &str,
    visited: &mut BTreeSet<String>,
) -> (String, bool) {
    if !visited.insert(item_id.to_owned()) {
        return (String::new(), false);
    }
    let item = &package.items[item_id];
    if !item.discoverable() {
        return (String::new(), false);
    }
    let reference = ItemReference {
        package: package_id.into(),
        item: item_id.into(),
    };
    if site.overload_anchors.contains_key(&reference) {
        return (String::new(), false);
    }
    let destination = &site.routes[&DocumentIdentity::Item { item: reference }];
    let title = if package.ecosystem == "julia" && item.kind == crate::ir::ItemKind::Method {
        item.signatures
            .first()
            .map(|signature| {
                super::signatures::signature(&item.name, &signature.signature, "julia")
            })
            .unwrap_or_else(|| item.name.clone())
    } else if package.ecosystem == "python" {
        site.pages[destination]
            .title
            .rsplit('.')
            .next()
            .unwrap()
            .to_owned()
    } else {
        item.name.clone()
    };
    let mut active = destination == route;
    let mut children = String::new();
    let expand = match package.ecosystem.as_str() {
        "rust" => item.kind == crate::ir::ItemKind::Module,
        "python" => item.kind != crate::ir::ItemKind::Class,
        _ => true,
    };
    if expand {
        for child in &item.children {
            let (html, child_active) = item_tree(site, route, package_id, package, child, visited);
            children.push_str(&html);
            active |= child_active;
        }
    }
    let mut html = String::new();
    if children.is_empty() {
        link_label(
            &mut html,
            site,
            route,
            destination,
            &format!("<code>{}</code>", escape(&title)),
        );
    } else {
        let open = if active { " open" } else { "" };
        write!(
            html,
            "<li><details class=\"item-disclosure\"{open}><summary><code>{}</code></summary><ul>",
            escape(&title)
        )
        .unwrap();
        link(&mut html, site, route, destination, "Overview");
        html.push_str(&children);
        html.push_str("</ul></details></li>");
    }
    (html, active)
}

fn link(html: &mut String, site: &Site<'_>, route: &str, destination: &str, title: &str) {
    link_label(html, site, route, destination, &escape(title));
}

fn link_label(html: &mut String, site: &Site<'_>, route: &str, destination: &str, label: &str) {
    let active = if destination == route {
        " aria-current=\"page\""
    } else {
        ""
    };
    write!(
        html,
        "<li><a href=\"{}\"{active}>{}</a></li>",
        escape(&site.url(route, destination)),
        label
    )
    .unwrap();
}
