//! Shared Julia API grouping for HTML, Markdown, and fragment consumers.

use std::collections::BTreeMap;

use crate::ir::*;

pub(crate) struct JuliaSection {
    pub title: &'static str,
    pub anchor: &'static str,
    pub members: Vec<ItemReference>,
}

fn is_type(item: &Item) -> bool {
    matches!(&item.language_data, Some(ItemLanguageData::Julia(data))
        if matches!(data.declaration, JuliaDeclaration::Type { .. }))
}

fn is_field(item: &Item) -> bool {
    matches!(&item.language_data, Some(ItemLanguageData::Julia(data))
        if matches!(data.declaration, JuliaDeclaration::Field { .. }))
}

pub(crate) fn field_owners(package: &Package) -> BTreeMap<String, String> {
    package
        .items
        .iter()
        .filter(|(_, item)| is_type(item))
        .flat_map(|(id, item)| {
            item.children
                .iter()
                .filter(|child| is_field(&package.items[*child]))
                .map(move |child| (child.clone(), id.clone()))
        })
        .collect()
}

pub(crate) fn field_anchor(item: &Item) -> String {
    format!("field-{}", item.name)
}

pub(crate) fn sections(package_id: &str, package: &Package, item: &Item) -> Vec<JuliaSection> {
    let mut sections = Vec::new();
    if is_type(item) {
        let mut fields: Vec<_> = item
            .children
            .iter()
            .filter(|id| is_field(&package.items[*id]))
            .collect();
        // Static child IDs are sorted by identity. Runtime fields lack source
        // locations, so stable sorting preserves their reflected order.
        fields.sort_by_key(|id| {
            package.items[*id]
                .source_location
                .as_ref()
                .and_then(|source| source.span)
                .map(|span| span.start)
        });
        if !fields.is_empty() {
            sections.push(JuliaSection {
                title: "Fields",
                anchor: "fields",
                members: fields
                    .into_iter()
                    .map(|id| ItemReference {
                        package: package_id.into(),
                        item: id.clone(),
                    })
                    .collect(),
            });
        }
    }
    let methods = methods(item);
    if !methods.is_empty() {
        sections.push(JuliaSection {
            title: "Methods",
            anchor: "methods",
            members: methods.to_vec(),
        });
    }
    sections
}

pub(super) fn methods(item: &Item) -> &[ItemReference] {
    match &item.language_data {
        Some(ItemLanguageData::Julia(data)) => match &data.declaration {
            JuliaDeclaration::Callable {
                role: JuliaCallableRole::Family { methods },
                ..
            } => methods,
            JuliaDeclaration::Type { constructors, .. } => constructors,
            _ => &[],
        },
        _ => &[],
    }
}

pub(crate) fn member_title(item: &Item) -> String {
    item.signatures
        .first()
        .map(|signature| super::signatures::signature(&item.name, &signature.signature, "julia"))
        .unwrap_or_else(|| item.name.clone())
}
