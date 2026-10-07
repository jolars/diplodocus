use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fatou_parser::ast::{
    AstNode, CallExpr, DocAttachment, DocText, HasArgList, ModuleDef, StringLiteral,
    TypeAnnotation, body_of,
};
use fatou_parser::syntax::{SyntaxKind as K, SyntaxNode};

use crate::diagnostics::{DiagnosticCode as Code, Severity};
use crate::ir::*;
use crate::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths};
use crate::provenance::ExtractionObservation;

use super::signatures::{ParsedSignature, lower, node_name};
use super::{JuliaExtraction, diagnostic, located, normalized_path, read_input, signatures};

pub(super) struct Draft {
    pub module: String,
    pub name: String,
    pub declaration: DraftDeclaration,
    pub documentation: Option<SourcedDocument>,
    pub documented: bool,
    pub source: SourceLocation,
    pub wrappers: Vec<SourceLocation>,
}

pub(super) enum DraftDeclaration {
    Named {
        kind: JuliaIdentityKind,
        declaration: JuliaDeclaration,
        signature: Option<Signature>,
    },
    Callable {
        signature: ParsedSignature,
        macro_definition: bool,
    },
    Generic,
    Alias {
        target: String,
    },
    Documentation {
        selector: Option<SignatureExpression>,
    },
}

pub(super) struct Export {
    pub module: String,
    pub name: String,
    pub visibility: JuliaVisibility,
    pub source: SourceLocation,
}

pub(super) struct Import {
    pub module: String,
    pub name: String,
    pub target: String,
    pub source: SourceLocation,
    pub whole_using: bool,
}

#[derive(Default)]
pub(super) struct Declarations {
    pub drafts: Vec<Draft>,
    pub exports: Vec<Export>,
    pub imports: Vec<Import>,
}

struct Context<'a> {
    repository: &'a ResolvedRepositoryPaths,
    package: &'a ResolvedPackagePaths,
    result: &'a mut JuliaExtraction,
    observation: &'a mut ExtractionObservation,
    cache: BTreeMap<PathBuf, Option<SyntaxNode>>,
    active: Vec<PathBuf>,
    includes: Vec<(String, SourceLocation)>,
    declarations: Declarations,
}

pub(super) fn extract(
    repository: &ResolvedRepositoryPaths,
    package: &ResolvedPackagePaths,
    entry: &Path,
    result: &mut JuliaExtraction,
    observation: &mut ExtractionObservation,
) -> Declarations {
    let mut context = Context {
        repository,
        package,
        result,
        observation,
        cache: BTreeMap::new(),
        active: Vec::new(),
        includes: Vec::new(),
        declarations: Declarations::default(),
    };
    context.file(entry, "", None);
    for (module, source) in std::mem::take(&mut context.includes) {
        let shadowed = context
            .declarations
            .drafts
            .iter()
            .any(|draft| draft.module == module && draft.name == "include")
            || context
                .declarations
                .imports
                .iter()
                .any(|import| import.module == module && import.name == "include");
        if shadowed {
            context.error(
                Code::JuliaInclude,
                "A locally declared include binding cannot establish standard include semantics.",
                &source,
            );
        }
    }
    context.declarations
}

impl Context<'_> {
    fn file(&mut self, path: &Path, module: &str, include: Option<&SourceLocation>) {
        let path = normalized_path(path);
        if !path.starts_with(&self.package.path) {
            if let Some(source) = include {
                self.error(
                    Code::SourcePathOutsideBoundary,
                    "A Julia include escapes the configured package.",
                    source,
                );
            }
            return;
        }
        let canonical = match std::fs::canonicalize(&path) {
            Ok(canonical) if canonical.starts_with(&self.package.path) => canonical,
            Ok(_) => {
                if let Some(source) = include {
                    self.error(
                        Code::SourcePathOutsideBoundary,
                        "A Julia include symlink escapes the configured package.",
                        source,
                    );
                }
                return;
            }
            Err(_) => {
                if let Some(source) = include {
                    self.error(
                        Code::JuliaSourceRead,
                        "Cannot read a statically included Julia file.",
                        source,
                    );
                }
                return;
            }
        };
        if self.active.contains(&canonical) {
            if let Some(source) = include {
                self.error(
                    Code::JuliaInclude,
                    "A Julia include cycle prevents static extraction.",
                    source,
                );
            }
            return;
        }
        let Some(input) = read_input(
            self.repository,
            self.package,
            &path,
            "julia-source",
            self.result,
            self.observation,
        ) else {
            return;
        };
        let Some(text) = &input.text else { return };
        let cst = if let Some(cst) = self.cache.get(&canonical) {
            cst.clone()
        } else {
            let parsed = fatou_parser::parser::parse(text);
            for native in &parsed.diagnostics {
                let mut source = input.source.clone();
                source.span = Some(SourceSpan {
                    start: native.start,
                    end: native.end,
                });
                self.error(
                    Code::JuliaSyntax,
                    format!("Julia syntax error: {}", native.message),
                    &source,
                );
            }
            let cst = parsed.diagnostics.is_empty().then_some(parsed.cst);
            self.cache.insert(canonical.clone(), cst.clone());
            cst
        };
        if let Some(cst) = cst {
            self.active.push(canonical);
            for node in cst.children() {
                self.walk(&node, &input.source, &path, module, None, false, Vec::new());
            }
            self.active.pop();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        node: &SyntaxNode,
        file_source: &SourceLocation,
        file: &Path,
        module: &str,
        documentation: Option<SourcedDocument>,
        documented: bool,
        wrappers: Vec<SourceLocation>,
    ) {
        let source = located(file_source, node.text_range());
        if let Some(attachment) = DocAttachment::cast(node.clone()) {
            let document = match attachment.text() {
                DocText::Static(text) => {
                    let parsed = super::docstrings::parse(
                        &text,
                        located(file_source, attachment.payload().text_range()),
                    );
                    self.result.diagnostics.extend(parsed.diagnostics);
                    Some(parsed.document)
                }
                DocText::Opaque(reason) => {
                    self.result.diagnostics.push(diagnostic(
                        Code::JuliaUnsupportedDocstring,
                        Severity::Warning,
                        format!("Documentation requires evaluation ({reason:?}); source retained."),
                        &located(file_source, attachment.payload().text_range()),
                    ));
                    Some(super::docstrings::opaque(
                        attachment.payload().text().to_string(),
                        located(file_source, attachment.payload().text_range()),
                    ))
                }
                DocText::Invalid(_) => {
                    self.error(
                        Code::JuliaUnsupportedDocstring,
                        "Documentation contains an invalid string literal.",
                        &source,
                    );
                    None
                }
            };
            self.walk(
                attachment.target(),
                file_source,
                file,
                module,
                document,
                true,
                wrappers,
            );
            return;
        }
        let mut draft = Draft {
            module: module.into(),
            name: String::new(),
            declaration: DraftDeclaration::Generic,
            documentation,
            documented,
            source: source.clone(),
            wrappers,
        };
        match node.kind() {
            K::MODULE_DEF => {
                let Some(value) = ModuleDef::cast(node.clone())
                    .and_then(|n| n.name())
                    .and_then(|n| node_name(n.syntax()))
                else {
                    self.unsupported(node, &source);
                    return;
                };
                draft.name = value.clone();
                draft.declaration = DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Module,
                    declaration: JuliaDeclaration::Module {
                        bare: has_token(node, K::BAREMODULE_KW),
                    },
                    signature: None,
                };
                self.declarations.drafts.push(draft);
                if let Some(block) = body_of(node) {
                    let nested = qualify(module, &value);
                    for child in block.children() {
                        self.walk(&child, file_source, file, &nested, None, false, Vec::new());
                    }
                }
            }
            K::FUNCTION_DEF | K::MACRO_DEF => {
                let Some(start) = header(node) else {
                    self.unsupported(node, &source);
                    return;
                };
                if let Some(mut signature) = signatures::parse(&start) {
                    let is_macro = node.kind() == K::MACRO_DEF;
                    if is_macro {
                        signature.name = format!("@{}", signature.name);
                    }
                    draft.name = signature.name.clone();
                    draft.declaration = DraftDeclaration::Callable {
                        signature,
                        macro_definition: is_macro,
                    };
                } else if node.kind() == K::FUNCTION_DEF
                    && let Some(name) = node_name(&start)
                {
                    draft.name = name;
                } else {
                    self.unsupported(node, &source);
                    return;
                }
                self.declarations.drafts.push(draft);
            }
            K::ASSIGNMENT_EXPR => {
                let Some((lhs, rhs)) = signatures::assignment(node) else {
                    self.unsupported(node, &source);
                    return;
                };
                if let Some(signature) = signatures::parse(&lhs) {
                    draft.name = signature.name.clone();
                    draft.declaration = DraftDeclaration::Callable {
                        signature,
                        macro_definition: false,
                    };
                    self.declarations.drafts.push(draft);
                } else if let Some(name) = node_name(&lhs) {
                    if let Some(target) = node_name(&rhs) {
                        draft.name = name;
                        draft.declaration = DraftDeclaration::Alias { target };
                        self.declarations.drafts.push(draft);
                    } else if draft.documented {
                        self.unsupported(node, &source);
                    }
                } else {
                    self.unsupported(node, &source);
                }
            }
            K::STRUCT_DEF | K::ABSTRACT_DEF | K::PRIMITIVE_DEF => {
                let Some(mut start) = header(node) else {
                    self.unsupported(node, &source);
                    return;
                };
                let mut supertype = None;
                if has_token(&start, K::SUBTYPE) {
                    let mut children = start.children();
                    let Some(base) = children.next() else {
                        self.unsupported(node, &source);
                        return;
                    };
                    supertype = children.next().map(|n| lower(&n));
                    start = base;
                }
                let Some(name) = node_name(&start) else {
                    self.unsupported(node, &source);
                    return;
                };
                let parameters = start
                    .children()
                    .find(|n| n.kind() == K::ARG_LIST)
                    .map(|list| list.children().map(|n| lower(&n)).collect())
                    .unwrap_or_default();
                let flavor = match node.kind() {
                    K::STRUCT_DEF => JuliaTypeKind::Struct {
                        mutable: has_token(node, K::MUTABLE_KW),
                    },
                    K::ABSTRACT_DEF => JuliaTypeKind::Abstract,
                    _ => JuliaTypeKind::Primitive {
                        bits: node
                            .descendants_with_tokens()
                            .filter_map(|e| e.into_token())
                            .find(|t| t.kind() == K::INTEGER)
                            .map(|t| t.text().to_string()),
                    },
                };
                draft.name = name.clone();
                draft.declaration = DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Type,
                    declaration: JuliaDeclaration::Type {
                        flavor,
                        parameters,
                        supertype,
                        constructors: Vec::new(),
                    },
                    signature: Some(Signature::LanguageSpecific {
                        language: "julia".into(),
                        syntax: lower(&header(node).unwrap()),
                    }),
                };
                self.declarations.drafts.push(draft);
                if let Some(block) = body_of(node) {
                    for child in block.children() {
                        self.field(&child, file_source, file, module, &name);
                    }
                }
            }
            K::CONST_STMT => {
                let Some(child) = node.children().next() else {
                    self.unsupported(node, &source);
                    return;
                };
                let (pattern, value) = if let Some((lhs, rhs)) = signatures::assignment(&child) {
                    (lhs, Some(lower(&rhs)))
                } else {
                    (child, None)
                };
                let (name, annotation) =
                    if let Some(annotation) = TypeAnnotation::cast(pattern.clone()) {
                        (
                            annotation.pattern().and_then(|e| node_name(e.syntax())),
                            annotation.ty().map(|e| lower(e.syntax())),
                        )
                    } else {
                        (node_name(&pattern), None)
                    };
                let Some(name) = name else {
                    self.unsupported(node, &source);
                    return;
                };
                draft.name = name;
                draft.declaration = DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Constant,
                    declaration: JuliaDeclaration::Constant,
                    signature: Some(Signature::Value { annotation, value }),
                };
                self.declarations.drafts.push(draft);
            }
            K::EXPORT_STMT | K::PUBLIC_STMT => self.exports(node, module, file_source),
            K::IMPORT_STMT | K::USING_STMT => self.imports(node, module, file_source),
            K::CALL_EXPR => {
                let Some(call) = CallExpr::cast(node.clone()) else {
                    return;
                };
                let callee = call.callee().and_then(|e| node_name(e.syntax()));
                if callee.as_deref() == Some("include") {
                    self.includes.push((module.into(), source.clone()));
                    let argument = call.arg_list().and_then(|list| {
                        let nodes: Vec<_> = list.syntax().children().collect();
                        (nodes.len() == 1)
                            .then(|| nodes[0].children().next())
                            .flatten()
                    });
                    let literal = argument.and_then(StringLiteral::cast).filter(|literal| {
                        literal.prefix().is_none()
                            && literal.suffix().is_none()
                            && literal.interpolations().next().is_none()
                    });
                    let path = literal.and_then(|literal| {
                        fatou_parser::parser::string_value(
                            &literal
                                .content_tokens()
                                .map(|token| token.text().to_owned())
                                .collect::<String>(),
                        )
                        .ok()
                    });
                    if let Some(path) = path {
                        let included = file.parent().unwrap().join(path);
                        if included.extension().is_none_or(|e| e != "jl") {
                            self.error(
                                Code::JuliaInclude,
                                "Julia includes must name maintained .jl source files.",
                                &source,
                            );
                        } else {
                            self.file(&included, module, Some(&source));
                        }
                    } else {
                        self.error(Code::JuliaInclude, "Only unconditional include calls with one ordinary literal path are supported.", &source);
                    }
                } else if callee.as_deref().is_some_and(|name| {
                    matches!(
                        name,
                        "eval" | "Core.eval" | "Base.include" | "include_string"
                    )
                }) {
                    self.unsupported(node, &source);
                } else if draft.documented {
                    if let Some(signature) = signatures::parse(node) {
                        draft.name = signature.name;
                        draft.declaration = DraftDeclaration::Documentation {
                            selector: Some(signature.dispatch),
                        };
                        self.declarations.drafts.push(draft);
                    } else {
                        self.unsupported(node, &source);
                    }
                }
            }
            K::MACRO_CALL => {
                let macro_name = node
                    .children()
                    .find(|n| n.kind() == K::MACRO_NAME)
                    .map(|n| n.text().to_string());
                let transparent = macro_name.as_deref().is_some_and(|name| {
                    matches!(
                        name,
                        "@inline"
                            | "@noinline"
                            | "@propagate_inbounds"
                            | "Base.@inline"
                            | "Base.@noinline"
                            | "Base.@propagate_inbounds"
                    )
                });
                let arguments: Vec<_> = node
                    .children()
                    .filter(|n| n.kind() != K::MACRO_NAME)
                    .flat_map(|n| {
                        if n.kind() == K::ARG_LIST {
                            n.children()
                                .map(|arg| {
                                    if arg.kind() == K::ARG {
                                        arg.children().next().unwrap_or(arg.clone())
                                    } else {
                                        arg
                                    }
                                })
                                .collect()
                        } else {
                            vec![n]
                        }
                    })
                    .collect();
                let inner = (arguments.len() == 1).then(|| arguments[0].clone());
                if transparent && let Some(inner) = inner {
                    draft.wrappers.push(source);
                    self.walk(
                        &inner,
                        file_source,
                        file,
                        module,
                        draft.documentation,
                        draft.documented,
                        draft.wrappers,
                    );
                } else if draft.documented
                    || contains_surface(node)
                    || macro_name.as_deref().is_some_and(|name| {
                        matches!(name, "@eval" | "@generated" | "@reexport" | "@doc")
                    })
                {
                    self.unsupported(node, &source);
                }
            }
            K::BEGIN_EXPR | K::TYPEGROUP_DEF => {
                if let Some(block) = body_of(node) {
                    for child in block.children() {
                        self.walk(&child, file_source, file, module, None, false, Vec::new());
                    }
                }
            }
            K::NAME | K::BINARY_EXPR if draft.documented => {
                if let Some(name) = node_name(node) {
                    draft.name = name;
                    draft.declaration = DraftDeclaration::Documentation { selector: None };
                    self.declarations.drafts.push(draft);
                } else {
                    self.unsupported(node, &source);
                }
            }
            K::IF_EXPR | K::FOR_EXPR | K::WHILE_EXPR | K::LET_EXPR | K::TRY_EXPR => {
                if contains_surface(node) {
                    self.unsupported(node, &source);
                }
            }
            _ if draft.documented => self.unsupported(node, &source),
            _ => {}
        }
    }

    fn field(
        &mut self,
        original: &SyntaxNode,
        file_source: &SourceLocation,
        file: &Path,
        module: &str,
        owner: &str,
    ) {
        let (node, documentation, documented) =
            if let Some(attachment) = DocAttachment::cast(original.clone()) {
                let source = located(file_source, attachment.payload().text_range());
                let doc = match attachment.text() {
                    DocText::Static(text) => {
                        let parsed = super::docstrings::parse(&text, source);
                        self.result.diagnostics.extend(parsed.diagnostics);
                        Some(parsed.document)
                    }
                    _ => {
                        self.result.diagnostics.push(diagnostic(
                            Code::JuliaUnsupportedDocstring,
                            Severity::Warning,
                            "Field documentation cannot be decoded statically.",
                            &source,
                        ));
                        Some(super::docstrings::opaque(
                            attachment.payload().text().to_string(),
                            source,
                        ))
                    }
                };
                (attachment.target().clone(), doc, true)
            } else {
                (original.clone(), None, false)
            };
        let mut field_node = node.clone();
        let is_const = field_node.kind() == K::CONST_STMT;
        if is_const && let Some(child) = field_node.children().next() {
            field_node = child;
        }
        let annotation = TypeAnnotation::cast(field_node.clone());
        let name = if let Some(annotation) = &annotation {
            annotation.pattern().and_then(|e| node_name(e.syntax()))
        } else if field_node.kind() == K::NAME {
            node_name(&field_node)
        } else {
            None
        };
        if let Some(name) = name {
            self.declarations.drafts.push(Draft {
                module: module.into(),
                name: format!("{owner}.{name}"),
                declaration: DraftDeclaration::Named {
                    kind: JuliaIdentityKind::Field,
                    declaration: JuliaDeclaration::Field { is_const },
                    signature: Some(Signature::Value {
                        annotation: annotation.and_then(|a| a.ty()).map(|e| lower(e.syntax())),
                        value: None,
                    }),
                },
                documentation,
                documented,
                source: located(file_source, node.text_range()),
                wrappers: Vec::new(),
            });
        } else {
            self.walk(
                &node,
                file_source,
                file,
                module,
                documentation,
                documented,
                Vec::new(),
            );
        }
    }

    fn exports(&mut self, node: &SyntaxNode, module: &str, file_source: &SourceLocation) {
        let visibility = if node.kind() == K::EXPORT_STMT {
            JuliaVisibility::Exported
        } else {
            JuliaVisibility::Public
        };
        for element in node.children_with_tokens() {
            if node.kind() == K::PUBLIC_STMT
                && element.text_range().start() == node.text_range().start()
            {
                continue;
            }
            let name = match &element {
                rowan::NodeOrToken::Token(token)
                    if token.kind() == K::IDENT || token.kind().is_operator() =>
                {
                    Some(token.text().to_string())
                }
                rowan::NodeOrToken::Node(child) if child.kind() == K::MACRO_NAME => {
                    Some(child.text().to_string().trim().to_owned())
                }
                rowan::NodeOrToken::Node(child) => node_name(child),
                _ => None,
            };
            if let Some(name) = name {
                self.declarations.exports.push(Export {
                    module: module.into(),
                    name,
                    visibility,
                    source: located(file_source, element.text_range()),
                });
            }
        }
    }

    fn imports(&mut self, node: &SyntaxNode, module: &str, file_source: &SourceLocation) {
        let colon = node
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == K::COLON)
            .map(|t| t.text_range().start());
        let clauses: Vec<_> = node.children().collect();
        let base = if colon.is_some() {
            clauses
                .first()
                .and_then(import_path)
                .and_then(|path| absolute_module(module, &path))
        } else {
            None
        };
        for clause in &clauses {
            if colon.is_some_and(|at| clause.text_range().start() < at) {
                continue;
            }
            let Some(path) = import_path(clause) else {
                self.unsupported(node, &located(file_source, node.text_range()));
                continue;
            };
            let target = if let Some(base) = &base {
                qualify(base, &path)
            } else {
                let Some(path) = absolute_module(module, &path) else {
                    self.unsupported(node, &located(file_source, node.text_range()));
                    continue;
                };
                path
            };
            let alias = if clause.kind() == K::IMPORT_ALIAS {
                clause
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| t.kind() == K::IDENT && t.text() != "as")
                    .last()
                    .map(|t| t.text().to_string())
            } else {
                None
            };
            let name = alias.unwrap_or_else(|| target.rsplit('.').next().unwrap().into());
            self.declarations.imports.push(Import {
                module: module.into(),
                name,
                target,
                source: located(file_source, clause.text_range()),
                whole_using: node.kind() == K::USING_STMT && colon.is_none(),
            });
        }
    }

    fn error(&mut self, code: Code, message: impl Into<String>, source: &SourceLocation) {
        self.result
            .diagnostics
            .push(diagnostic(code, Severity::Error, message, source));
    }

    fn unsupported(&mut self, node: &SyntaxNode, source: &SourceLocation) {
        self.error(
            Code::JuliaUnsupportedSurface,
            format!(
                "Julia {:?} cannot establish an API declaration without evaluation.",
                node.kind()
            ),
            source,
        );
    }
}

pub(super) fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.into()
    } else {
        format!("{module}.{name}")
    }
}

fn header(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children()
        .find(|n| n.kind() == K::SIGNATURE)?
        .children()
        .next()
}

fn has_token(node: &SyntaxNode, kind: K) -> bool {
    node.children_with_tokens()
        .any(|element| element.kind() == kind)
}

fn contains_surface(node: &SyntaxNode) -> bool {
    node.descendants().any(|n| {
        matches!(
            n.kind(),
            K::FUNCTION_DEF
                | K::MACRO_DEF
                | K::MODULE_DEF
                | K::STRUCT_DEF
                | K::ABSTRACT_DEF
                | K::PRIMITIVE_DEF
                | K::EXPORT_STMT
                | K::PUBLIC_STMT
                | K::IMPORT_STMT
                | K::USING_STMT
                | K::CONST_STMT
        ) || n.kind() == K::ASSIGNMENT_EXPR
            && n.children()
                .next()
                .is_some_and(|lhs| signatures::parse(&lhs).is_some())
            || n.kind() == K::CALL_EXPR
                && CallExpr::cast(n)
                    .and_then(|c| c.callee())
                    .and_then(|e| node_name(e.syntax()))
                    .as_deref()
                    == Some("include")
    })
}

fn import_path(node: &SyntaxNode) -> Option<String> {
    let node = if node.kind() == K::IMPORT_ALIAS {
        node.children().next()?
    } else {
        node.clone()
    };
    (node.kind() == K::IMPORT_PATH).then(|| node.text().to_string().trim().to_owned())
}

fn absolute_module(module: &str, path: &str) -> Option<String> {
    let dots = path.chars().take_while(|c| *c == '.').count();
    if dots == 0 {
        return Some(path.into());
    }
    let mut parts: Vec<_> = module.split('.').filter(|s| !s.is_empty()).collect();
    for _ in 1..dots {
        parts.pop()?;
    }
    if parts.is_empty() {
        return None;
    }
    let prefix = parts.join(".");
    let suffix = &path[dots..];
    Some(if suffix.is_empty() {
        prefix
    } else {
        qualify(&prefix, suffix)
    })
}
