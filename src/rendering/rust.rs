//! Shared Rust API grouping and anchors for every output format.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::*;
use crate::validation::{DocumentIdentity, ResolvedDocument};

pub(crate) struct RustPage<'a> {
    pub sections: Vec<RustSection<'a>>,
}

pub(crate) struct RustSection<'a> {
    pub title: String,
    pub anchor: String,
    pub implementation: Option<&'a RustImplementation>,
    pub documentation: Option<RustMember<'a>>,
    pub members: Vec<RustMember<'a>>,
}

pub(crate) struct RustMember<'a> {
    pub reference: ItemReference,
    pub item: &'a Item,
    pub anchor: String,
    pub resolved: Option<&'a ResolvedDocument>,
    pub children: Vec<RustMember<'a>>,
}

impl RustPage<'_> {
    pub fn members(&self) -> impl Iterator<Item = &RustMember<'_>> {
        self.sections
            .iter()
            .flat_map(|section| section.documentation.iter().chain(&section.members))
            .flat_map(|member| std::iter::once(member).chain(&member.children))
    }
}

pub(crate) fn data(item: &Item) -> Option<&RustItemData> {
    match &item.language_data {
        Some(ItemLanguageData::Rust(data)) => Some(data),
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
                if let Some(next) = data(parent).and_then(|data| data.owner.as_ref()) {
                    owner = &next.item;
                } else {
                    return Some((id.clone(), owner.to_owned()));
                }
            }
        })
        .collect()
}

pub(crate) fn member_anchor(package: &Package, item_id: &str) -> String {
    let item = &package.items[item_id];
    let Some(data) = data(item) else {
        return item.name.clone();
    };
    if data.declaration == RustDeclarationKind::Implementation
        && let Some(implementation) = data.owner.as_ref().and_then(|owner| {
            self::data(&package.items[&owner.item])?
                .implementations
                .iter()
                .find(|implementation| {
                    implementation
                        .documentation_item
                        .as_ref()
                        .is_some_and(|reference| reference.item == item_id)
                })
        })
    {
        return implementation_anchor(&implementation.identity);
    }
    let kind = match data.declaration {
        RustDeclarationKind::Variant => "variant",
        RustDeclarationKind::Field => "field",
        RustDeclarationKind::AssociatedType | RustDeclarationKind::TypeAlias => "associated-type",
        RustDeclarationKind::Constant => "associated-constant",
        RustDeclarationKind::Method | RustDeclarationKind::Function => "method",
        _ => "member",
    };
    let mut anchor = format!("{kind}-{}", item.name);
    if let Some(owner) = &data.owner
        && package
            .items
            .get(&owner.item)
            .and_then(self::data)
            .is_some_and(|data| data.declaration == RustDeclarationKind::Variant)
    {
        anchor = format!("{}-{anchor}", member_anchor(package, &owner.item));
    }
    if let Some(implementation) = &data.implementation {
        anchor.push('-');
        anchor.push_str(&crate::provenance::fingerprint_bytes(implementation.as_bytes()).value);
    }
    anchor
}

pub(crate) fn page<'a>(
    package_id: &str,
    package: &'a Package,
    item_id: &str,
    records: &BTreeMap<&DocumentIdentity, &'a ResolvedDocument>,
) -> Option<RustPage<'a>> {
    let declaration = data(&package.items[item_id])?;
    let owners = member_owners(package);
    let member = |id: &str| {
        let reference = ItemReference {
            package: package_id.into(),
            item: id.into(),
        };
        let identity = DocumentIdentity::Item {
            item: reference.clone(),
        };
        RustMember {
            reference,
            item: &package.items[id],
            anchor: member_anchor(package, id),
            resolved: records.get(&identity).copied(),
            children: Vec::new(),
        }
    };
    let mut groups = BTreeMap::<u8, Vec<RustMember<'a>>>::new();
    let mut implementations = BTreeMap::<&str, Vec<RustMember<'a>>>::new();
    for (id, owner) in &owners {
        if owner != item_id || !package.items[id].discoverable() {
            continue;
        }
        let item_data = data(&package.items[id]).expect("Rust member");
        if item_data.declaration == RustDeclarationKind::Implementation {
            continue;
        }
        if let Some(parent) = &item_data.owner
            && data(&package.items[&parent.item])
                .is_some_and(|data| data.declaration == RustDeclarationKind::Variant)
        {
            continue;
        }
        let mut entry = member(id);
        if item_data.declaration == RustDeclarationKind::Variant {
            entry.children = package
                .items
                .iter()
                .filter(|(_, item)| {
                    item.discoverable()
                        && data(item)
                            .and_then(|data| data.owner.as_ref())
                            .is_some_and(|owner| owner.item == *id)
                })
                .map(|(id, _)| member(id))
                .collect();
        }
        if let Some(implementation) = item_data.implementation.as_deref() {
            implementations
                .entry(implementation)
                .or_default()
                .push(entry);
        } else {
            let group = match item_data.declaration {
                RustDeclarationKind::Field => 0,
                RustDeclarationKind::Variant => 1,
                RustDeclarationKind::AssociatedType | RustDeclarationKind::TypeAlias => 2,
                RustDeclarationKind::Constant | RustDeclarationKind::Static => 3,
                _ => 4,
            };
            groups.entry(group).or_default().push(entry);
        }
    }
    let mut sections = Vec::new();
    for (group, mut members) in groups {
        members.sort_by(|a, b| {
            (&a.item.name, &a.reference.item).cmp(&(&b.item.name, &b.reference.item))
        });
        let (title, anchor) = [
            ("Fields", "fields"),
            ("Variants", "variants"),
            ("Associated types", "associated-types"),
            ("Associated constants", "associated-constants"),
            ("Methods", "methods"),
        ][usize::from(group)];
        sections.push(RustSection {
            title: title.into(),
            anchor: anchor.into(),
            implementation: None,
            documentation: None,
            members,
        });
    }
    let mut declarations: Vec<_> = declaration.implementations.iter().collect();
    declarations.sort_by(|a, b| {
        (a.is_blanket, a.is_synthetic, &a.trait_name, &a.identity).cmp(&(
            b.is_blanket,
            b.is_synthetic,
            &b.trait_name,
            &b.identity,
        ))
    });
    for implementation in declarations {
        let mut members = implementations
            .remove(implementation.identity.as_str())
            .unwrap_or_default();
        members.sort_by(|a, b| {
            (&a.item.name, &a.reference.item).cmp(&(&b.item.name, &b.reference.item))
        });
        sections.push(RustSection {
            title: implementation.trait_name.as_ref().map_or_else(
                || "Inherent implementation".into(),
                |name| format!("Implementation of {name}"),
            ),
            anchor: implementation_anchor(&implementation.identity),
            documentation: implementation
                .documentation_item
                .as_ref()
                .map(|reference| member(&reference.item)),
            implementation: Some(implementation),
            members,
        });
    }
    Some(RustPage { sections })
}

fn implementation_anchor(identity: &str) -> String {
    format!(
        "impl-{}",
        crate::provenance::fingerprint_bytes(identity.as_bytes()).value
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, declaration: RustDeclarationKind, owner: Option<&str>) -> Item {
        Item {
            kind: match declaration {
                RustDeclarationKind::Field => ItemKind::Field,
                RustDeclarationKind::Method => ItemKind::Method,
                _ => ItemKind::Type,
            },
            name: name.into(),
            qualified_name: format!("demo::{name}"),
            language_data: Some(ItemLanguageData::Rust(RustItemData {
                declaration,
                public_path: format!("demo::{name}"),
                owner: owner.map(|item| ItemReference {
                    package: "demo".into(),
                    item: item.into(),
                }),
                implementation: None,
                implementations: Vec::new(),
                attributes: Vec::new(),
            })),
            aliases: Vec::new(),
            signatures: Vec::new(),
            documentation: None,
            source_location: None,
            children: Vec::new(),
            provenance: Vec::new(),
        }
    }

    fn package() -> Package {
        Package {
            slug: "rust".into(),
            name: "demo".into(),
            ecosystem: "rust".into(),
            version: None,
            metadata: None,
            repository: "repo".into(),
            path: None,
            metadata_path: crate::diagnostics::DiagnosticPath::try_from("Cargo.toml").unwrap(),
            kind: Default::default(),
            visibility: Default::default(),
            extraction_targets: BTreeMap::new(),
            items: BTreeMap::from([
                (
                    "result".into(),
                    item("Result", RustDeclarationKind::Enum, None),
                ),
                (
                    "left".into(),
                    item("Left", RustDeclarationKind::Variant, Some("result")),
                ),
                (
                    "right".into(),
                    item("Right", RustDeclarationKind::Variant, Some("result")),
                ),
                (
                    "left-field".into(),
                    item("value", RustDeclarationKind::Field, Some("left")),
                ),
                (
                    "right-field".into(),
                    item("value", RustDeclarationKind::Field, Some("right")),
                ),
                (
                    "left-method".into(),
                    item("convert", RustDeclarationKind::Method, Some("result")),
                ),
                (
                    "right-method".into(),
                    item("convert", RustDeclarationKind::Method, Some("result")),
                ),
            ]),
        }
    }

    #[test]
    fn variant_fields_share_the_enum_page_and_keep_distinct_anchors() {
        let package = package();
        let owners = member_owners(&package);
        assert_eq!(owners["left-field"], "result");
        assert_eq!(owners["right-field"], "result");
        assert_eq!(
            member_anchor(&package, "left-field"),
            "variant-Left-field-value"
        );
        assert_eq!(
            member_anchor(&package, "right-field"),
            "variant-Right-field-value"
        );
        let page = page("demo", &package, "result", &BTreeMap::new()).unwrap();
        assert_eq!(page.sections[0].title, "Variants");
        assert_eq!(page.sections[0].members.len(), 2);
        assert_eq!(page.sections[0].members[0].children.len(), 1);
    }

    #[test]
    fn methods_with_equal_names_are_grouped_by_semantic_implementation() {
        let mut package = package();
        let implementations: Vec<_> = ["impl Left for Result", "impl Right for Result"]
            .into_iter()
            .enumerate()
            .map(|(index, identity)| {
                let member_id = ["left-method", "right-method"][index];
                let Some(ItemLanguageData::Rust(data)) =
                    &mut package.items.get_mut(member_id).unwrap().language_data
                else {
                    unreachable!()
                };
                data.implementation = Some(identity.into());
                RustImplementation {
                    identity: identity.into(),
                    signature: SignatureExpression::Literal {
                        text: identity.into(),
                    },
                    trait_name: Some(["Left", "Right"][index].into()),
                    trait_target: None,
                    documentation_item: None,
                    source_location: None,
                    members: vec![ItemReference {
                        package: "demo".into(),
                        item: member_id.into(),
                    }],
                    is_synthetic: false,
                    is_blanket: false,
                }
            })
            .collect();
        let Some(ItemLanguageData::Rust(data)) =
            &mut package.items.get_mut("result").unwrap().language_data
        else {
            unreachable!()
        };
        data.implementations = implementations;
        let left_anchor = member_anchor(&package, "left-method");
        let right_anchor = member_anchor(&package, "right-method");
        assert_ne!(left_anchor, right_anchor);
        assert!(left_anchor.starts_with("method-convert-"));
        let page = page("demo", &package, "result", &BTreeMap::new()).unwrap();
        assert_eq!(page.sections[1].title, "Implementation of Left");
        assert_eq!(page.sections[2].title, "Implementation of Right");
        assert_eq!(page.sections[1].members[0].anchor, left_anchor);
        assert_eq!(page.members().count(), 6);
    }
}
