use std::collections::BTreeSet;

use arity_parser::{
    ast::{
        AssignmentExpr, AstNode, AstToken, CallExpr, Expr, FunctionExpr, HasArgList, token_name,
    },
    parser,
    syntax::{SyntaxElement, SyntaxKind},
};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    Parameter, ParameterKind, RDeclaration, Signature, SignatureExpression, SourceLocation,
    SourceRole, SourcedSignature,
};

use super::{diagnostic, evidence, located};

pub(super) struct Definition {
    pub name: String,
    pub source: SourceLocation,
    pub value: DefinitionValue,
    required_builtins: BTreeSet<String>,
}

pub(super) enum DefinitionValue {
    Function {
        signature: Box<SourcedSignature>,
        declaration: RDeclaration,
    },
    Alias(String),
    Unsupported,
}

pub(super) fn parse(
    text: &str,
    source: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Definition> {
    let parsed = parser::parse(text);
    if !parsed.diagnostics.is_empty() {
        for error in parsed.diagnostics {
            diagnostics.push(diagnostic(
                DiagnosticCode::RSyntax,
                Severity::Error,
                error.message,
                &located(source, error.start, error.end),
            ));
        }
        return vec![];
    }
    let mut definitions = vec![];
    for node in parsed.cst.children() {
        let Some(assignment) = AssignmentExpr::cast(node.clone()) else {
            if let Some(expr) = Expr::cast_node(node) {
                uncertain_bindings(expr, source, &mut definitions);
            }
            continue;
        };
        let Some(token) = assignment.target_name_token() else {
            continue;
        };
        let name = token_name(&token).trim_matches('`').to_owned();
        let range = assignment.syntax().text_range();
        let location = located(source, usize::from(range.start()), usize::from(range.end()));
        let mut required_builtins = BTreeSet::new();
        let value = if !matches!(
            assignment.op_kind(),
            Some(SyntaxKind::ASSIGN_LEFT | SyntaxKind::ASSIGN_EQ | SyntaxKind::ASSIGN_RIGHT)
        ) || name.contains('\\')
        {
            DefinitionValue::Unsupported
        } else {
            match assignment.value_element().and_then(Expr::cast) {
                Some(Expr::Function(function)) => {
                    let formals = function.formals();
                    let mut names = BTreeSet::new();
                    if formals.iter().any(|formal| {
                        !names.insert(formal.name())
                            || (formal.name() == "..." && formal.default().is_some())
                    }) {
                        diagnostics.push(diagnostic(DiagnosticCode::RUnsupportedSurface, Severity::Error, "Repeated formal names or a default for `...` cannot establish a valid R signature.", &location));
                        definitions.push(Definition {
                            name,
                            source: location,
                            value: DefinitionValue::Unsupported,
                            required_builtins,
                        });
                        continue;
                    }
                    let signature = signature(&function, &location);
                    match classify(&function, &mut required_builtins) {
                        Some(declaration) => DefinitionValue::Function {
                            signature: Box::new(signature),
                            declaration,
                        },
                        None => DefinitionValue::Unsupported,
                    }
                }
                Some(Expr::Name(name)) if !name.is_reserved_constant() => {
                    DefinitionValue::Alias(name.name().trim_matches('`').into())
                }
                Some(Expr::StringLiteral(name)) if name.is_backtick() => name
                    .unquote()
                    .filter(|name| !name.contains('\\'))
                    .map(|name| DefinitionValue::Alias(name.into()))
                    .unwrap_or(DefinitionValue::Unsupported),
                _ => DefinitionValue::Unsupported,
            }
        };
        definitions.push(Definition {
            name,
            source: location,
            value,
            required_builtins,
        });
    }
    definitions
}

fn uncertain_bindings(expr: Expr, source: &SourceLocation, definitions: &mut Vec<Definition>) {
    if matches!(expr, Expr::Function(_)) {
        return;
    }
    let name = match &expr {
        Expr::Assignment(assignment) => assignment
            .target_name()
            .map(|n| n.trim_matches('`').to_owned()),
        Expr::Call(call) if matches!(callee(call).as_deref(), Some("assign" | "base::assign")) => {
            call.named_arg("x")
                .or_else(|| call.nth_positional(0))
                .and_then(Expr::cast)
                .as_ref()
                .and_then(static_name)
        }
        _ => None,
    };
    if let Some(name) = name {
        let range = expr.text_range();
        definitions.push(Definition {
            name,
            source: located(source, usize::from(range.start()), usize::from(range.end())),
            value: DefinitionValue::Unsupported,
            required_builtins: BTreeSet::new(),
        });
    }
    if let SyntaxElement::Node(node) = expr.syntax() {
        for child in node.children_with_tokens().filter_map(Expr::cast) {
            uncertain_bindings(child, source, definitions);
        }
    }
}

fn signature(function: &FunctionExpr, source: &SourceLocation) -> SourcedSignature {
    let mut after_dots = false;
    let mut sources = vec![evidence(source, SourceRole::Signature)];
    let parameters = function
        .formals()
        .into_iter()
        .map(|formal| {
            let name = token_name(&formal.name_token())
                .trim_matches('`')
                .to_owned();
            let kind = if name == "..." {
                after_dots = true;
                ParameterKind::LanguageSpecific {
                    language: "r".into(),
                    name: "dots".into(),
                }
            } else if after_dots {
                ParameterKind::KeywordOnly
            } else {
                ParameterKind::PositionalOrKeyword
            };
            let range = formal.text_range();
            sources.push(evidence(
                &located(source, usize::from(range.start()), usize::from(range.end())),
                SourceRole::Signature,
            ));
            Parameter {
                name,
                kind,
                annotation: None,
                default: formal.default().map(expression),
            }
        })
        .collect();
    SourcedSignature {
        signature: Signature::Callable {
            parameters,
            returns: None,
        },
        sources,
    }
}

fn classify(
    function: &FunctionExpr,
    required_builtins: &mut BTreeSet<String>,
) -> Option<RDeclaration> {
    let body = function.body().and_then(Expr::cast)?;
    let terminal = terminal(body.clone());
    if let Expr::Call(call) = &terminal {
        let callee = callee(call);
        if matches!(callee.as_deref(), Some("UseMethod" | "base::UseMethod")) {
            if callee.as_deref() == Some("UseMethod") && shadows(function, "UseMethod") {
                return None;
            }
            if callee.as_deref() == Some("UseMethod") {
                required_builtins.insert("UseMethod".into());
            }
            let generic = call
                .named_arg("generic")
                .or_else(|| call.nth_positional(0))
                .and_then(Expr::cast)
                .and_then(|expr| literal_string(&expr))?;
            let object = call
                .named_arg("object")
                .or_else(|| call.nth_positional(1))
                .map(expression);
            return Some(RDeclaration::S3Generic {
                dispatch_name: generic,
                dispatch_object: object,
                methods: vec![],
            });
        }
        if matches!(callee.as_deref(), Some("structure" | "base::structure")) {
            if callee.as_deref() == Some("structure") {
                if shadows(function, "structure") {
                    return None;
                }
                required_builtins.insert("structure".into());
            }
            if let Some(class) = call.named_arg("class").and_then(Expr::cast) {
                let classes = match class {
                    Expr::Call(ref call)
                        if matches!(self::callee(call).as_deref(), Some("c" | "base::c")) =>
                    {
                        if self::callee(call).as_deref() == Some("c") {
                            if shadows(function, "c") {
                                return None;
                            }
                            required_builtins.insert("c".into());
                        }
                        call.args()
                            .map(|a| {
                                a.value()
                                    .and_then(Expr::cast)
                                    .and_then(|e| literal_string(&e))
                            })
                            .collect::<Option<Vec<_>>>()
                    }
                    _ => literal_string(&class).map(|c| vec![c]),
                };
                if let Some(classes) = classes
                    && !classes.is_empty()
                {
                    return Some(RDeclaration::Constructor { classes });
                }
                return None;
            }
        }
    }
    // A nested closure's dispatch does not make its enclosing function generic.
    if contains_dispatch(&body) {
        return None;
    }
    Some(RDeclaration::Function)
}

fn shadows(function: &FunctionExpr, name: &str) -> bool {
    fn binds(expr: Expr, name: &str) -> bool {
        if matches!(expr, Expr::Function(_)) {
            return false;
        }
        if let Expr::Assignment(assignment) = &expr
            && assignment
                .target_name()
                .is_some_and(|n| n.trim_matches('`') == name)
        {
            return true;
        }
        if let Expr::Call(call) = &expr
            && matches!(callee(call).as_deref(), Some("assign" | "base::assign"))
            && call
                .named_arg("x")
                .or_else(|| call.nth_positional(0))
                .and_then(Expr::cast)
                .as_ref()
                .and_then(literal_string)
                .is_none_or(|binding| binding == name)
        {
            return true;
        }
        match expr.syntax() {
            SyntaxElement::Node(node) => node
                .children_with_tokens()
                .filter_map(Expr::cast)
                .any(|child| binds(child, name)),
            _ => false,
        }
    }
    function
        .formals()
        .iter()
        .any(|p| p.name().trim_matches('`') == name)
        || function
            .body()
            .and_then(Expr::cast)
            .is_some_and(|body| binds(body, name))
}

pub(super) fn reject_shadowed_builtins(
    definitions: &mut [Definition],
    imports: impl Iterator<Item = String>,
) {
    let names: BTreeSet<_> = definitions
        .iter()
        .map(|d| d.name.clone())
        .chain(imports)
        .collect();
    for definition in definitions {
        if !definition.required_builtins.is_disjoint(&names) {
            definition.value = DefinitionValue::Unsupported;
        }
    }
}

fn contains_dispatch(expr: &Expr) -> bool {
    if matches!(expr, Expr::Function(_)) {
        return false;
    }
    if let Expr::Call(call) = expr
        && matches!(
            callee(call).as_deref(),
            Some("UseMethod" | "base::UseMethod")
        )
    {
        return true;
    }
    match expr.syntax() {
        SyntaxElement::Node(node) => node
            .children_with_tokens()
            .filter_map(Expr::cast)
            .any(|child| contains_dispatch(&child)),
        _ => false,
    }
}

fn terminal(expr: Expr) -> Expr {
    match expr {
        Expr::Block(ref block) => block
            .statements()
            .filter_map(Expr::cast)
            .last()
            .map(terminal)
            .unwrap_or(expr),
        Expr::Call(ref call) if callee(call).as_deref() == Some("return") => call
            .nth_positional(0)
            .and_then(Expr::cast)
            .map(terminal)
            .unwrap_or(expr),
        _ => expr,
    }
}

pub(super) fn static_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.name().trim_matches('`').into()),
        Expr::StringLiteral(name) => name
            .unquote()
            .filter(|value| !value.is_empty() && !value.contains('\\'))
            .map(str::to_owned),
        Expr::Binary(binary) => binary
            .namespace_access()
            .filter(|n| !n.internal)
            .map(|n| format!("{}::{}", n.package, n.name)),
        _ => None,
    }
}

fn literal_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(name) if !name.is_backtick() => static_name(expr),
        _ => None,
    }
}

pub(super) fn callee(call: &CallExpr) -> Option<String> {
    call.base()
        .and_then(Expr::cast)
        .as_ref()
        .and_then(static_name)
}

pub(super) fn expression(element: SyntaxElement) -> SignatureExpression {
    match Expr::cast(element.clone()) {
        Some(Expr::Name(name)) if !name.is_reserved_constant() => SignatureExpression::Name {
            name: name.name().trim_matches('`').into(),
            target: None,
        },
        Some(Expr::StringLiteral(ref string)) if string.is_backtick() => {
            SignatureExpression::Name {
                name: token_name(string.syntax()).into(),
                target: None,
            }
        }
        Some(
            Expr::Name(_)
            | Expr::StringLiteral(_)
            | Expr::FloatLiteral(_)
            | Expr::IntLiteral(_)
            | Expr::ComplexLiteral(_),
        ) => SignatureExpression::Literal {
            text: element.to_string(),
        },
        Some(Expr::Call(call)) => SignatureExpression::Apply {
            constructor: Box::new(
                call.base()
                    .map(expression)
                    .unwrap_or_else(|| opaque("missing-callee", vec![], None)),
            ),
            arguments: call
                .args()
                .map(|arg| {
                    let value = arg
                        .value()
                        .map(expression)
                        .unwrap_or_else(|| opaque("missing-argument", vec![], None));
                    match arg.name() {
                        Some(name) => opaque(&format!("named-argument:{name}"), vec![value], None),
                        None => value,
                    }
                })
                .collect(),
        },
        _ => match element {
            SyntaxElement::Node(node) => opaque(
                &format!("{:?}", node.kind()).to_ascii_lowercase(),
                node.children_with_tokens()
                    .filter(|e| {
                        !arity_parser::ast::kinds::is_trivia(e.kind())
                            && e.kind() != SyntaxKind::COMMENT
                    })
                    .map(expression)
                    .collect(),
                Some(node.to_string()),
            ),
            SyntaxElement::Token(token) => SignatureExpression::Literal {
                text: token.text().into(),
            },
        },
    }
}

fn opaque(
    name: &str,
    children: Vec<SignatureExpression>,
    source: Option<String>,
) -> SignatureExpression {
    SignatureExpression::LanguageSpecific {
        language: "r".into(),
        name: name.into(),
        children,
        source,
    }
}
