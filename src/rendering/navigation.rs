//! Shared page navigation and package API trees.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::escape;
use crate::ir::{ItemReference, Package};
use crate::site::{Site, relative_url};
use crate::validation::DocumentIdentity;

pub(super) fn sections(html: &mut String, site: &Site<'_>, route: &str, class: &str) {
    write!(
        html,
        "<nav class=\"global-nav {class}\" aria-label=\"Sections\"><ul>"
    )
    .unwrap();
    let owner = site.pages[route].owner.as_deref();
    section_link(html, route, "index.html", "Guide", owner.is_none());
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
            route,
            &format!("packages/{}/index.html", package.slug),
            package_label(site, package),
            owner == Some(id.as_str()),
        );
    }
    html.push_str("</ul></nav>");
}

fn section_link(html: &mut String, route: &str, destination: &str, label: &str, active: bool) {
    // The current section stays selected even when its landing page is not open.
    let current = if active { " aria-current=\"true\"" } else { "" };
    write!(
        html,
        "<li><a href=\"{}\"{current}>{}</a></li>",
        escape(&relative_url(route, destination)),
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
            _ => &package.name,
        }
    } else {
        &package.name
    }
}

pub(super) fn contextual(html: &mut String, site: &Site<'_>, route: &str) {
    if let Some(owner) = site.pages[route].owner.as_deref() {
        package_pages(html, site, route, owner);
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
        group(html, route, "Shared concepts", &concepts);
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
    let overview = format!("{base}index.html");
    let mut root = vec![(overview.clone(), "Overview".to_owned())];
    let mut groups: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (destination, page) in &site.pages {
        if destination == &overview
            || !page.visible
            || page.owner.as_deref() != owner
            || page.item.is_some()
            || page.document.is_none()
            || page.concept.is_some()
        {
            continue;
        }
        let Some(path) = destination.strip_prefix(base) else {
            continue;
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
    group(html, route, label, &root);
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
        group(html, route, &label, &pages);
    }
}

pub(super) fn group(html: &mut String, route: &str, label: &str, links: &[(String, String)]) {
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
        link(html, route, destination, title);
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
    let destination = &site.routes[&DocumentIdentity::Item {
        item: ItemReference {
            package: package_id.into(),
            item: item_id.into(),
        },
    }];
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
    for child in &item.children {
        let (html, child_active) = item_tree(site, route, package_id, package, child, visited);
        children.push_str(&html);
        active |= child_active;
    }
    let mut html = String::new();
    if children.is_empty() {
        link_label(
            &mut html,
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
        link(&mut html, route, destination, "Overview");
        html.push_str(&children);
        html.push_str("</ul></details></li>");
    }
    (html, active)
}

fn link(html: &mut String, route: &str, destination: &str, title: &str) {
    link_label(html, route, destination, &escape(title));
}

fn link_label(html: &mut String, route: &str, destination: &str, label: &str) {
    let active = if destination == route {
        " aria-current=\"page\""
    } else {
        ""
    };
    write!(
        html,
        "<li><a href=\"{}\"{active}>{}</a></li>",
        escape(&relative_url(route, destination)),
        label
    )
    .unwrap();
}
