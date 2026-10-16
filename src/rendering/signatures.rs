use crate::ir::{ParameterKind, Signature, SignatureExpression};

pub(super) fn signature_for_site(
    site: &crate::site::Site<'_>,
    name: &str,
    value: &Signature,
    ecosystem: &str,
) -> String {
    if !matches!(ecosystem, "python" | "rust") {
        return signature(name, value, ecosystem);
    }
    let names = if ecosystem == "python" {
        public_names(site)
    } else {
        PublicNames {
            aliases: std::collections::BTreeMap::new(),
            private_modules: Vec::new(),
        }
    };
    let mut value = value.clone();
    match &mut value {
        Signature::Callable {
            parameters,
            returns,
        } => {
            for parameter in parameters {
                if let Some(annotation) = &mut parameter.annotation {
                    qualify_for_site(annotation, &names, site, ecosystem);
                }
                if let Some(default) = &mut parameter.default {
                    qualify_for_site(default, &names, site, ecosystem);
                }
            }
            if let Some(returns) = returns {
                qualify_for_site(returns, &names, site, ecosystem);
            }
        }
        Signature::Value { annotation, value } => {
            if let Some(annotation) = annotation {
                qualify_for_site(annotation, &names, site, ecosystem);
            }
            if let Some(value) = value {
                qualify_for_site(value, &names, site, ecosystem);
            }
        }
        Signature::LanguageSpecific { syntax, .. } => {
            qualify_for_site(syntax, &names, site, ecosystem)
        }
    }
    signature(name, &value, ecosystem)
}

pub(super) fn expression_for_site(
    site: &crate::site::Site<'_>,
    value: &SignatureExpression,
    ecosystem: &str,
) -> String {
    let mut value = value.clone();
    if ecosystem == "python" {
        qualify(&mut value, &public_names(site));
    } else if ecosystem == "rust" {
        qualify_rust(&mut value, site);
    }
    expression(&value, ecosystem)
}

fn qualify_for_site(
    value: &mut SignatureExpression,
    names: &PublicNames,
    site: &crate::site::Site<'_>,
    ecosystem: &str,
) {
    if ecosystem == "rust" {
        qualify_rust(value, site);
    } else {
        qualify(value, names);
    }
}

fn qualify_rust(value: &mut SignatureExpression, site: &crate::site::Site<'_>) {
    match value {
        SignatureExpression::Name {
            name,
            target: Some(target),
        } => {
            if let Some(data) = site
                .workspace
                .packages
                .get(&target.package)
                .and_then(|package| package.items.get(&target.item))
                .and_then(super::rust::data)
            {
                *name = if name.contains("::") {
                    data.public_path.clone()
                } else {
                    data.public_path
                        .rsplit("::")
                        .next()
                        .unwrap_or(name)
                        .to_owned()
                };
            }
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            qualify_rust(constructor, site);
            for argument in arguments {
                qualify_rust(argument, site);
            }
        }
        SignatureExpression::Sequence { items }
        | SignatureExpression::LanguageSpecific {
            children: items, ..
        } => {
            for item in items {
                qualify_rust(item, site);
            }
        }
        SignatureExpression::Literal { .. } | SignatureExpression::Name { target: None, .. } => {}
    }
}

struct PublicNames {
    aliases: std::collections::BTreeMap<String, String>,
    private_modules: Vec<String>,
}

fn public_names(site: &crate::site::Site<'_>) -> PublicNames {
    let mut names = std::collections::BTreeMap::new();
    let mut private_modules = Vec::new();
    for page in site.pages.values() {
        if let (Some(item), Some(public)) = (page.item, page.public_name.as_ref()) {
            if item.kind == crate::ir::ItemKind::Module && !item.discoverable() {
                private_modules.push(item.qualified_name.clone());
            }
            names.insert(item.qualified_name.clone(), public.clone());
            for alias in &item.aliases {
                names.insert(alias.qualified_name.clone(), public.clone());
            }
        }
    }
    PublicNames {
        aliases: names,
        private_modules,
    }
}

fn qualify(value: &mut SignatureExpression, names: &PublicNames) {
    match value {
        SignatureExpression::Name { name, .. } => {
            if let Some(public) = names.aliases.get(name) {
                *name = public.clone();
            } else if let Some(local) = names
                .private_modules
                .iter()
                .find_map(|module| {
                    name.strip_prefix(module)
                        .and_then(|suffix| suffix.strip_prefix('.'))
                })
                .map(str::to_owned)
            {
                // Local type variables and aliases remain descriptive names;
                // displaying a private module would suggest a supported import.
                *name = local;
            }
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            qualify(constructor, names);
            for argument in arguments {
                qualify(argument, names);
            }
        }
        SignatureExpression::Sequence { items }
        | SignatureExpression::LanguageSpecific {
            children: items, ..
        } => {
            for item in items {
                qualify(item, names);
            }
        }
        SignatureExpression::Literal { .. } => {}
    }
}

pub(super) fn signature(name: &str, signature: &Signature, ecosystem: &str) -> String {
    match signature {
        Signature::Callable {
            parameters,
            returns,
        } => {
            if ecosystem == "julia" {
                let mut positional = Vec::new();
                let mut keywords = Vec::new();
                for parameter in parameters {
                    let mut part = parameter.name.clone();
                    if let Some(annotation) = &parameter.annotation {
                        part.push_str(&format!("::{}", expression(annotation, ecosystem)));
                    }
                    if matches!(
                        parameter.kind,
                        ParameterKind::VariadicPositional | ParameterKind::VariadicKeyword
                    ) {
                        part.push_str("...");
                    }
                    if let Some(default) = &parameter.default {
                        part.push_str(&format!(" = {}", expression(default, ecosystem)));
                    }
                    if matches!(
                        parameter.kind,
                        ParameterKind::KeywordOnly | ParameterKind::VariadicKeyword
                    ) {
                        keywords.push(part);
                    } else {
                        positional.push(part);
                    }
                }
                let mut args = positional.join(", ");
                if !keywords.is_empty() {
                    args.push_str(&format!("; {}", keywords.join(", ")));
                }
                let mut value = format!("{name}({args})");
                if let Some(returns) = returns {
                    value.push_str(&format!("::{}", expression(returns, ecosystem)));
                }
                return value;
            }
            let mut parts = Vec::new();
            let mut keyword_marker = false;
            for (index, parameter) in parameters.iter().enumerate() {
                if ecosystem == "python"
                    && parameter.kind == ParameterKind::KeywordOnly
                    && !keyword_marker
                {
                    parts.push("*".into());
                    keyword_marker = true;
                }
                let prefix = match parameter.kind {
                    ParameterKind::VariadicPositional if ecosystem == "python" => {
                        keyword_marker = true;
                        "*"
                    }
                    ParameterKind::VariadicKeyword if ecosystem == "python" => "**",
                    _ => "",
                };
                let mut part = format!("{prefix}{}", parameter.name);
                if let Some(annotation) = &parameter.annotation {
                    part.push_str(&format!(": {}", expression(annotation, ecosystem)));
                }
                if let Some(default) = &parameter.default {
                    part.push_str(&format!(" = {}", expression(default, ecosystem)));
                }
                parts.push(part);
                if ecosystem == "python"
                    && parameter.kind == ParameterKind::PositionalOnly
                    && parameters
                        .get(index + 1)
                        .is_none_or(|p| p.kind != ParameterKind::PositionalOnly)
                {
                    parts.push("/".into());
                }
            }
            let mut value = format!("{name}({})", parts.join(", "));
            if let Some(returns) = returns {
                value.push_str(&format!(" -> {}", expression(returns, ecosystem)));
            }
            value
        }
        Signature::Value { annotation, value } => {
            let mut result = name.to_owned();
            if let Some(annotation) = annotation {
                let separator = if ecosystem == "julia" { "::" } else { ": " };
                result.push_str(&format!("{separator}{}", expression(annotation, ecosystem)));
            }
            if let Some(value) = value {
                result.push_str(&format!(" = {}", expression(value, ecosystem)));
            }
            result
        }
        Signature::LanguageSpecific { syntax, .. } => {
            let value = expression(syntax, ecosystem);
            if ecosystem == "julia"
                && let Some((module, binding)) = name.rsplit_once('.')
                && value
                    .strip_prefix(binding)
                    .is_some_and(|suffix| suffix.starts_with(['(', '{']))
            {
                return format!("{module}.{value}");
            }
            value
        }
    }
}
pub(super) fn expression(value: &SignatureExpression, ecosystem: &str) -> String {
    match value {
        SignatureExpression::Name { name, .. } => name.clone(),
        SignatureExpression::Literal { text } => text.clone(),
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            let (open, close) = match ecosystem {
                "python" => ('[', ']'),
                "rust" => ('<', '>'),
                _ => ('(', ')'),
            };
            format!(
                "{}{open}{}{close}",
                expression(constructor, ecosystem),
                arguments
                    .iter()
                    .map(|a| expression(a, ecosystem))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        SignatureExpression::Sequence { items } => items
            .iter()
            .map(|a| expression(a, ecosystem))
            .collect::<Vec<_>>()
            .join(", "),
        SignatureExpression::LanguageSpecific {
            source,
            children,
            name,
            ..
        } => source.clone().unwrap_or_else(|| {
            let separator = match name.as_str() {
                "tokens" if ecosystem == "rust" => "",
                "union" => " | ",
                _ => " ",
            };
            children
                .iter()
                .map(|a| expression(a, ecosystem))
                .collect::<Vec<_>>()
                .join(separator)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tokens_preserve_receivers_lifetimes_and_where_clauses() {
        let syntax = SignatureExpression::LanguageSpecific {
            language: "rust".into(),
            name: "tokens".into(),
            source: None,
            children: vec![
                SignatureExpression::Literal {
                    text: "pub fn fit<'a, T>(&'a self, value: ".into(),
                },
                SignatureExpression::Apply {
                    constructor: Box::new(SignatureExpression::Name {
                        name: "Vec".into(),
                        target: None,
                    }),
                    arguments: vec![SignatureExpression::Name {
                        name: "T".into(),
                        target: None,
                    }],
                },
                SignatureExpression::Literal {
                    text: ") -> &'a Self\nwhere T: Clone + 'a".into(),
                },
            ],
        };
        assert_eq!(
            signature(
                "fit",
                &Signature::LanguageSpecific {
                    language: "rust".into(),
                    syntax
                },
                "rust"
            ),
            "pub fn fit<'a, T>(&'a self, value: Vec<T>) -> &'a Self\nwhere T: Clone + 'a"
        );
    }
}
