//! C++ member ownership and stable overload anchors shared by all renderers.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::*;
use crate::validation::{DocumentIdentity, ResolvedDocument};

use super::members::{ApiMember, ApiPage, ApiSection};

pub(crate) fn data(item: &Item) -> Option<&CppItemData> {
    match &item.language_data {
        Some(ItemLanguageData::Cpp(data)) => Some(data),
        _ => None,
    }
}

pub(crate) fn member_owners(package: &Package) -> BTreeMap<String, String> {
    package
        .items
        .iter()
        .filter_map(|(id, item)| {
            let mut owner = data(item)?.owner.as_ref()?.item.as_str();
            let mut visited = BTreeSet::from([id.as_str()]);
            loop {
                if !visited.insert(owner) {
                    return None;
                }
                let parent = package.items.get(owner)?;
                if let Some(next) = data(parent)?.owner.as_ref() {
                    owner = &next.item;
                } else {
                    return Some((id.clone(), owner.to_owned()));
                }
            }
        })
        .collect()
}

pub(crate) fn member_anchor(id: &str) -> String {
    format!(
        "cpp-{}",
        crate::provenance::fingerprint_bytes(id.as_bytes()).value
    )
}

pub(crate) fn page<'a>(
    package_id: &str,
    package: &'a Package,
    item_id: &str,
    records: &BTreeMap<&DocumentIdentity, &'a ResolvedDocument>,
) -> Option<ApiPage<'a>> {
    data(&package.items[item_id])?;
    let owners = member_owners(package);
    let member = |id: &str| {
        let reference = ItemReference {
            package: package_id.into(),
            item: id.into(),
        };
        let identity = DocumentIdentity::Item {
            item: reference.clone(),
        };
        ApiMember {
            reference,
            item: &package.items[id],
            anchor: member_anchor(id),
            resolved: records.get(&identity).copied(),
            children: vec![],
        }
    };
    let mut groups = BTreeMap::<&str, Vec<ApiMember<'a>>>::new();
    for (id, owner) in &owners {
        if owner != item_id {
            continue;
        }
        let declaration = data(&package.items[id])?;
        if declaration.family.is_some() && declaration.family.as_ref().unwrap().item != item_id {
            continue;
        }
        let mut entry = member(id);
        for overload in &declaration.overloads {
            entry.children.push(member(&overload.item));
        }
        let group = if declaration.family.is_some() {
            "Overloads"
        } else {
            match declaration.declaration {
                CppDeclarationKind::Field | CppDeclarationKind::Variable => "Fields",
                CppDeclarationKind::Enumerator => "Values",
                CppDeclarationKind::TypeAlias => "Types",
                _ => "Methods",
            }
        };
        groups.entry(group).or_default().push(entry);
    }
    let sections = groups
        .into_iter()
        .map(|(title, mut members)| {
            members.sort_by(|a, b| {
                (&a.item.name, &a.reference.item).cmp(&(&b.item.name, &b.reference.item))
            });
            ApiSection {
                title: title.into(),
                anchor: format!("cpp-{}", title.to_lowercase()),
                implementation: None,
                documentation: None,
                members,
            }
        })
        .collect();
    Some(ApiPage { sections })
}
