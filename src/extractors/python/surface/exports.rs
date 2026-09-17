use std::collections::BTreeMap;

use super::*;

pub(super) fn exports(module: &ParsedModule) -> PythonExports {
    if module.exports.is_empty() {
        return PythonExports::Implicit;
    }
    let mut operations: Vec<_> = module.exports.iter().collect();
    operations.sort_by_key(|op| op.source.span.map(|s| (s.start, s.end)));
    let mut declarations: Vec<_> = module.declarations.iter().collect();
    declarations.sort_by_key(|d| d.source.span.map(|s| (s.start, s.end)));
    let mut declarations = declarations.into_iter().peekable();
    let mut constants = BTreeMap::new();
    let mut names = None;
    let mut sources = vec![];
    let mut unsupported = String::new();
    for operation in operations {
        while let Some(declaration) = declarations.peek() {
            if declaration.source.span.map(|s| s.start) >= operation.source.span.map(|s| s.start) {
                break;
            }
            let declaration = declarations.next().unwrap();
            let value = match declaration.signature.as_ref().map(|s| &s.signature) {
                Some(Signature::Value {
                    value: Some(value), ..
                }) => strings(value, &constants),
                _ => None,
            };
            constants.insert(declaration.name.clone(), value);
        }
        let value = evaluate(&operation.value, &constants, names.as_ref());
        sources.push(evidence(&operation.source, SourceRole::Export));
        names = match (operation.kind, value) {
            (ExportOperationKind::Assign, value) => value,
            (ExportOperationKind::Extend, Some(value)) => names.map(|mut names: Vec<String>| {
                names.extend(value);
                names
            }),
            (ExportOperationKind::Append, Some(value)) if value.len() == 1 => {
                names.map(|mut names: Vec<String>| {
                    names.extend(value);
                    names
                })
            }
            _ => None,
        };
        if names.is_none() {
            unsupported = match &operation.value {
                ExportValue::Unsupported(raw) => raw.clone(),
                _ => "unsupported __all__ expression".into(),
            };
        }
    }
    match names {
        Some(names) => PythonExports::Explicit { names, sources },
        None => PythonExports::Dynamic {
            expression: SignatureExpression::LanguageSpecific {
                language: "python".into(),
                name: "dynamic-exports".into(),
                children: vec![],
                source: Some(unsupported),
            },
            sources,
        },
    }
}

fn evaluate(
    value: &ExportValue,
    constants: &BTreeMap<String, Option<Vec<String>>>,
    previous: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    match value {
        ExportValue::Names(names) => Some(names.clone()),
        ExportValue::Name(name) if name == "__all__" => previous.cloned(),
        ExportValue::Name(name) => constants.get(name)?.clone(),
        ExportValue::Concat(values) => values.iter().try_fold(vec![], |mut out, v| {
            out.extend(evaluate(v, constants, previous)?);
            Some(out)
        }),
        ExportValue::Unsupported(_) => None,
    }
}

fn strings(
    value: &SignatureExpression,
    constants: &BTreeMap<String, Option<Vec<String>>>,
) -> Option<Vec<String>> {
    match value {
        SignatureExpression::Name { name, .. } => constants.get(name)?.clone(),
        SignatureExpression::LanguageSpecific {
            language,
            name,
            children,
            ..
        } if language == "python" && matches!(name.as_str(), "list" | "tuple") => children
            .iter()
            .map(|v| match v {
                SignatureExpression::Literal { text } => string_literal(text),
                _ => None,
            })
            .collect(),
        SignatureExpression::LanguageSpecific {
            language,
            name,
            children,
            ..
        } if language == "python" && name == "add" => {
            children.iter().try_fold(vec![], |mut out, v| {
                out.extend(strings(v, constants)?);
                Some(out)
            })
        }
        _ => None,
    }
}

fn string_literal(text: &str) -> Option<String> {
    // Export names cannot contain escapes or quote characters, so this subset
    // suffices without treating arbitrary Python literal syntax as evaluated.
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains(['\\', '"'])).then(|| inner.to_owned())
}

pub(super) fn visibility(
    exports: &PythonExports,
    name: &str,
    explicit_import: bool,
    definition: bool,
) -> Option<PythonVisibility> {
    match exports {
        PythonExports::Explicit { names, .. } => names
            .iter()
            .any(|n| n == name)
            .then_some(PythonVisibility::Public),
        PythonExports::Implicit => (!name.starts_with('_') && (definition || explicit_import))
            .then_some(PythonVisibility::Public),
        PythonExports::Dynamic { .. } => {
            (!name.starts_with('_') && definition).then_some(PythonVisibility::Unknown)
        }
    }
}
