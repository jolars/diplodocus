use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use roxmltree::Node;

use crate::ir::*;

use super::{child, text};

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)(?:u8|u|U|L)?"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[\p{L}_][\p{L}\p{N}_]*|(?:0[xX][0-9a-fA-F']+(?:\.[0-9a-fA-F']*)?(?:[pP][+-]?[0-9']+)?|[0-9][0-9']*(?:\.[0-9']*)?(?:[eE][+-]?[0-9']+)?|\.[0-9]+(?:[eE][+-]?[0-9']+)?)[uUlLfF]*|::|&&|\.\.\.|->|==|!=|<=|>=|[^\s]"#).unwrap()
});

pub(super) fn node(name: &str, children: Vec<SignatureExpression>) -> SignatureExpression {
    SignatureExpression::LanguageSpecific {
        language: "cpp".into(),
        name: name.into(),
        children,
        source: None,
    }
}

pub(super) fn tokens(source: &str) -> SignatureExpression {
    node(
        "tokens",
        TOKENS
            .find_iter(source)
            .map(|token| {
                let token = token.as_str();
                if token.starts_with(|c: char| c.is_alphabetic() || c == '_') {
                    SignatureExpression::Name {
                        name: token.into(),
                        target: None,
                    }
                } else {
                    SignatureExpression::Literal { text: token.into() }
                }
            })
            .collect(),
    )
}

fn syntax(xml: Node<'_, '_>, references: &BTreeMap<String, ItemReference>) -> SignatureExpression {
    let mut parts = Vec::new();
    for part in xml.children() {
        if part.is_text() {
            if let SignatureExpression::LanguageSpecific { children, .. } =
                tokens(part.text().unwrap_or(""))
            {
                parts.extend(children);
            }
        } else if part.has_tag_name("ref") {
            parts.push(SignatureExpression::Name {
                name: text(part),
                target: part
                    .attribute("refid")
                    .and_then(|id| references.get(id))
                    .cloned(),
            });
        }
    }
    node("tokens", parts)
}

fn part(
    xml: Node<'_, '_>,
    tag: &str,
    references: &BTreeMap<String, ItemReference>,
) -> SignatureExpression {
    child(xml, tag).map_or_else(|| tokens(""), |xml| syntax(xml, references))
}

fn templates(
    xml: Node<'_, '_>,
    references: &BTreeMap<String, ItemReference>,
) -> SignatureExpression {
    node(
        "template",
        child(xml, "templateparamlist")
            .into_iter()
            .flat_map(|list| list.children().filter(|n| n.has_tag_name("param")))
            .map(|param| {
                node(
                    "parameter",
                    vec![
                        part(param, "type", references),
                        tokens(&text_child(param, "declname")),
                        part(param, "defval", references),
                    ],
                )
            })
            .collect(),
    )
}

pub(super) fn text_child(xml: Node<'_, '_>, tag: &str) -> String {
    child(xml, tag).map(text).unwrap_or_default()
}

fn suffix(xml: Node<'_, '_>) -> String {
    let args = text_child(xml, "argsstring");
    let mut depth = 0;
    // Reuse literal tokenization so parentheses in defaults cannot end the list.
    for token in TOKENS.find_iter(&args) {
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return args[token.end()..].trim().to_owned();
                }
            }
            _ => {}
        }
    }
    String::new()
}

pub(super) fn signature(
    xml: Node<'_, '_>,
    declaration: CppDeclarationKind,
    references: &BTreeMap<String, ItemReference>,
) -> Signature {
    use CppDeclarationKind as K;
    let name = text_child(xml, "name");
    let name = if name.is_empty() {
        text_child(xml, "compoundname")
    } else {
        name
    };
    let name = SignatureExpression::Name { name, target: None };
    let template = templates(xml, references);
    let syntax = match declaration {
        K::Function | K::Method | K::Constructor | K::Destructor | K::Operator => {
            let parameters = xml
                .children()
                .filter(|n| n.has_tag_name("param"))
                .map(|param| {
                    node(
                        "parameter",
                        vec![
                            part(param, "type", references),
                            tokens(&text_child(param, "declname")),
                            part(param, "defval", references),
                            part(param, "array", references),
                        ],
                    )
                })
                .collect();
            let mut qualifiers = ["static", "constexpr", "consteval", "explicit", "inline"]
                .into_iter()
                .filter(|key| xml.attribute(*key) == Some("yes"))
                .collect::<Vec<_>>()
                .join(" ");
            if xml
                .attribute("virt")
                .is_some_and(|value| value != "non-virtual")
            {
                qualifiers = format!("virtual {qualifiers}");
            }
            node(
                "function",
                vec![
                    template,
                    tokens(&qualifiers),
                    part(xml, "type", references),
                    name,
                    node("parameters", parameters),
                    tokens(&suffix(xml)),
                ],
            )
        }
        K::Class | K::Struct | K::Union | K::Namespace => {
            let keyword = match declaration {
                K::Class => "class",
                K::Struct => "struct",
                K::Union => "union",
                _ => "namespace",
            };
            let bases = xml
                .children()
                .filter(|n| n.has_tag_name("basecompoundref"))
                .map(|base| {
                    node(
                        "base",
                        vec![
                            tokens(base.attribute("prot").unwrap_or("public")),
                            syntax(base, references),
                        ],
                    )
                })
                .collect();
            node(
                "type",
                vec![template, tokens(keyword), name, node("bases", bases)],
            )
        }
        K::Enum => node(
            "enum",
            vec![
                tokens(if xml.attribute("strong") == Some("yes") {
                    "enum class"
                } else {
                    "enum"
                }),
                name,
                part(xml, "type", references),
            ],
        ),
        K::TypeAlias => node("alias", vec![name, part(xml, "type", references)]),
        K::Macro => node(
            "macro",
            vec![
                name,
                node(
                    "macro-parameters",
                    xml.children()
                        .filter(|n| n.has_tag_name("param"))
                        .map(|p| tokens(&text_child(p, "defname")))
                        .collect(),
                ),
                part(xml, "initializer", references),
            ],
        ),
        _ => node(
            "value",
            vec![
                tokens(
                    &["static", "constexpr", "constinit", "mutable"]
                        .into_iter()
                        .filter(|key| xml.attribute(*key) == Some("yes"))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                part(xml, "type", references),
                name,
                part(xml, "initializer", references),
            ],
        ),
    };
    Signature::LanguageSpecific {
        language: "cpp".into(),
        syntax,
    }
}

pub(super) fn dispatch(xml: Node<'_, '_>) -> SignatureExpression {
    let mut bindings = BTreeMap::new();
    let template = child(xml, "templateparamlist")
        .into_iter()
        .flat_map(|list| list.children().filter(|n| n.has_tag_name("param")))
        .enumerate()
        .map(|(index, param)| {
            let name = text_child(param, "declname");
            if !name.is_empty() {
                bindings.insert(name, format!("T{index}"));
            }
            tokens(&text_child(param, "type"))
        })
        .collect();
    let mut parameters = xml
        .children()
        .filter(|n| n.has_tag_name("param"))
        .map(|param| {
            node(
                "parameter-type",
                vec![
                    tokens(&text_child(param, "type")),
                    tokens(&text_child(param, "array")),
                ],
            )
        })
        .collect::<Vec<_>>();
    for parameter in &mut parameters {
        normalize_bindings(parameter, &bindings);
    }
    let suffix = suffix(xml);
    let qualifiers = suffix
        .split("noexcept")
        .next()
        .unwrap_or("")
        .split('=')
        .next()
        .unwrap_or("");
    node(
        "dispatch",
        vec![
            node("template-kinds", template),
            node("parameters", parameters),
            tokens(qualifiers),
        ],
    )
}

fn normalize_bindings(value: &mut SignatureExpression, bindings: &BTreeMap<String, String>) {
    match value {
        SignatureExpression::Name { name, .. } => {
            if let Some(binding) = bindings.get(name) {
                *name = binding.clone();
            }
        }
        SignatureExpression::LanguageSpecific { children, .. } => {
            for child in children {
                normalize_bindings(child, bindings);
            }
        }
        _ => {}
    }
}

/// Format C++ declaration nodes without reparsing a display signature.
pub(crate) fn format(value: &SignatureExpression) -> String {
    match value {
        SignatureExpression::Name { name, .. } => name.clone(),
        SignatureExpression::Literal { text } => text.clone(),
        SignatureExpression::LanguageSpecific { name, children, .. } => {
            let parts = children.iter().map(format).collect::<Vec<_>>();
            let p = |index| parts.get(index).map_or("", String::as_str);
            let join = || {
                parts
                    .iter()
                    .filter(|p| !p.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            match name.as_str() {
                "tokens" => {
                    let mut result = String::new();
                    let mut previous = "";
                    for token in &parts {
                        let tight = result.is_empty()
                            || matches!(
                                token.as_str(),
                                "::" | "," | ";" | ")" | "]" | ">" | "(" | "[" | "<"
                            )
                            || matches!(previous, "::" | "(" | "[" | "<" | "~");
                        if !tight {
                            result.push(' ');
                        }
                        result.push_str(token);
                        previous = token;
                    }
                    result
                }
                "template" if parts.is_empty() => String::new(),
                "template" => format!("template<{}>", parts.join(", ")),
                "parameter" => format!(
                    "{}{}{}",
                    [p(0), p(1)]
                        .into_iter()
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                    p(3),
                    if p(2).is_empty() {
                        String::new()
                    } else {
                        format!(" = {}", p(2))
                    }
                ),
                "parameters" => format!("({})", parts.join(", ")),
                "function" => format!(
                    "{}{}{}{}",
                    parts
                        .iter()
                        .take(3)
                        .filter(|p| !p.is_empty())
                        .map(|p| format!("{p} "))
                        .collect::<String>(),
                    p(3),
                    p(4),
                    if p(5).is_empty() {
                        String::new()
                    } else {
                        format!(" {}", p(5))
                    }
                ),
                "bases" if parts.is_empty() => String::new(),
                "bases" => format!(": {}", parts.join(", ")),
                "enum" => format!(
                    "{} {}{}",
                    p(0),
                    p(1),
                    if p(2).is_empty() {
                        String::new()
                    } else {
                        format!(" : {}", p(2))
                    }
                ),
                "alias" => format!("using {} = {}", p(0), p(1)),
                "value" => format!(
                    "{}{}{}",
                    parts
                        .iter()
                        .take(2)
                        .filter(|part| !part.is_empty())
                        .map(|part| format!("{part} "))
                        .collect::<String>(),
                    p(2),
                    if p(3).is_empty() {
                        String::new()
                    } else if p(3).starts_with('=') {
                        format!(" {}", p(3))
                    } else {
                        format!(" = {}", p(3))
                    }
                ),
                "macro-parameters" if parts.is_empty() => String::new(),
                "macro-parameters" => format!("({})", parts.join(", ")),
                "macro" => format!("#define {}{} {}", p(0), p(1), p(2))
                    .trim_end()
                    .to_owned(),
                _ => join(),
            }
        }
        SignatureExpression::Sequence { items } => {
            items.iter().map(format).collect::<Vec<_>>().join(", ")
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => format!(
            "{}<{}>",
            format(constructor),
            arguments.iter().map(format).collect::<Vec<_>>().join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpp_signatures_preserve_scientific_literals_arrays_and_storage_qualifiers() {
        assert_eq!(format(&tokens("1e-8")), "1e-8");
        assert_eq!(format(&tokens("0x1.fp+3")), "0x1.fp+3");
        let xml = roxmltree::Document::parse(r#"<memberdef static="yes" constexpr="yes"><type>int</type><name>limit</name><initializer>=256</initializer></memberdef>"#).unwrap();
        let Signature::LanguageSpecific { syntax, .. } = signature(
            xml.root_element(),
            CppDeclarationKind::Field,
            &BTreeMap::new(),
        ) else {
            panic!("C++ signature");
        };
        assert_eq!(format(&syntax), "static constexpr int limit = 256");
        let xml = roxmltree::Document::parse(r#"<memberdef><type>void</type><name>read</name><argsstring>(double values[3])</argsstring><param><type>double</type><declname>values</declname><array>[3]</array></param></memberdef>"#).unwrap();
        let Signature::LanguageSpecific { syntax, .. } = signature(
            xml.root_element(),
            CppDeclarationKind::Function,
            &BTreeMap::new(),
        ) else {
            panic!("C++ signature");
        };
        assert_eq!(format(&syntax), "void read(double values[3])");
        let xml = roxmltree::Document::parse(r#"<memberdef><type>void</type><name>read</name><argsstring>(int count=1'000) const</argsstring><param><type>int</type><declname>count</declname><defval>1'000</defval></param></memberdef>"#).unwrap();
        let Signature::LanguageSpecific { syntax, .. } = signature(
            xml.root_element(),
            CppDeclarationKind::Method,
            &BTreeMap::new(),
        ) else {
            panic!("C++ signature");
        };
        assert_eq!(format(&syntax), "void read(int count = 1'000) const");
        assert!(format(&node("function", vec![])).is_empty());
    }
}
