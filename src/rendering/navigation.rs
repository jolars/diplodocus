//! Shared page navigation and package API trees.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::escape;
use crate::ir::{ItemReference, Package};
use crate::site::{Site, relative_url};
use crate::validation::DocumentIdentity;

pub(super) fn project_pages(html: &mut String, site: &Site<'_>, route: &str) {
    let mut root = vec![("index.html".to_owned(), "Overview".to_owned())];
    let mut groups: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (destination, page) in &site.pages {
        if destination == "index.html"
            || !page.visible
            || page.owner.is_some()
            || page.document.is_none()
            || page.concept.is_some()
        {
            continue;
        }
        if let Some((directory, _)) = destination.split_once('/') {
            groups
                .entry(directory)
                .or_default()
                .push((destination.clone(), page.title.clone()));
        } else {
            root.push((destination.clone(), page.title.clone()));
        }
    }
    group(html, route, "Documentation", &root);
    for (directory, mut pages) in groups {
        let overview = format!("{directory}/index.html");
        let label = site
            .pages
            .get(&overview)
            .map(|page| page.title.clone())
            .unwrap_or_else(|| directory.replace(['-', '_'], " "));
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

pub(super) fn packages(html: &mut String, site: &Site<'_>, route: &str) {
    let packages: Vec<_> = site
        .workspace
        .packages
        .iter()
        .filter(|(_, package)| {
            package.visibility != crate::configuration::PackageVisibility::Hidden
        })
        .collect();
    if packages.is_empty() {
        return;
    }
    html.push_str("<section class=\"nav-group\" role=\"group\" aria-label=\"Reference\"><h2>Reference</h2><ul>");
    for (id, package) in &packages {
        let unique_ecosystem = packages
            .iter()
            .filter(|(_, other)| other.ecosystem == package.ecosystem)
            .count()
            == 1;
        let label = if unique_ecosystem {
            match package.ecosystem.as_str() {
                "python" => "Python",
                "r" => "R",
                _ => &package.name,
            }
        } else {
            &package.name
        };
        let open = if site.pages[route].owner.as_ref() == Some(id) {
            " open"
        } else {
            ""
        };
        write!(html, "<li class=\"nav-package\"><details class=\"package-disclosure\"{open}><summary>{}</summary><ul>", escape(label)).unwrap();
        let overview = format!("packages/{}/index.html", package.slug);
        link(html, route, &overview, "Overview");
        let documents: Vec<_> = site
            .pages
            .iter()
            .filter(|(destination, page)| {
                *destination != &overview
                    && page.visible
                    && page.owner.as_ref() == Some(id)
                    && page.item.is_none()
                    && page.document.is_some()
            })
            .collect();
        if !documents.is_empty() {
            html.push_str("<li class=\"nav-label\">Guides</li>");
            for (destination, page) in documents {
                link(html, route, destination, &page.title);
            }
        }
        if !package.items.is_empty() {
            html.push_str("<li class=\"nav-label\">API</li>");
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
        html.push_str("</ul></details></li>");
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
    let destination = &site.routes[&DocumentIdentity::Item {
        item: ItemReference {
            package: package_id.into(),
            item: item_id.into(),
        },
    }];
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
            &format!("<code>{}</code>", escape(&item.name)),
        );
    } else {
        let open = if active { " open" } else { "" };
        write!(
            html,
            "<li><details class=\"item-disclosure\"{open}><summary><code>{}</code></summary><ul>",
            escape(&item.name)
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
