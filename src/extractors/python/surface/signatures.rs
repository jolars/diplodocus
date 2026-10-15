use std::collections::{BTreeMap, BTreeSet};

use crate::ir::*;

pub(super) fn resolve(items: &mut BTreeMap<String, Item>, registry: &IdentityRegistry) {
    let retained: BTreeSet<_> = items.keys().cloned().collect();
    for item in items.values_mut() {
        for signature in &mut item.signatures {
            match &mut signature.signature {
                Signature::Callable {
                    parameters,
                    returns,
                } => {
                    for parameter in parameters {
                        for value in [&mut parameter.annotation, &mut parameter.default]
                            .into_iter()
                            .flatten()
                        {
                            expression(value, registry, &retained);
                        }
                    }
                    if let Some(value) = returns {
                        expression(value, registry, &retained);
                    }
                }
                Signature::Value { annotation, value } => {
                    for value in [annotation, value].into_iter().flatten() {
                        expression(value, registry, &retained);
                    }
                }
                Signature::LanguageSpecific { syntax, .. } => {
                    expression(syntax, registry, &retained);
                }
            }
        }
        if let Some(ItemLanguageData::Python(data)) = &mut item.language_data {
            match &mut data.declaration {
                PythonDeclaration::TypeAlias { target } => expression(target, registry, &retained),
                PythonDeclaration::Class { bases, .. } => {
                    for base in bases {
                        expression(base, registry, &retained);
                    }
                }
                _ => {}
            }
        }
    }
}

fn expression(
    value: &mut SignatureExpression,
    registry: &IdentityRegistry,
    retained: &BTreeSet<String>,
) {
    match value {
        SignatureExpression::Name { name, target } => {
            // Removed and ambiguous declarations must not leave dangling links.
            *target = registry
                .resolve_alias(name)
                .ok()
                .flatten()
                .filter(|reference| retained.contains(&reference.item))
                .cloned();
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            expression(constructor, registry, retained);
            for value in arguments {
                expression(value, registry, retained);
            }
        }
        SignatureExpression::Sequence { items }
        | SignatureExpression::LanguageSpecific {
            children: items, ..
        } => {
            for value in items {
                expression(value, registry, retained);
            }
        }
        SignatureExpression::Literal { .. } => {}
    }
}
