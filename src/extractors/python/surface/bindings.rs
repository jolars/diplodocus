use super::*;

#[derive(Clone)]
pub(super) struct Binding {
    pub target: String,
    pub kind: ItemAliasKind,
    pub source: SourceLocation,
    pub explicit: bool,
}

pub(super) fn imports(module: &ParsedModule) -> BTreeMap<String, Vec<Binding>> {
    let mut bindings: BTreeMap<String, Vec<Binding>> = BTreeMap::new();
    for import in &module.imports {
        let base = import_base(module, import);
        for name in &import.names {
            let ordinary = import.level == 0 && import.module.is_none();
            let local = name.alias.clone().unwrap_or_else(|| {
                if ordinary {
                    name.name.split('.').next().unwrap_or("").into()
                } else {
                    name.name.clone()
                }
            });
            let target = if ordinary {
                if name.alias.is_some() {
                    name.name.clone()
                } else {
                    local.clone()
                }
            } else if let Some(base) = &base {
                if base.is_empty() {
                    name.name.clone()
                } else {
                    format!("{base}.{}", name.name)
                }
            } else {
                // An empty target cannot resolve inside the supplied package.
                String::new()
            };
            bindings.entry(local).or_default().push(Binding {
                target,
                kind: ItemAliasKind::PythonReexport,
                source: import.source.clone(),
                explicit: name.alias.is_some(),
            });
        }
    }
    bindings
}

fn import_base(module: &ParsedModule, import: &ParsedImport) -> Option<String> {
    if import.level == 0 {
        return Some(import.module.clone().unwrap_or_default());
    }
    let mut parts: Vec<_> = module.name.split('.').collect();
    if !module.is_package {
        parts.pop();
    }
    for _ in 1..import.level {
        parts.pop()?;
    }
    if parts.is_empty() {
        return None;
    }
    let mut base = parts.join(".");
    if let Some(suffix) = &import.module
        && !suffix.is_empty()
    {
        base.push('.');
        base.push_str(suffix);
    }
    Some(base)
}

pub(super) fn resolve(
    name: &str,
    direct: &BTreeMap<String, BTreeSet<String>>,
    bindings: &BTreeMap<String, Vec<Binding>>,
    visited: &mut BTreeSet<String>,
) -> BTreeSet<String> {
    if !visited.insert(name.into()) {
        return BTreeSet::new();
    }
    let mut result = direct.get(name).cloned().unwrap_or_default();
    if let Some(edges) = bindings.get(name) {
        for edge in edges {
            result.extend(resolve(&edge.target, direct, bindings, visited));
        }
    }
    // A module or class alias also qualifies its members. Guard the rewritten
    // prefix separately so cycles cannot grow an unbounded dotted spelling.
    for (index, _) in name.match_indices('.').rev() {
        let prefix = &name[..index];
        let suffix = &name[index + 1..];
        if let Some(edges) = bindings.get(prefix)
            && visited.insert(prefix.into())
        {
            for edge in edges {
                result.extend(resolve(
                    &format!("{}.{suffix}", edge.target),
                    direct,
                    bindings,
                    visited,
                ));
            }
            visited.remove(prefix);
        }
    }
    visited.remove(name);
    result
}

pub(super) fn decorator_name(
    expression: &SignatureExpression,
    imports: &BTreeMap<String, Vec<Binding>>,
) -> Option<String> {
    let name = match expression {
        SignatureExpression::Name { name, .. } => name,
        SignatureExpression::LanguageSpecific { name, children, .. } if name == "call" => {
            return decorator_name(children.first()?, imports);
        }
        _ => return None,
    };
    let (head, suffix) = name.split_once('.').unwrap_or((name, ""));
    if let Some(bindings) = imports.get(head) {
        let targets: BTreeSet<_> = bindings.iter().map(|b| b.target.as_str()).collect();
        if targets.len() != 1 {
            return None;
        }
        let target = targets.first()?;
        return Some(if suffix.is_empty() {
            (*target).into()
        } else {
            format!("{target}.{suffix}")
        });
    }
    Some(name.clone())
}
