use crate::ir::{ParameterKind, Signature, SignatureExpression};

pub(super) fn signature(name: &str, signature: &Signature, ecosystem: &str) -> String {
    match signature {
        Signature::Callable {
            parameters,
            returns,
        } => {
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
                result.push_str(&format!(": {}", expression(annotation, ecosystem)));
            }
            if let Some(value) = value {
                result.push_str(&format!(" = {}", expression(value, ecosystem)));
            }
            result
        }
        Signature::LanguageSpecific { syntax, .. } => expression(syntax, ecosystem),
    }
}
fn expression(value: &SignatureExpression, ecosystem: &str) -> String {
    match value {
        SignatureExpression::Name { name, .. } => name.clone(),
        SignatureExpression::Literal { text } => text.clone(),
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            let (open, close) = if ecosystem == "python" {
                ('[', ']')
            } else {
                ('(', ')')
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
            let separator = if name == "union" { " | " } else { " " };
            children
                .iter()
                .map(|a| expression(a, ecosystem))
                .collect::<Vec<_>>()
                .join(separator)
        }),
    }
}
