//! Shared Python class presentation, independent of HTML or Markdown output.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::*;
use crate::validation::{DocumentIdentity, ResolvedDocument};

pub(crate) struct PythonClass<'a> {
    pub bases: Vec<SignatureExpression>,
    pub constructors: Vec<Signature>,
    pub constructor_owner: Option<ItemReference>,
    pub members: Vec<PythonMember<'a>>,
}

pub(crate) struct PythonMember<'a> {
    pub reference: ItemReference,
    pub item: &'a Item,
    pub resolved: Option<&'a ResolvedDocument>,
    pub anchor: String,
    pub inherited: bool,
}

pub(crate) fn member_owners(package: &Package) -> BTreeMap<String, String> {
    package
        .items
        .iter()
        .filter(|(_, item)| class_declaration(item).is_some())
        .flat_map(|(id, item)| {
            item.children
                .iter()
                .filter(|child| package.items[*child].kind != ItemKind::Class)
                .map(move |child| (child.clone(), id.clone()))
        })
        .collect()
}

pub(crate) fn member_anchor(item: &Item) -> String {
    item.name.clone()
}

pub(crate) fn class<'a>(
    package_id: &str,
    package: &'a Package,
    item_id: &str,
    records: &BTreeMap<&DocumentIdentity, &'a ResolvedDocument>,
) -> Option<PythonClass<'a>> {
    let (bases, _) = class_declaration(&package.items[item_id])?;
    let members = members(package, item_id, &mut BTreeSet::new());
    let (constructors, constructor_owner) =
        constructors(package_id, package, item_id, &mut BTreeSet::new());
    Some(PythonClass {
        bases: bases.to_vec(),
        constructors,
        constructor_owner,
        members: members
            .into_iter()
            .filter(|(_, member)| public_member(member))
            .map(|(id, item)| {
                let reference = ItemReference {
                    package: package_id.into(),
                    item: id.into(),
                };
                let identity = DocumentIdentity::Item {
                    item: reference.clone(),
                };
                PythonMember {
                    resolved: records.get(&identity).copied(),
                    reference,
                    item,
                    anchor: member_anchor(item),
                    inherited: !package.items[item_id]
                        .children
                        .iter()
                        .any(|child| child == id),
                }
            })
            .collect(),
    })
}

fn public_member(item: &Item) -> bool {
    if item.kind == ItemKind::Class {
        return false;
    }
    let Some(ItemLanguageData::Python(data)) = &item.language_data else {
        return false;
    };
    if matches!(
        data.declaration,
        PythonDeclaration::Callable {
            role: PythonCallableRole::Overload { .. },
            ..
        }
    ) {
        return false;
    }
    !item.name.starts_with('_')
        || matches!(
            data.declaration,
            PythonDeclaration::Callable {
                binding: PythonCallableKind::Constructor,
                ..
            }
        )
}

fn class_declaration(item: &Item) -> Option<(&[SignatureExpression], &PythonConstructor)> {
    match &item.language_data {
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Class { bases, constructor },
            ..
        })) => Some((bases, constructor)),
        _ => None,
    }
}

fn base_name(base: &SignatureExpression) -> Option<(&str, Option<&ItemReference>)> {
    match base {
        SignatureExpression::Name { name, target } => Some((name, target.as_ref())),
        SignatureExpression::Apply { constructor, .. } => base_name(constructor),
        _ => None,
    }
}

fn bases<'a>(package: &'a Package, item_id: &str) -> Vec<&'a str> {
    class_declaration(&package.items[item_id])
        .into_iter()
        .flat_map(|(bases, _)| bases)
        .filter_map(base_name)
        .filter_map(|(name, target)| {
            target
                .and_then(|target| package.items.get_key_value(&target.item))
                .or_else(|| {
                    package.items.iter().find(|(_, item)| {
                        class_declaration(item).is_some()
                            && (item.qualified_name == name
                                || item
                                    .aliases
                                    .iter()
                                    .any(|alias| alias.qualified_name == name))
                    })
                })
                .filter(|(_, item)| class_declaration(item).is_some())
                .map(|(id, _)| id.as_str())
        })
        .collect()
}

fn linearization<'a>(
    package: &'a Package,
    item_id: &'a str,
    visiting: &mut BTreeSet<String>,
) -> Vec<&'a str> {
    if !visiting.insert(item_id.into()) {
        return Vec::new();
    }
    let direct = bases(package, item_id);
    let mut sequences: Vec<_> = direct
        .iter()
        .map(|base| linearization(package, base, visiting))
        .collect();
    sequences.push(direct);
    let mut result = vec![item_id];
    while sequences.iter().any(|sequence| !sequence.is_empty()) {
        let candidate = sequences
            .iter()
            .filter_map(|sequence| sequence.first().copied())
            .find(|head| {
                !sequences
                    .iter()
                    .any(|sequence| sequence.iter().skip(1).any(|tail| tail == head))
            });
        let Some(candidate) = candidate else {
            break;
        };
        if !result.contains(&candidate) {
            result.push(candidate);
        }
        for sequence in &mut sequences {
            if sequence.first() == Some(&candidate) {
                sequence.remove(0);
            }
        }
    }
    visiting.remove(item_id);
    result
}

fn members<'a>(
    package: &'a Package,
    item_id: &str,
    visiting: &mut BTreeSet<String>,
) -> Vec<(&'a str, &'a Item)> {
    let item_id = package
        .items
        .get_key_value(item_id)
        .expect("Python class")
        .0
        .as_str();
    let mut result = Vec::new();
    // Python's C3 order keeps sibling overrides ahead of a shared ancestor.
    for owner in linearization(package, item_id, visiting) {
        for child in &package.items[owner].children {
            let item = &package.items[child];
            if !result
                .iter()
                .any(|(_, existing): &(&str, &Item)| existing.name == item.name)
            {
                result.push((child.as_str(), item));
            }
        }
    }
    result
}

fn constructors(
    package_id: &str,
    package: &Package,
    item_id: &str,
    visiting: &mut BTreeSet<String>,
) -> (Vec<Signature>, Option<ItemReference>) {
    let item_id = package
        .items
        .get_key_value(item_id)
        .expect("Python class")
        .0
        .as_str();
    for owner in linearization(package, item_id, visiting) {
        let (_, constructor) = class_declaration(&package.items[owner]).expect("Python class");
        let explicit = package.items[owner].children.iter().find(|id| {
            matches!(&package.items[*id].language_data, Some(ItemLanguageData::Python(data))
            if matches!(&data.declaration, PythonDeclaration::Callable {
                binding: PythonCallableKind::Constructor,
                role: PythonCallableRole::Family { .. }, ..
            }))
        });
        let signatures = if let Some(explicit) = explicit {
            member_signatures(&package.items[explicit])
                .into_iter()
                .map(|mut signature| {
                    if let Signature::Callable { returns, .. } = &mut signature {
                        *returns = None;
                    }
                    signature
                })
                .collect()
        } else if matches!(constructor, PythonConstructor::Dataclass { init: true, .. }) {
            let mut fields = Vec::new();
            dataclass_fields(package, owner, &mut fields);
            let mut parameters: Vec<_> = fields.into_iter().filter_map(field_parameter).collect();
            // Dataclasses put keyword-only fields after all positional fields.
            parameters.sort_by_key(|parameter| parameter.kind == ParameterKind::KeywordOnly);
            vec![Signature::Callable {
                parameters,
                returns: None,
            }]
        } else {
            continue;
        };
        return (
            signatures,
            Some(ItemReference {
                package: package_id.into(),
                item: owner.into(),
            }),
        );
    }
    (Vec::new(), None)
}

fn dataclass_fields<'a>(package: &'a Package, item_id: &'a str, result: &mut Vec<&'a Item>) {
    for owner in linearization(package, item_id, &mut BTreeSet::new())
        .into_iter()
        .rev()
    {
        if let Some((_, PythonConstructor::Dataclass { fields, .. })) =
            class_declaration(&package.items[owner])
        {
            for field in fields {
                let item = &package.items[&field.item];
                if let Some(existing) = result
                    .iter_mut()
                    .find(|existing| existing.name == item.name)
                {
                    *existing = item;
                } else {
                    result.push(item);
                }
            }
        }
    }
}

fn field_parameter(item: &Item) -> Option<Parameter> {
    let Signature::Value { annotation, value } = &item.signatures.first()?.signature else {
        return None;
    };
    let mut default = value.clone();
    let mut kind = ParameterKind::PositionalOrKeyword;
    if let Some(SignatureExpression::LanguageSpecific { name, children, .. }) = value
        && name == "call"
        && matches!(children.first(), Some(SignatureExpression::Name { name, .. }) if name == "dataclasses.field")
    {
        default = None;
        for keyword in children.iter().skip(1) {
            if let SignatureExpression::LanguageSpecific { name, children, .. } = keyword {
                let value = children.first();
                match name.as_str() {
                    "keyword:init" if matches!(value, Some(SignatureExpression::Literal { text }) if text == "False") =>
                    {
                        return None;
                    }
                    "keyword:kw_only" if matches!(value, Some(SignatureExpression::Literal { text }) if text == "True") => {
                        kind = ParameterKind::KeywordOnly
                    }
                    "keyword:default" => default = value.cloned(),
                    "keyword:default_factory" => {
                        default = Some(SignatureExpression::Literal {
                            text: "<factory>".into(),
                        })
                    }
                    _ => {}
                }
            }
        }
    }
    Some(Parameter {
        name: item.name.clone(),
        kind,
        annotation: annotation.clone(),
        default,
    })
}

pub(crate) fn member_signatures(item: &Item) -> Vec<Signature> {
    let bound = matches!(&item.language_data, Some(ItemLanguageData::Python(data))
    if matches!(data.declaration, PythonDeclaration::Callable {
        binding: PythonCallableKind::InstanceMethod | PythonCallableKind::ClassMethod | PythonCallableKind::Constructor, ..
    }));
    item.signatures
        .iter()
        .map(|sourced| {
            let mut signature = sourced.signature.clone();
            if bound
                && let Signature::Callable { parameters, .. } = &mut signature
                && parameters.first().is_some_and(|parameter| {
                    matches!(
                        parameter.kind,
                        ParameterKind::PositionalOnly | ParameterKind::PositionalOrKeyword
                    )
                })
            {
                parameters.remove(0);
            }
            signature
        })
        .collect()
}
