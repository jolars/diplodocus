use std::collections::BTreeMap;

use ruff_python_ast::{Expr, Number, Operator, Parameter as AstParameter, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{Parameter, ParameterKind, Signature, SignatureExpression, SourceLocation};

use super::{diagnostic, located};

pub(super) struct Expressions<'a> {
    pub names: BTreeMap<String, String>,
    pub locals: BTreeMap<String, String>,
    pub qualify_locals: bool,
    pub text: &'a str,
    pub source: &'a SourceLocation,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl Expressions<'_> {
    pub fn callable(&mut self, function: &StmtFunctionDef) -> Signature {
        let arguments = &function.parameters;
        let mut parameters = Vec::new();
        for value in &arguments.posonlyargs {
            parameters.push(self.parameter(
                &value.parameter,
                ParameterKind::PositionalOnly,
                value.default.as_deref(),
            ));
        }
        for value in &arguments.args {
            parameters.push(self.parameter(
                &value.parameter,
                ParameterKind::PositionalOrKeyword,
                value.default.as_deref(),
            ));
        }
        if let Some(value) = &arguments.vararg {
            parameters.push(self.parameter(value, ParameterKind::VariadicPositional, None));
        }
        for value in &arguments.kwonlyargs {
            parameters.push(self.parameter(
                &value.parameter,
                ParameterKind::KeywordOnly,
                value.default.as_deref(),
            ));
        }
        if let Some(value) = &arguments.kwarg {
            parameters.push(self.parameter(value, ParameterKind::VariadicKeyword, None));
        }
        Signature::Callable {
            parameters,
            returns: function
                .returns
                .as_deref()
                .map(|value| self.expression(value)),
        }
    }

    fn parameter(
        &mut self,
        value: &AstParameter,
        kind: ParameterKind,
        default: Option<&Expr>,
    ) -> Parameter {
        Parameter {
            name: value.name.to_string(),
            kind,
            annotation: value
                .annotation
                .as_deref()
                .map(|value| self.expression(value)),
            default: default.map(|value| self.expression(value)),
        }
    }

    pub fn expression(&mut self, value: &Expr) -> SignatureExpression {
        use SignatureExpression as E;
        if let Some(mut name) = dotted_name(value) {
            let (root, suffix) = name
                .split_once('.')
                .map_or((name.as_str(), ""), |(root, _)| (root, &name[root.len()..]));
            if let Some(canonical) = self
                .names
                .get(root)
                .or_else(|| self.qualify_locals.then(|| self.locals.get(root)).flatten())
            {
                name = format!("{canonical}{suffix}");
            }
            return E::Name { name, target: None };
        }
        let raw = self.raw(value);
        match value {
            Expr::StringLiteral(value) => E::Literal {
                text: string_literal(value.value.to_str()),
            },
            Expr::NumberLiteral(value) => E::Literal {
                text: match &value.value {
                    Number::Int(value) => value.to_string(),
                    Number::Float(value) => format!("{value:?}"),
                    Number::Complex { real, imag } => format!("({real:?}+{imag:?}j)"),
                },
            },
            Expr::BooleanLiteral(value) => E::Literal {
                text: if value.value { "True" } else { "False" }.into(),
            },
            Expr::NoneLiteral(_) => E::Literal {
                text: "None".into(),
            },
            Expr::EllipsisLiteral(_) => E::Literal { text: "...".into() },
            Expr::Subscript(value) => E::Apply {
                constructor: Box::new(self.expression(&value.value)),
                arguments: match &*value.slice {
                    Expr::Tuple(tuple) => tuple
                        .elts
                        .iter()
                        .map(|value| self.expression(value))
                        .collect(),
                    slice => vec![self.expression(slice)],
                },
            },
            Expr::List(value) => node(
                "list",
                value
                    .elts
                    .iter()
                    .map(|value| self.expression(value))
                    .collect(),
                raw,
            ),
            Expr::Tuple(value) => node(
                "tuple",
                value
                    .elts
                    .iter()
                    .map(|value| self.expression(value))
                    .collect(),
                raw,
            ),
            Expr::Set(value) => node(
                "set",
                value
                    .elts
                    .iter()
                    .map(|value| self.expression(value))
                    .collect(),
                raw,
            ),
            Expr::Dict(value) => {
                let children = value
                    .items
                    .iter()
                    .map(|item| {
                        let mut children = Vec::new();
                        if let Some(key) = &item.key {
                            children.push(self.expression(key));
                        }
                        children.push(self.expression(&item.value));
                        node(
                            if item.key.is_some() {
                                "entry"
                            } else {
                                "unpack"
                            },
                            children,
                            None,
                        )
                    })
                    .collect();
                node("dict", children, raw)
            }
            Expr::BinOp(value) => node(
                operator(value.op),
                vec![self.expression(&value.left), self.expression(&value.right)],
                raw,
            ),
            Expr::UnaryOp(value) => node(
                &format!("unary-{}", value.op.as_str()),
                vec![self.expression(&value.operand)],
                raw,
            ),
            Expr::Attribute(value) => node(
                &format!("attribute:{}", value.attr),
                vec![self.expression(&value.value)],
                raw,
            ),
            Expr::Call(value) => {
                let mut children = vec![self.expression(&value.func)];
                children.extend(
                    value
                        .arguments
                        .args
                        .iter()
                        .map(|value| self.expression(value)),
                );
                for keyword in &value.arguments.keywords {
                    children.push(node(
                        &keyword.arg.as_ref().map_or_else(
                            || "unpack-keywords".into(),
                            |name| format!("keyword:{name}"),
                        ),
                        vec![self.expression(&keyword.value)],
                        None,
                    ));
                }
                node("call", children, raw)
            }
            Expr::Starred(value) => node("unpack", vec![self.expression(&value.value)], raw),
            Expr::Slice(value) => node(
                "slice",
                [&value.lower, &value.upper, &value.step]
                    .into_iter()
                    .map(|value| {
                        value.as_deref().map_or_else(
                            || node("omitted", vec![], None),
                            |value| self.expression(value),
                        )
                    })
                    .collect(),
                raw,
            ),
            _ => {
                self.diagnostics.push(diagnostic(DiagnosticCode::PythonUnsupportedSyntax, Severity::Error,
                    "This signature expression is outside the supported static subset; its source is retained.",
                    &located(self.source, value.range())));
                node("unsupported", vec![], raw)
            }
        }
    }

    fn raw(&self, value: &Expr) -> Option<String> {
        Some(self.text[usize::from(value.start())..usize::from(value.end())].into())
    }
}

pub(super) fn dotted_name(value: &Expr) -> Option<String> {
    match value {
        Expr::Name(value) => Some(value.id.to_string()),
        Expr::Attribute(value) => Some(format!("{}.{}", dotted_name(&value.value)?, value.attr)),
        _ => None,
    }
}

fn node(
    name: &str,
    children: Vec<SignatureExpression>,
    source: Option<String>,
) -> SignatureExpression {
    // Empty supported containers are semantic nodes, not opaque source-only
    // fallbacks. Their node tag alone distinguishes list, tuple, and dict.
    let source = if children.is_empty() && name != "unsupported" {
        None
    } else {
        source
    };
    SignatureExpression::LanguageSpecific {
        language: "python".into(),
        name: name.into(),
        children,
        source,
    }
}

fn operator(operator: Operator) -> &'static str {
    match operator {
        Operator::Add => "add",
        Operator::Sub => "subtract",
        Operator::Mult => "multiply",
        Operator::MatMult => "matrix-multiply",
        Operator::Div => "divide",
        Operator::Mod => "modulo",
        Operator::Pow => "power",
        Operator::LShift => "left-shift",
        Operator::RShift => "right-shift",
        Operator::BitOr => "union",
        Operator::BitXor => "bit-xor",
        Operator::BitAnd => "bit-and",
        Operator::FloorDiv => "floor-divide",
    }
}

fn string_literal(value: &str) -> String {
    let mut text = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            c if c.is_control() => text.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => text.push(c),
        }
    }
    text.push('"');
    text
}
