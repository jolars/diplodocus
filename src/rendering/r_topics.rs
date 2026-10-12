//! Navigation between R declarations without merging their semantic identities.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::escape;
use crate::ir::{Item, ItemLanguageData, ItemReference, RDeclaration, RGenericReference};
use crate::site::{Site, SiteError, relative_url};
use crate::validation::DocumentIdentity;

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
