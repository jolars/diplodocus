//! Navigation between R declarations without merging their semantic identities.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::escape;
use crate::ir::{
    Block, DocumentFormat, Inline, Item, ItemLanguageData, ItemReference, Package, RDeclaration,
    RGenericReference,
};
use crate::site::{Site, SiteError, relative_url};
use crate::validation::{DocumentIdentity, ReferenceTarget};

pub(crate) fn title(item: &Item) -> Option<&[Inline]> {
    let document = item.documentation.as_ref()?;
    if !matches!(&document.source_format, DocumentFormat::Extracted { name } if name == "rd") {
        return None;
    }
    match document.document.blocks.first()? {
        Block::Heading {
            level: 2, inlines, ..
        } => Some(inlines),
        _ => None,
    }
}

pub(crate) fn page_title(item: &Item) -> Option<String> {
    title(item).map(super::html::plain)
}

pub(crate) fn routes(package: &Package) -> BTreeMap<String, String> {
    let stems: BTreeMap<_, _> = package
        .items
        .iter()
        .map(|(id, item)| {
            let name = match &item.language_data {
                Some(ItemLanguageData::R(data))
                    if data
                        .help_topic
                        .as_ref()
                        .is_some_and(|topic| topic.primary.item == *id) =>
                {
                    &data.help_topic.as_ref().unwrap().name
                }
                _ => &item.name,
            };
            let stem = name
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            (id.clone(), stem.trim_matches(['-', '.']).to_owned())
        })
        .collect();
    let mut counts = BTreeMap::new();
    for stem in stems.values() {
        *counts.entry(stem).or_insert(0) += 1;
    }
    stems
        .iter()
        .map(|(id, stem)| {
            let stem = if stem.is_empty() || stem == "index" || counts[stem] > 1 {
                format!(
                    "{}-{}",
                    if stem.is_empty() { "topic" } else { stem },
                    crate::provenance::fingerprint_bytes(id.as_bytes()).value
                )
            } else {
                stem.clone()
            };
            (
                id.clone(),
                format!("packages/{}/reference/{stem}.html", package.slug),
            )
        })
        .collect()
}

pub(super) fn source_links(site: &Site<'_>, item: &Item) -> String {
    let mut links = BTreeMap::new();
    for source in item.source_location.iter().chain(
        item.documentation
            .as_ref()
            .and_then(|doc| doc.source_location.as_ref()),
    ) {
        let target = ReferenceTarget::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
            directory: false,
            fragment: None,
        };
        if let Some(url) = crate::validation::repository_target_url(site.workspace, &target) {
            links.insert(url, source.path.as_str());
        }
    }
    if links.is_empty() {
        return String::new();
    }
    let mut html = String::from("<p class=\"source-links\">Source: ");
    for (index, (url, label)) in links.iter().enumerate() {
        if index > 0 {
            html.push_str(", ");
        }
        write!(
            html,
            "<a href=\"{}\"><code>{}</code></a>",
            escape(url),
            escape(label)
        )
        .unwrap();
    }
    html.push_str("</p>");
    html
}

pub(super) fn index(site: &Site<'_>, route: &str, owner: &str) -> String {
    let package = &site.workspace.packages[owner];
    let mut topics: Vec<_> = package.items.iter().filter(|(id, item)| {
        item.discoverable() && !matches!(&item.language_data, Some(ItemLanguageData::R(data)) if data.help_topic.as_ref().is_some_and(|topic| topic.primary.item != **id))
    }).collect();
    topics.sort_by_key(|(_, item)| &item.name);
    let mut html = String::from("<ul class=\"reference-topics\">");
    for (id, item) in topics {
        let target = &site.routes[&DocumentIdentity::Item {
            item: ItemReference {
                package: owner.into(),
                item: id.clone(),
            },
        }];
        write!(
            html,
            "<li><a href=\"{}\"><code>{}</code></a>",
            escape(&relative_url(route, target)),
            escape(&item.name)
        )
        .unwrap();
        if let Some(title) = page_title(item) {
            write!(html, "<p class=\"topic-title\">{}</p>", escape(&title)).unwrap();
        }
        if let Some(Block::Paragraph { inlines, .. }) = item
            .documentation
            .as_ref()
            .and_then(|document| document.document.blocks.get(1))
        {
            write!(html, "<p>{}</p>", escape(&super::html::plain(inlines))).unwrap();
        }
        html.push_str("</li>");
    }
    html.push_str("</ul>");
    html
}

pub(super) fn navigation(
    site: &Site<'_>,
    route: &str,
    owner: &str,
    item: &Item,
    mut heading: impl FnMut(&str) -> Result<String, SiteError>,
) -> Result<String, SiteError> {
    let Some(ItemLanguageData::R(data)) = &item.language_data else {
        return Ok(String::new());
    };
    let mut html = String::new();
    let mut members = BTreeSet::new();
    if let Some(topic) = &data.help_topic {
        write!(
            html,
            "<p>Help topic: {}</p>",
            link(site, route, &topic.primary, &topic.name)
        )
        .unwrap();
        members.extend(topic.members.iter().cloned());
    }
    match &data.declaration {
        RDeclaration::S3Generic { methods, .. } => members.extend(methods.iter().cloned()),
        RDeclaration::S3Method { generic, .. } => {
            let label = match generic {
                RGenericReference::Workspace { item } => {
                    let declaration = &site.workspace.packages[&item.package].items[&item.item];
                    link(site, route, item, &declaration.qualified_name)
                }
                RGenericReference::External { package, name } => {
                    format!("<code>{}</code>", escape(&format!("{package}::{name}")))
                }
            };
            write!(html, "<p>S3 generic: {label}</p>").unwrap();
            members.extend(site.workspace.packages[owner].items.iter().filter(|(_, sibling)| {
                matches!(&sibling.language_data, Some(ItemLanguageData::R(data)) if matches!(&data.declaration,
                    RDeclaration::S3Method { generic: sibling_generic, .. } if sibling_generic == generic))
            }).map(|(id, _)| ItemReference { package: owner.into(), item: id.clone() }));
        }
        _ => {}
    }
    let mut classes = BTreeMap::<&str, Vec<String>>::new();
    let mut declarations = Vec::new();
    for member in &members {
        let declaration = &site.workspace.packages[&member.package].items[&member.item];
        if !declaration.discoverable() {
            continue;
        }
        let entry = link(site, route, member, &declaration.qualified_name);
        if let Some(ItemLanguageData::R(data)) = &declaration.language_data
            && let RDeclaration::S3Method { class, .. } = &data.declaration
        {
            classes.entry(class).or_default().push(entry);
        } else {
            declarations.push(entry);
        }
    }
    if !declarations.is_empty() {
        html.push_str("<nav aria-label=\"Shared help declarations\">");
        html.push_str(&heading("Declarations")?);
        html.push_str("<ul>");
        for entry in declarations {
            write!(html, "<li>{entry}</li>").unwrap();
        }
        html.push_str("</ul></nav>");
    }
    if !classes.is_empty() {
        html.push_str("<nav aria-label=\"S3 methods by class\">");
        html.push_str(&heading("Methods by class")?);
        for (class, entries) in classes {
            write!(
                html,
                "<section><h3>Class <code>{}</code></h3><ul>",
                escape(class)
            )
            .unwrap();
            for entry in entries {
                write!(html, "<li>{entry}</li>").unwrap();
            }
            html.push_str("</ul></section>");
        }
        html.push_str("</nav>");
    }
    Ok(html)
}

fn link(site: &Site<'_>, route: &str, item: &ItemReference, label: &str) -> String {
    let destination = &site.routes[&DocumentIdentity::Item { item: item.clone() }];
    let current = if destination == route {
        " aria-current=\"page\""
    } else {
        ""
    };
    format!(
        "<a href=\"{}\"{current}><code>{}</code></a>",
        escape(&relative_url(route, destination)),
        escape(label)
    )
}
