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
                return layout_source(&value, "julia");
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
            let suffix = returns.as_ref().map_or_else(String::new, |returns| {
                format!(" -> {}", expression(returns, ecosystem))
            });
            layout_call(name, &parts, &suffix)
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
            let value = if ecosystem == "rust" {
                layout_rust(syntax)
            } else {
                layout_source(&expression(syntax, ecosystem), ecosystem)
            };
            if ecosystem == "julia"
                && let Some((module, binding)) = name.rsplit_once('.')
                && value
                    .strip_prefix(binding)
                    .is_some_and(|suffix| suffix.starts_with(['(', '{']))
            {
                return layout_source(&format!("{module}.{value}"), ecosystem);
            }
            value
        }
    }
}
const SIGNATURE_WIDTH: usize = 80;

fn needs_layout(source: &str) -> bool {
    !source.contains('\n') && source.chars().count() > SIGNATURE_WIDTH
}

fn layout_call(name: &str, parameters: &[String], suffix: &str) -> String {
    let source = format!("{name}({}){suffix}", parameters.join(", "));
    if !needs_layout(&source) || parameters.is_empty() {
        return source;
    }
    format!("{name}(\n    {}\n){suffix}", parameters.join(",\n    "))
}

// R and Julia retain source expressions for some signatures. Their existing
// parsers identify argument boundaries without splitting strings or nested calls.
pub(super) fn layout_source(source: &str, language: &str) -> String {
    if !needs_layout(source) {
        return source.to_owned();
    }
    let ranges: Vec<std::ops::Range<usize>> = match language {
        "r" => {
            use arity_parser::ast::{AstNode, Expr, HasArgList};
            let parsed = arity_parser::parser::parse(source);
            if !parsed.diagnostics.is_empty() {
                return source.to_owned();
            }
            let Some(Expr::Call(call)) = parsed.cst.children_with_tokens().find_map(Expr::cast)
            else {
                return source.to_owned();
            };
            call.args()
                .map(|arg| arg.syntax().text_range().into())
                .collect()
        }
        "julia" => {
            use fatou_parser::ast::{AstNode, CallExpr, HasArgList};
            use fatou_parser::syntax::SyntaxKind as K;
            let parsed = fatou_parser::parser::parse(source);
            if !parsed.diagnostics.is_empty() {
                return source.to_owned();
            }
            let Some(call) = parsed.cst.descendants().find_map(CallExpr::cast) else {
                return source.to_owned();
            };
            let Some(list) = call.arg_list() else {
                return source.to_owned();
            };
            list.syntax()
                .children()
                .flat_map(|node| {
                    if node.kind() == K::PARAMETERS {
                        node.children().collect::<Vec<_>>()
                    } else {
                        vec![node]
                    }
                })
                .map(|node| node.text_range().into())
                .collect()
        }
        _ => return source.to_owned(),
    };
    layout_ranges(source, &ranges)
}

fn layout_ranges(source: &str, ranges: &[std::ops::Range<usize>]) -> String {
    let (Some(first), Some(last)) = (ranges.first(), ranges.last()) else {
        return source.to_owned();
    };
    let mut result = source[..first.start].trim_end().to_owned();
    let suffix = source[last.end..].trim_start();
    let (trailing_comma, suffix) = suffix
        .strip_prefix(',')
        .map_or((false, suffix), |rest| (true, rest.trim_start()));
    result.push('\n');
    for (index, range) in ranges.iter().enumerate() {
        result.push_str("    ");
        result.push_str(source[range.clone()].trim());
        if let Some(next) = ranges.get(index + 1) {
            result.push_str(source[range.end..next.start].trim());
        } else if trailing_comma {
            result.push(',');
        }
        result.push('\n');
    }
    result.push_str(suffix);
    result
}

fn layout_rust(syntax: &SignatureExpression) -> String {
    let source = expression(syntax, "rust");
    if !needs_layout(&source) {
        return source;
    }
    let SignatureExpression::LanguageSpecific { children, .. } = syntax else {
        return source;
    };
    let mut offset = 0;
    let mut function = false;
    for child in children {
        if matches!(child, SignatureExpression::Literal { text } if text == "fn ") {
            function = true;
        }
        if function
            && let SignatureExpression::LanguageSpecific {
                children: declaration,
                ..
            } = child
            && let Some(SignatureExpression::LanguageSpecific {
                children: parameters,
                ..
            }) = declaration.first()
            && matches!(parameters.first(), Some(SignatureExpression::Literal { text }) if text == "(")
            && matches!(parameters.last(), Some(SignatureExpression::Literal { text }) if text == ")")
        {
            let mut position = offset + 1;
            let mut ranges = Vec::new();
            for parameter in &parameters[1..parameters.len() - 1] {
                let length = expression(parameter, "rust").len();
                if !matches!(parameter, SignatureExpression::Literal { text } if text == ", ") {
                    ranges.push(position..position + length);
                }
                position += length;
            }
            return layout_ranges(&source, &ranges);
        }
        offset += expression(child, "rust").len();
    }
    source
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
    fn callable_layout_breaks_parameters_without_splitting_defaults() {
        let parameters = vec![
            "observations: Sequence[float]".to_owned(),
            "predictions: Sequence[float] = (1.0, 2.0, 3.0)".to_owned(),
            "*".to_owned(),
            "normalize: bool = True".to_owned(),
        ];
        assert_eq!(
            layout_call("mean_squared_error", &parameters, " -> float"),
            "mean_squared_error(\n    observations: Sequence[float],\n    predictions: Sequence[float] = (1.0, 2.0, 3.0),\n    *,\n    normalize: bool = True\n) -> float"
        );
        assert_eq!(layout_call("f", &["x".into()], ""), "f(x)");
        let boundary = "x".repeat(77);
        assert!(!layout_call("f", std::slice::from_ref(&boundary), "").contains('\n'));
        assert!(layout_call("f", &[format!("{boundary}x")], "").contains('\n'));
    }

    #[test]
    fn r_usage_layout_preserves_nested_defaults_and_strings() {
        let source = "mean_squared_error(observations = c(1, 2, 3), predictions = list(values = c(1, 2)), label = \"a, b\")";
        assert_eq!(
            layout_source(source, "r"),
            "mean_squared_error(\n    observations = c(1, 2, 3),\n    predictions = list(values = c(1, 2)),\n    label = \"a, b\"\n)"
        );
    }

    #[test]
    fn julia_layout_preserves_keywords_and_where_clauses() {
        let source = "mean_squared_error(observations::Vector{T}, predictions::Vector{T}; normalize = true, label = \"a, b\") where T";
        assert_eq!(
            layout_source(source, "julia"),
            "mean_squared_error(\n    observations::Vector{T},\n    predictions::Vector{T};\n    normalize = true,\n    label = \"a, b\"\n) where T"
        );
        let trailing_comma = source.replace(") where T", ",) where T");
        assert!(layout_source(&trailing_comma, "julia").ends_with("label = \"a, b\",\n) where T"));
        let keywords_only = "mean_squared_error(; observations = zeros(100), predictions = zeros(100), normalize = true)";
        assert!(layout_source(keywords_only, "julia").starts_with("mean_squared_error(;\n"));
    }

    #[test]
    fn existing_multiline_and_unparseable_signatures_are_preserved() {
        let multiline = "f(\n    observations,\n    predictions\n)";
        for language in ["r", "julia"] {
            assert_eq!(layout_source(multiline, language), multiline);
            let invalid = format!("f({}", "x".repeat(100));
            assert_eq!(layout_source(&invalid, language), invalid);
        }
    }

    #[test]
    fn rust_layout_uses_structured_parameter_boundaries() {
        let literal = |text: &str| SignatureExpression::Literal { text: text.into() };
        let tokens = |children| SignatureExpression::LanguageSpecific {
            language: "rust".into(),
            name: "tokens".into(),
            source: None,
            children,
        };
        let syntax = tokens(vec![
            literal("pub "),
            literal("fn "),
            literal("mean_squared_error"),
            literal("<'a, T>"),
            tokens(vec![
                tokens(vec![
                    literal("("),
                    literal("observations: &'a [T]"),
                    literal(", "),
                    literal("predictions: &'a [(T, T)]"),
                    literal(")"),
                ]),
                literal(" -> Result<T, Error>"),
            ]),
            literal(" where T: Clone"),
        ]);
        assert_eq!(
            layout_rust(&syntax),
            "pub fn mean_squared_error<'a, T>(\n    observations: &'a [T],\n    predictions: &'a [(T, T)]\n) -> Result<T, Error> where T: Clone"
        );
    }

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
