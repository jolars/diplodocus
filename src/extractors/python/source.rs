use std::collections::BTreeMap;

use ruff_python_ast::{self as ast, Expr, PySourceType, PythonVersion, Stmt};
use ruff_python_parser::{ParseOptions, parse_unchecked};
use ruff_text_size::Ranged;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    Signature, SourceEvidence, SourceLocation, SourceRole, SourceSpan, SourcedSignature,
};

use super::signatures::Expressions;
use super::{
    DeclarationKind, DocstringSourceSegment, ExportOperation, ExportOperationKind, ExportValue,
    ImportedName, ParsedDeclaration, ParsedDecorator, ParsedDocstring, ParsedImport, ParsedModule,
    PythonSourceKind, diagnostic, located, syntax_parsers,
};

pub(super) fn parse(
    text: &str,
    name: String,
    is_package: bool,
    kind: PythonSourceKind,
    source: SourceLocation,
    target_version: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> ParsedModule {
    let mut module = ParsedModule {
        name,
        is_package,
        kind,
        source,
        docstring: None,
        declarations: vec![],
        imports: vec![],
        exports: vec![],
        valid: true,
    };
    let source_type = match kind {
        PythonSourceKind::Source => PySourceType::Python,
        PythonSourceKind::Stub => PySourceType::Stub,
    };
    let version: PythonVersion = target_version.parse().expect("validated target grammar");
    let parsed = parse_unchecked(
        text,
        ParseOptions::from(source_type).with_target_version(version),
    );
    for error in parsed.errors() {
        module.valid = false;
        diagnostics.push(diagnostic(
            DiagnosticCode::PythonSyntax,
            Severity::Error,
            format!("Invalid Python syntax: {}", error.error),
            &located(&module.source, error.range()),
        ));
    }
    for error in parsed.unsupported_syntax_errors() {
        module.valid = false;
        diagnostics.push(diagnostic(
            DiagnosticCode::PythonUnsupportedVersion,
            Severity::Error,
            format!("Syntax is not supported by selected Python {target_version}: {error}"),
            &located(&module.source, error.range()),
        ));
    }
    if !module.valid {
        return module;
    }
    // Keep the parsed value, including tokens, alive while adapting the AST.
    let body = &parsed.syntax().as_module().expect("module parse mode").body;
    let mut context = Context {
        module_name: &module.name,
        is_package,
        expressions: Expressions {
            names: BTreeMap::new(),
            locals: BTreeMap::new(),
            qualify_locals: true,
            text,
            source: &module.source,
            diagnostics,
        },
    };
    module.docstring = context.docstring(body.first());
    module.declarations = context.declarations(body, true, &module.name);
    for statement in body {
        match statement {
            Stmt::Import(value) => module.imports.push(ParsedImport {
                module: None,
                level: 0,
                names: imports(&value.names),
                source: context.location(statement),
            }),
            Stmt::ImportFrom(value) => module.imports.push(ParsedImport {
                module: value.module.as_ref().map(ToString::to_string),
                level: value.level,
                names: imports(&value.names),
                source: context.location(statement),
            }),
            _ => {}
        }
        if let Some(operation) = context.export(statement) {
            module.exports.push(operation);
        }
    }
    module
}

fn imports(names: &[ast::Alias]) -> Vec<ImportedName> {
    names
        .iter()
        .map(|name| ImportedName {
            name: name.name.to_string(),
            alias: name.asname.as_ref().map(ToString::to_string),
        })
        .collect()
}

struct Context<'a> {
    module_name: &'a str,
    is_package: bool,
    expressions: Expressions<'a>,
}

impl Context<'_> {
    fn location(&self, value: &impl Ranged) -> SourceLocation {
        located(self.expressions.source, value.range())
    }

    fn declarations(
        &mut self,
        body: &[Stmt],
        module: bool,
        qualifier: &str,
    ) -> Vec<ParsedDeclaration> {
        let saved_names = self.expressions.names.clone();
        let saved_locals = self.expressions.locals.clone();
        for statement in body {
            let names = match statement {
                Stmt::FunctionDef(value) => vec![value.name.as_str()],
                Stmt::ClassDef(value) => vec![value.name.as_str()],
                Stmt::Assign(value) => value
                    .targets
                    .iter()
                    .filter_map(|target| match target {
                        Expr::Name(name) => Some(name.id.as_str()),
                        _ => None,
                    })
                    .collect(),
                Stmt::AnnAssign(value) => match &*value.target {
                    Expr::Name(name) => vec![name.id.as_str()],
                    _ => vec![],
                },
                Stmt::TypeAlias(value) => match &*value.name {
                    Expr::Name(name) => vec![name.id.as_str()],
                    _ => vec![],
                },
                _ => vec![],
            };
            for name in names {
                self.expressions
                    .locals
                    .insert(name.into(), format!("{qualifier}.{name}"));
            }
        }
        let mut declarations = Vec::new();
        for (index, statement) in body.iter().enumerate() {
            match statement {
                Stmt::Import(value) if module => {
                    for alias in &value.names {
                        let (binding, canonical) = alias.asname.as_ref().map_or_else(
                            || { let root = alias.name.as_str().split('.').next().unwrap(); (root.to_owned(), root.to_owned()) },
                            |name| (name.to_string(), alias.name.to_string()));
                        self.expressions.names.insert(binding, canonical);
                    }
                }
                Stmt::ImportFrom(value) if module => {
                    let mut parts: Vec<_> = self.module_name.split('.').collect();
                    if !self.is_package { parts.pop(); }
                    let prefix = if value.level == 0 { Some(String::new()) }
                        else if value.level as usize <= parts.len() {
                            parts.truncate(parts.len() + 1 - value.level as usize);
                            Some(parts.join("."))
                        } else { None };
                    if let Some(mut prefix) = prefix {
                        if let Some(module) = &value.module {
                            if !prefix.is_empty() { prefix.push('.'); }
                            prefix.push_str(module.as_str());
                        }
                        for alias in &value.names {
                            if alias.name.as_str() == "*" { continue; }
                            let binding = alias.asname.as_ref().map_or_else(|| alias.name.to_string(), ToString::to_string);
                            self.expressions.names.insert(binding, format!("{prefix}.{}", alias.name));
                        }
                    }
                }
                Stmt::FunctionDef(function) => {
                    let mut declaration = self.declaration(function.name.to_string(), DeclarationKind::Function, statement);
                    if function.type_params.is_some() { self.unsupported(statement, "Generic type parameters are not yet represented in Python signatures."); }
                    let signature = self.expressions.callable(function);
                    declaration.signature = Some(self.signature(signature, &declaration.source));
                    declaration.decorators = self.decorators(&function.decorator_list);
                    declaration.is_async = function.is_async;
                    declaration.docstring = self.docstring(function.body.first());
                    declarations.push(declaration);
                    self.bind_local(function.name.as_str());
                }
                Stmt::ClassDef(class) => {
                    let mut declaration = self.declaration(class.name.to_string(), DeclarationKind::Class, statement);
                    if class.type_params.is_some() { self.unsupported(statement, "Generic class type parameters are not yet represented."); }
                    declaration.decorators = self.decorators(&class.decorator_list);
                    if let Some(arguments) = &class.arguments {
                        declaration.bases = arguments.args.iter().map(|value| self.expressions.expression(value)).collect();
                        if !arguments.keywords.is_empty() { self.unsupported(statement, "Class keyword arguments (including metaclasses) have unsupported static semantics."); }
                    }
                    declaration.members = self.declarations(&class.body, false, &format!("{qualifier}.{}", class.name));
                    declaration.docstring = self.docstring(class.body.first());
                    declarations.push(declaration);
                    self.bind_local(class.name.as_str());
                }
                Stmt::Assign(value) => {
                    for target in &value.targets {
                        if let Expr::Name(name) = target {
                            if name.id == "__all__" && module { continue; }
                            let mut declaration = self.declaration(name.id.to_string(), DeclarationKind::Assignment, statement);
                            let signature = Signature::Value { annotation: None, value: Some(self.assignment_value(&value.value)) };
                            declaration.signature = Some(self.signature(signature, &declaration.source));
                            declaration.docstring = self.docstring(body.get(index + 1));
                            declarations.push(declaration);
                            self.bind_local(name.id.as_str());
                        } else if !contains_all(statement) {
                            self.unsupported(statement, "Only simple named assignment declarations are supported.");
                        }
                    }
                }
                Stmt::AnnAssign(value) => {
                    if let Expr::Name(name) = &*value.target {
                        if name.id == "__all__" && module { continue; }
                        let mut declaration = self.declaration(name.id.to_string(), DeclarationKind::Assignment, statement);
                        let signature = Signature::Value { annotation: Some(self.expressions.expression(&value.annotation)),
                            value: value.value.as_deref().map(|value| self.assignment_value(value)) };
                        declaration.signature = Some(self.signature(signature, &declaration.source));
                        declaration.docstring = self.docstring(body.get(index + 1));
                        declarations.push(declaration);
                        self.bind_local(name.id.as_str());
                    } else if !contains_all(statement) {
                        self.unsupported(statement, "Only simple named annotated declarations are supported.");
                    }
                }
                Stmt::TypeAlias(value) => {
                    if let Expr::Name(name) = &*value.name {
                        let mut declaration = self.declaration(name.id.to_string(), DeclarationKind::TypeAlias, statement);
                        if value.type_params.is_some() { self.unsupported(statement, "Generic type alias parameters are not yet represented."); }
                        let signature = Signature::Value { annotation: None, value: Some(self.expressions.expression(&value.value)) };
                        declaration.signature = Some(self.signature(signature, &declaration.source));
                        declaration.docstring = self.docstring(body.get(index + 1));
                        declarations.push(declaration);
                        self.bind_local(name.id.as_str());
                    }
                }
                Stmt::If(_) | Stmt::For(_) | Stmt::While(_) | Stmt::Try(_) | Stmt::With(_) | Stmt::Match(_) =>
                    self.unsupported(statement, "Conditional or dynamically scoped declarations cannot define an authoritative static surface."),
                Stmt::Import(_) | Stmt::ImportFrom(_) if !module =>
                    self.unsupported(statement, "Class-scoped imports are outside the supported declaration subset."),
                Stmt::AugAssign(_) | Stmt::Delete(_) if !contains_all(statement) =>
                    self.unsupported(statement, "Mutation of a declaration is outside the supported static subset."),
                _ => {}
            }
        }
        self.expressions.names = saved_names;
        self.expressions.locals = saved_locals;
        declarations
    }

    fn bind_local(&mut self, name: &str) {
        if let Some(canonical) = self.expressions.locals.get(name) {
            self.expressions
                .names
                .insert(name.into(), canonical.clone());
        }
    }

    fn declaration(
        &self,
        name: String,
        kind: DeclarationKind,
        statement: &Stmt,
    ) -> ParsedDeclaration {
        ParsedDeclaration {
            name,
            kind,
            source: self.location(statement),
            signature: None,
            decorators: vec![],
            is_async: false,
            bases: vec![],
            members: vec![],
            docstring: None,
        }
    }

    fn signature(&self, signature: Signature, location: &SourceLocation) -> SourcedSignature {
        SourcedSignature {
            signature,
            sources: vec![SourceEvidence {
                source: location.clone(),
                role: SourceRole::Signature,
                parsers: syntax_parsers(),
            }],
        }
    }

    fn decorators(&mut self, decorators: &[ast::Decorator]) -> Vec<ParsedDecorator> {
        self.expressions.qualify_locals = false;
        let result = decorators
            .iter()
            .map(|value| ParsedDecorator {
                expression: if matches!(&value.expression, Expr::Attribute(attribute) if matches!(attribute.attr.as_str(), "setter" | "deleter" | "getter")) {
                    super::signatures::dotted_name(&value.expression).map_or_else(
                        || self.expressions.expression(&value.expression),
                        |name| crate::ir::SignatureExpression::Name { name, target: None })
                } else { self.expressions.expression(&value.expression) },
                source: self.location(value),
            })
            .collect();
        self.expressions.qualify_locals = true;
        result
    }

    fn assignment_value(&mut self, value: &Expr) -> crate::ir::SignatureExpression {
        // Export helper evaluation and alias reconciliation use lexical binding
        // names and source order, not canonical item references assigned here.
        self.expressions.qualify_locals = false;
        let saved_names = self.expressions.names.clone();
        self.expressions
            .names
            .retain(|name, canonical| self.expressions.locals.get(name) != Some(canonical));
        let expression = self.expressions.expression(value);
        self.expressions.names = saved_names;
        self.expressions.qualify_locals = true;
        expression
    }

    fn docstring(&self, statement: Option<&Stmt>) -> Option<ParsedDocstring> {
        let Stmt::Expr(statement) = statement? else {
            return None;
        };
        let Expr::StringLiteral(literal) = &*statement.value else {
            return None;
        };
        let mut segments = Vec::new();
        let mut offset = 0;
        for part in literal.value.iter() {
            let range = part.content_range();
            let start = usize::from(range.start());
            let end = usize::from(range.end());
            let length = part.as_str().len();
            if &self.expressions.text[start..end] == part.as_str() {
                segments.push(DocstringSourceSegment {
                    decoded: SourceSpan {
                        start: offset,
                        end: offset + length,
                    },
                    source: SourceSpan { start, end },
                });
            }
            offset += length;
        }
        Some(ParsedDocstring {
            text: literal.value.to_str().into(),
            source: self.location(literal),
            segments,
        })
    }

    fn unsupported(&mut self, statement: &impl Ranged, message: &str) {
        self.expressions.diagnostics.push(diagnostic(
            DiagnosticCode::PythonUnsupportedSyntax,
            Severity::Error,
            message,
            &self.location(statement),
        ));
    }

    fn export(&self, statement: &Stmt) -> Option<ExportOperation> {
        let source = self.location(statement);
        let operation = |kind, value| {
            Some(ExportOperation {
                kind,
                value,
                source: source.clone(),
            })
        };
        match statement {
            Stmt::Assign(value) if value.targets.iter().any(is_all) => {
                operation(ExportOperationKind::Assign, self.export_value(&value.value))
            }
            Stmt::AnnAssign(value) if is_all(&value.target) => value
                .value
                .as_deref()
                .and_then(|value| operation(ExportOperationKind::Assign, self.export_value(value))),
            Stmt::AugAssign(value) if is_all(&value.target) && value.op == ast::Operator::Add => {
                operation(ExportOperationKind::Extend, self.export_value(&value.value))
            }
            Stmt::Expr(value) => {
                if let Expr::Call(call) = &*value.value
                    && let Expr::Attribute(attribute) = &*call.func
                    && is_all(&attribute.value)
                    && call.arguments.args.len() == 1
                    && call.arguments.keywords.is_empty()
                {
                    let argument = &call.arguments.args[0];
                    match attribute.attr.as_str() {
                        "extend" => {
                            return operation(
                                ExportOperationKind::Extend,
                                self.export_value(argument),
                            );
                        }
                        "append" => {
                            if let Expr::StringLiteral(value) = argument {
                                return operation(
                                    ExportOperationKind::Append,
                                    ExportValue::Names(vec![value.value.to_str().into()]),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                contains_all(statement).then(|| self.unsupported_export(statement))
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => None,
            _ => contains_all(statement).then(|| self.unsupported_export(statement)),
        }
    }

    fn unsupported_export(&self, statement: &Stmt) -> ExportOperation {
        ExportOperation {
            kind: ExportOperationKind::Unsupported,
            value: ExportValue::Unsupported(self.raw(statement)),
            source: self.location(statement),
        }
    }

    fn export_value(&self, value: &Expr) -> ExportValue {
        match value {
            Expr::List(list) => self
                .export_names(&list.elts)
                .unwrap_or_else(|| ExportValue::Unsupported(self.raw(value))),
            Expr::Tuple(tuple) => self
                .export_names(&tuple.elts)
                .unwrap_or_else(|| ExportValue::Unsupported(self.raw(value))),
            Expr::Name(name) => ExportValue::Name(name.id.to_string()),
            Expr::BinOp(value) if value.op == ast::Operator::Add => ExportValue::Concat(vec![
                self.export_value(&value.left),
                self.export_value(&value.right),
            ]),
            _ => ExportValue::Unsupported(self.raw(value)),
        }
    }

    fn export_names(&self, values: &[Expr]) -> Option<ExportValue> {
        values
            .iter()
            .map(|value| match value {
                Expr::StringLiteral(value) => Some(value.value.to_str().to_owned()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(ExportValue::Names)
    }

    fn raw(&self, value: &impl Ranged) -> String {
        self.expressions.text[usize::from(value.start())..usize::from(value.end())].into()
    }
}

fn is_all(value: &Expr) -> bool {
    matches!(value, Expr::Name(name) if name.id == "__all__")
}

fn contains_all(statement: &Stmt) -> bool {
    use ast::visitor::{Visitor, walk_expr};
    struct Find(bool);
    impl<'a> Visitor<'a> for Find {
        fn visit_expr(&mut self, expression: &'a Expr) {
            if is_all(expression) {
                self.0 = true;
            }
            walk_expr(self, expression);
        }
    }
    let mut visitor = Find(false);
    visitor.visit_stmt(statement);
    visitor.0
}
