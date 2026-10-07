use std::collections::BTreeMap;

use fatou_parser::ast::{
    AssignmentExpr, AstNode, AstToken, CallExpr, CurlyExpr, HasArgList, TypeAnnotation,
};
use fatou_parser::syntax::{SyntaxKind as K, SyntaxNode};

use crate::ir::{Parameter, ParameterKind, Signature, SignatureExpression as E};

#[derive(Clone)]
pub(super) struct ParsedSignature {
    pub name: String,
    pub signature: Signature,
    pub dispatch: E,
}

pub(super) fn trivia(kind: K) -> bool {
    matches!(
        kind,
        K::WHITESPACE | K::NEWLINE | K::COMMENT | K::BLOCK_COMMENT
    )
}

pub(super) fn node_name(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        K::NAME => Some(node.text().to_string().trim().to_owned()),
        K::NONSTANDARD_IDENTIFIER => {
            let raw = node.text().to_string();
            fatou_parser::parser::string_value(raw.strip_prefix("var\"")?.strip_suffix('"')?).ok()
        }
        K::PAREN_EXPR | K::CURLY_EXPR => node.children().next().and_then(|n| node_name(&n)),
        K::BINARY_EXPR if node.children_with_tokens().any(|t| t.kind() == K::DOT) => {
            let mut children = node.children();
            Some(format!(
                "{}.{}",
                node_name(&children.next()?)?,
                node_name(&children.next()?)?
            ))
        }
        _ => None,
    }
}

pub(super) fn lower(node: &SyntaxNode) -> E {
    if matches!(node.kind(), K::NAME | K::BINARY_EXPR)
        && let Some(name) = node_name(node)
    {
        return E::Name { name, target: None };
    }
    if matches!(node.kind(), K::ARG | K::SIGNATURE | K::PAREN_EXPR) && node.children().count() == 1
    {
        return lower(&node.children().next().unwrap());
    }
    if node.kind() == K::LITERAL {
        return E::Literal {
            text: node.text().to_string().trim().to_owned(),
        };
    }
    let children = node
        .children_with_tokens()
        .filter(|element| !trivia(element.kind()))
        .map(|element| match element {
            rowan::NodeOrToken::Node(child) => lower(&child),
            rowan::NodeOrToken::Token(token) => E::Literal {
                text: token.text().to_string(),
            },
        })
        .collect();
    E::LanguageSpecific {
        language: "julia".into(),
        name: format!("{:?}", node.kind()).to_ascii_lowercase(),
        children,
        source: Some(node.text().to_string().trim().to_owned()),
    }
}

pub(super) fn parse(start: &SyntaxNode) -> Option<ParsedSignature> {
    let mut cursor = start.clone();
    let mut wheres = Vec::new();
    let mut returns = None;
    loop {
        match cursor.kind() {
            K::WHERE_EXPR => {
                let mut children = cursor.children();
                let core = children.next()?;
                let mut specs = Vec::new();
                for child in children {
                    if child.kind() == K::BRACES {
                        if let Some(list) = child.children().find(|n| n.kind() == K::ARG_LIST) {
                            specs.extend(list.children().map(|n| unwrap_arg(&n)));
                        } else {
                            specs.extend(child.children().map(|n| unwrap_arg(&n)));
                        }
                    } else {
                        specs.push(child);
                    }
                }
                // Inner clauses bind before outer clauses.
                specs.extend(wheres);
                wheres = specs;
                cursor = core;
            }
            K::TYPE_ANNOTATION => {
                let annotation = TypeAnnotation::cast(cursor.clone())?;
                let mut return_node = annotation.ty()?.syntax().clone();
                while return_node.kind() == K::WHERE_EXPR {
                    let mut children = return_node.children();
                    let value = children.next()?;
                    for child in children {
                        if child.kind() == K::BRACES {
                            wheres.extend(child.children().map(|n| unwrap_arg(&n)));
                        } else {
                            wheres.push(child);
                        }
                    }
                    return_node = value;
                }
                returns = Some(lower(&return_node));
                cursor = annotation.pattern()?.syntax().clone();
            }
            _ => break,
        }
    }
    let call = CallExpr::cast(cursor.clone())?;
    let callee = call.callee().map(|e| e.syntax().clone());
    let name = if let Some(node) = &callee {
        node_name(node)?
    } else {
        call.callee_operator()?.syntax().text().to_string()
    };
    let type_args: Vec<_> = callee
        .as_ref()
        .and_then(|n| CurlyExpr::cast(n.clone()))
        .and_then(|n| n.arg_list())
        .map(|list| list.syntax().children().map(|n| lower(&n)).collect())
        .unwrap_or_default();
    let mut parameters = Vec::new();
    for child in call.arg_list()?.syntax().children() {
        if child.kind() == K::PARAMETERS {
            for keyword in child.children() {
                parameters.push(parameter(&keyword, true)?);
            }
        } else {
            parameters.push(parameter(&child, false)?);
        }
    }
    let mut substitutions = BTreeMap::new();
    for (index, variable) in wheres.iter().enumerate() {
        let name = variable
            .descendants()
            .find(|n| n.kind() == K::NAME)
            .and_then(|n| node_name(&n))?;
        if substitutions.insert(name, index.to_string()).is_some() {
            return None;
        }
    }
    let dispatch_parameters = parameters
        .iter()
        .filter(|p| {
            !matches!(
                p.kind,
                ParameterKind::KeywordOnly | ParameterKind::VariadicKeyword
            )
        })
        .map(|parameter| {
            let mut annotation = parameter.annotation.clone().unwrap_or(E::Name {
                name: "Any".into(),
                target: None,
            });
            substitute(&mut annotation, &substitutions);
            if parameter.kind == ParameterKind::VariadicPositional {
                node("vararg", vec![annotation])
            } else {
                annotation
            }
        })
        .collect();
    let dispatch = node(
        "dispatch",
        vec![
            node(
                "constructor-head",
                type_args
                    .iter()
                    .cloned()
                    .map(|mut e| {
                        substitute(&mut e, &substitutions);
                        e
                    })
                    .collect(),
            ),
            node("positional", dispatch_parameters),
            node(
                "where",
                wheres
                    .iter()
                    .map(|n| {
                        let mut e = lower(n);
                        substitute(&mut e, &substitutions);
                        e
                    })
                    .collect(),
            ),
        ],
    );
    let callable = Signature::Callable {
        parameters,
        returns,
    };
    let signature = if wheres.is_empty() && type_args.is_empty() {
        callable
    } else {
        // Keep the full header structured so consumers never have to parse a
        // display string to recover Julia's constraints or constructor head.
        Signature::LanguageSpecific {
            language: "julia".into(),
            syntax: lower(start),
        }
    };
    Some(ParsedSignature {
        name,
        signature,
        dispatch,
    })
}

fn unwrap_arg(node: &SyntaxNode) -> SyntaxNode {
    if node.kind() == K::ARG {
        node.children().next().unwrap_or(node.clone())
    } else {
        node.clone()
    }
}

fn parameter(node: &SyntaxNode, keyword: bool) -> Option<Parameter> {
    let mut cursor = unwrap_arg(node);
    let mut default = None;
    if matches!(cursor.kind(), K::KEYWORD_ARG | K::ASSIGNMENT_EXPR) {
        let mut children = cursor.children();
        cursor = children.next()?;
        default = Some(lower(&children.next()?));
    }
    let mut variadic = false;
    let mut annotation = None;
    loop {
        match cursor.kind() {
            K::SPLAT_EXPR => {
                variadic = true;
                cursor = cursor.children().next()?;
            }
            K::TYPE_ANNOTATION => {
                let value = TypeAnnotation::cast(cursor.clone())?;
                annotation = Some(lower(value.ty()?.syntax()));
                if let Some(pattern) = value.pattern() {
                    cursor = pattern.syntax().clone();
                } else {
                    return Some(Parameter {
                        name: String::new(),
                        kind: if variadic {
                            ParameterKind::VariadicPositional
                        } else {
                            ParameterKind::PositionalOnly
                        },
                        annotation,
                        default,
                    });
                }
            }
            _ => break,
        }
    }
    Some(Parameter {
        name: node_name(&cursor)?,
        kind: match (keyword, variadic) {
            (false, false) => ParameterKind::PositionalOnly,
            (false, true) => ParameterKind::VariadicPositional,
            (true, false) => ParameterKind::KeywordOnly,
            (true, true) => ParameterKind::VariadicKeyword,
        },
        annotation,
        default,
    })
}

fn node(name: &str, children: Vec<E>) -> E {
    E::LanguageSpecific {
        language: "julia".into(),
        name: name.into(),
        children,
        source: None,
    }
}

fn substitute(expression: &mut E, substitutions: &BTreeMap<String, String>) {
    match expression {
        E::Name { name, target } => {
            if let Some(value) = substitutions.get(name) {
                *expression = node(
                    "bound-type-variable",
                    vec![E::Literal {
                        text: value.clone(),
                    }],
                );
                return;
            }
            *target = None;
        }
        E::Apply {
            constructor,
            arguments,
        } => {
            substitute(constructor, substitutions);
            for e in arguments {
                substitute(e, substitutions);
            }
        }
        E::Sequence { items }
        | E::LanguageSpecific {
            children: items, ..
        } => {
            for e in items {
                substitute(e, substitutions);
            }
        }
        E::Literal { .. } => {}
    }
    if let E::LanguageSpecific { source, .. } = expression {
        *source = None;
    }
}

pub(crate) fn selector(text: &str) -> Option<(String, E)> {
    let parsed = fatou_parser::parser::parse(text);
    if !parsed.diagnostics.is_empty() || parsed.cst.children().count() != 1 {
        return None;
    }
    let header = parsed.cst.children().next()?;
    let signature = parse(&header)?;
    Some((signature.name, signature.dispatch))
}

pub(super) fn assignment(node: &SyntaxNode) -> Option<(SyntaxNode, SyntaxNode)> {
    let assignment = AssignmentExpr::cast(node.clone())?;
    use fatou_parser::ast::AstToken;
    (assignment.op()?.syntax().text() == "=")
        .then(|| (assignment.lhs(), assignment.rhs()))
        .and_then(|(lhs, rhs)| Some((lhs?.syntax().clone(), rhs?.syntax().clone())))
}
