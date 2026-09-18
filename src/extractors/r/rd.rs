use std::collections::{BTreeMap, BTreeSet};

use arity_parser::{
    ast::{AstNode, Expr, HasArgList},
    parser,
};
use rd_ast::{RdListItem, RdListKind, RdNode, RdPath, RdPathSegment, RdShapeError, RdTag};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    Attributes, Block, Document, DocumentFormat, Inline, Item, ItemAlias, ItemAliasKind,
    ItemLanguageData, ListItem, Parameter, ParameterKind, RDeclaration, RGenericReference,
    Signature, SignatureExpression, SourceEvidence, SourceLocation, SourceRole, SourceSpan,
    SourcedDocument,
};

use super::{diagnostic, located, provenance, source};

pub(super) struct Topic {
    aliases: Vec<String>,
    parts: Vec<Part>,
    usages: Vec<Usage>,
    source: SourceLocation,
    raw: String,
    valid: bool,
    pub(super) uses_r: bool,
}

enum Part {
    Blocks(Vec<Block>),
    Usage,
    Arguments(Vec<(Vec<String>, ListItem)>),
}

struct Usage {
    name: String,
    method: Option<(String, String)>,
    arguments: String,
    parameters: Vec<Parameter>,
}

struct Context<'a> {
    raw: &'a str,
    source: &'a SourceLocation,
    diagnostics: &'a mut Vec<Diagnostic>,
    uses_r: bool,
}

pub(super) fn parse(
    text: &str,
    source: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Topic> {
    let parsed = match rd_source::parse(text.as_bytes()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let span = match &error {
                rd_source::ParseError::NulByte { offset } => Some(*offset..offset + 1),
                rd_source::ParseError::UnsupportedEncoding {
                    span: Some(span), ..
                }
                | rd_source::ParseError::NestingLimitExceeded { span } => Some(span.bytes()),
                _ => None,
            };
            let source = span
                .map(|s| located(source, s.start, s.end))
                .unwrap_or_else(|| source.clone());
            diagnostics.push(diagnostic(
                DiagnosticCode::RRdSyntax,
                Severity::Error,
                error.to_string(),
                &source,
            ));
            return None;
        }
    };
    for report in parsed.diagnostics() {
        let range = report.span().bytes();
        diagnostics.push(diagnostic(
            DiagnosticCode::RRdSyntax,
            if matches!(report.severity(), rd_source::Severity::Warning) {
                Severity::Warning
            } else {
                Severity::Error
            },
            report.message(),
            &located(source, range.start, range.end),
        ));
    }
    diagnostics.push(diagnostic(
        DiagnosticCode::RRdSourceAttribution,
        Severity::Warning,
        "Rd nodes have file-level source attribution; exact node byte ranges are unavailable.",
        source,
    ));
    let document = parsed.document();
    let mut context = Context {
        raw: text,
        source,
        diagnostics,
        uses_r: false,
    };
    let mut valid = !parsed
        .diagnostics()
        .iter()
        .any(|d| matches!(d.severity(), rd_source::Severity::Error));
    // Strict views reject duplicate sections instead of selecting the first one.
    for view in [
        document.inspect_name(),
        document.inspect_title(),
        document.inspect_usage(),
        document.inspect_description(),
        document.inspect_details(),
        document.inspect_value(),
        document.inspect_references(),
        document.inspect_examples(),
        document.inspect_note(),
        document.inspect_see_also(),
        document.inspect_author(),
    ] {
        if let Err(error) = view {
            context.shape(error);
            valid = false;
        }
    }
    if document
        .inspect_name()
        .ok()
        .flatten()
        .and_then(plain)
        .is_none_or(|name| name.trim().is_empty())
    {
        context.error(
            DiagnosticCode::RRdInformationLoss,
            "Rd requires a static topic name.",
        );
        valid = false;
    }
    let mut aliases = vec![];
    for alias in document.inspect_aliases() {
        match alias {
            Ok(alias) => {
                if let Some(name) = plain(alias.nodes()).filter(|name| !name.trim().is_empty()) {
                    aliases.push(name.trim().into());
                } else {
                    context.error(
                        DiagnosticCode::RRdInformationLoss,
                        "Rd alias is not a static name.",
                    );
                    valid = false;
                }
            }
            Err(error) => {
                context.shape(error);
                valid = false;
            }
        }
    }
    let mut arguments = vec![];
    match document.inspect_arguments() {
        Ok(entries) => {
            for argument in entries {
                match argument {
                    Ok(argument) => {
                        let Some(names) = plain(argument.name) else {
                            context.error(
                                DiagnosticCode::RRdInformationLoss,
                                "Rd argument names must be static text.",
                            );
                            valid = false;
                            continue;
                        };
                        let mut blocks = vec![Block::Paragraph {
                            inlines: vec![Inline::Code {
                                value: names.trim().into(),
                                span: context.span(),
                            }],
                            span: context.span(),
                        }];
                        blocks.extend(context.blocks(argument.description, &RdPath::new(vec![])));
                        arguments.push((
                            names
                                .split(',')
                                .map(|name| name.trim().trim_matches('`').to_owned())
                                .collect(),
                            ListItem {
                                checked: None,
                                blocks,
                                span: context.span(),
                            },
                        ));
                    }
                    Err(error) => {
                        context.shape(error);
                        valid = false;
                    }
                }
            }
        }
        Err(error) => {
            context.shape(error);
            valid = false;
        }
    }
    let mut parts = vec![];
    let mut usages = vec![];
    for (index, node) in document.nodes().iter().enumerate() {
        let path = RdPath::new(vec![RdPathSegment::TopLevel(index)]);
        let RdNode::Tagged(tagged) = node else {
            if !matches!(node, RdNode::Comment(_))
                && plain(std::slice::from_ref(node)).is_none_or(|s| !s.trim().is_empty())
            {
                parts.push(Part::Blocks(
                    context.blocks(std::slice::from_ref(node), &path),
                ));
            }
            continue;
        };
        match tagged.tag() {
            RdTag::Name | RdTag::Alias => {}
            RdTag::Usage => {
                let parsed = context.usages(tagged.children(), &path);
                if parsed.is_empty() && !tagged.children().is_empty() {
                    parts.push(Part::Blocks(vec![
                        context.unsupported_block("usage", &path),
                    ]));
                } else {
                    usages.extend(parsed);
                    parts.push(Part::Usage);
                }
            }
            RdTag::Arguments => parts.push(Part::Arguments(arguments.clone())),
            RdTag::Title => parts.push(Part::Blocks(vec![Block::Heading {
                level: 1,
                attributes: Attributes::default(),
                inlines: context.inlines(tagged.children(), &path),
                span: context.span(),
            }])),
            RdTag::Description => {
                parts.push(Part::Blocks(context.blocks(tagged.children(), &path)))
            }
            RdTag::Details
            | RdTag::Value
            | RdTag::References
            | RdTag::Note
            | RdTag::SeeAlso
            | RdTag::Author
            | RdTag::Keyword
            | RdTag::Concept => {
                let title = match tagged.tag() {
                    RdTag::Details => "Details",
                    RdTag::Value => "Value",
                    RdTag::References => "References",
                    RdTag::Note => "Notes",
                    RdTag::SeeAlso => "See also",
                    RdTag::Author => "Author",
                    RdTag::Keyword => "Keywords",
                    _ => "Concepts",
                };
                let mut blocks = vec![heading(title, context.span())];
                blocks.extend(context.blocks(tagged.children(), &path));
                parts.push(Part::Blocks(blocks));
            }
            RdTag::Examples => {
                let mut blocks = vec![heading("Examples", context.span())];
                match context.code(tagged.children(), &path, &mut BTreeMap::new()) {
                    Some(code) => blocks.push(code_block(code.trim().into(), context.span())),
                    None => blocks.push(context.unsupported_block("examples", &path)),
                }
                parts.push(Part::Blocks(blocks));
            }
            _ => parts.push(Part::Blocks(
                context.blocks(std::slice::from_ref(node), &path),
            )),
        }
    }
    Some(Topic {
        aliases,
        parts,
        usages,
        source: source.clone(),
        raw: text.into(),
        valid,
        uses_r: context.uses_r,
    })
}

impl Context<'_> {
    fn span(&self) -> SourceSpan {
        SourceSpan {
            start: 0,
            end: self.raw.len(),
        }
    }

    fn error(&mut self, code: DiagnosticCode, message: impl Into<String>) {
        self.diagnostics
            .push(diagnostic(code, Severity::Error, message, self.source));
    }

    fn shape(&mut self, error: RdShapeError) {
        self.error(DiagnosticCode::RRdInformationLoss, error.to_string());
    }

    fn unsupported(&mut self, kind: &str, path: &RdPath) {
        self.diagnostics.push(diagnostic(
            DiagnosticCode::UnsupportedRd,
            Severity::Warning,
            format!("Unsupported Rd `{kind}` at {path}; content remains unevaluated."),
            self.source,
        ));
    }

    fn unsupported_block(&mut self, kind: &str, path: &RdPath) -> Block {
        self.unsupported(kind, path);
        Block::Unsupported {
            source_kind: format!("rd-{kind}"),
            raw: self.raw.into(),
            span: self.span(),
        }
    }

    fn blocks(&mut self, nodes: &[RdNode], path: &RdPath) -> Vec<Block> {
        let mut result = vec![];
        let mut paragraph = vec![];
        let mut line_has_content = false;
        for (index, node) in nodes.iter().enumerate() {
            let child_path = path.with_child(index);
            if let RdNode::Text(text) = node {
                let text = text.replace("\r\n", "\n");
                // Rd can split a blank line across adjacent text nodes.
                for piece in text.split_inclusive('\n') {
                    let ends_line = piece.ends_with('\n');
                    if ends_line && !line_has_content && piece.trim().is_empty() {
                        flush(&mut result, &mut paragraph, self.span());
                    } else {
                        paragraph.push(Inline::Text {
                            value: piece.into(),
                            span: self.span(),
                        });
                    }
                    line_has_content = !ends_line && (line_has_content || !piece.trim().is_empty());
                }
                continue;
            }
            if let RdNode::Tagged(tagged) = node {
                match tagged.tag() {
                    RdTag::Itemize | RdTag::Enumerate | RdTag::Describe => {
                        flush(&mut result, &mut paragraph, self.span());
                        line_has_content = false;
                        match tagged.inspect_list(&child_path) {
                            Ok(list) => {
                                let mut items = vec![];
                                for entry in list.items() {
                                    let blocks = match entry {
                                        Ok(RdListItem::Delimited(item)) => {
                                            self.blocks(item.body(), item.path())
                                        }
                                        Ok(RdListItem::Described(item)) => {
                                            let mut blocks = vec![Block::Paragraph {
                                                inlines: self.inlines(item.label(), item.path()),
                                                span: self.span(),
                                            }];
                                            blocks.extend(self.blocks(item.body(), item.path()));
                                            blocks
                                        }
                                        Err(error) => {
                                            self.shape(error);
                                            vec![self.unsupported_block("list-item", &child_path)]
                                        }
                                        _ => vec![self.unsupported_block("list-item", &child_path)],
                                    };
                                    items.push(ListItem {
                                        checked: None,
                                        blocks,
                                        span: self.span(),
                                    });
                                }
                                result.push(Block::List {
                                    ordered: list.kind() == RdListKind::Enumerate,
                                    items,
                                    span: self.span(),
                                });
                            }
                            Err(error) => {
                                self.shape(error);
                                result.push(self.unsupported_block("list", &child_path));
                            }
                        }
                        continue;
                    }
                    RdTag::Preformatted => {
                        flush(&mut result, &mut paragraph, self.span());
                        line_has_content = false;
                        if let Some(code) = plain(tagged.children()) {
                            result.push(Block::CodeBlock {
                                language: None,
                                source: code,
                                source_segments: vec![],
                                span: self.span(),
                            });
                        } else {
                            result.push(self.unsupported_block("preformatted", &child_path));
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            let inlines = self.inline_node(node, &child_path);
            line_has_content |= !inlines.is_empty();
            paragraph.extend(inlines);
        }
        flush(&mut result, &mut paragraph, self.span());
        result
    }

    fn inlines(&mut self, nodes: &[RdNode], path: &RdPath) -> Vec<Inline> {
        nodes
            .iter()
            .enumerate()
            .flat_map(|(index, node)| self.inline_node(node, &path.with_child(index)))
            .collect()
    }

    fn inline_node(&mut self, node: &RdNode, path: &RdPath) -> Vec<Inline> {
        let mut result = vec![];
        let span = self.span();
        match node {
            RdNode::Text(value) | RdNode::RCode(value) | RdNode::Verb(value) => {
                result.push(Inline::Text {
                    value: value.clone(),
                    span,
                })
            }
            RdNode::Comment(_) => {}
            RdNode::Group(group) => result.extend(self.inlines(group.children(), path)),
            RdNode::Tagged(tagged) => match tagged.tag() {
                RdTag::Emph | RdTag::Var | RdTag::Dfn => result.push(Inline::Emphasis {
                    inlines: self.inlines(tagged.children(), path),
                    span,
                }),
                RdTag::Strong | RdTag::Bold => result.push(Inline::Strong {
                    inlines: self.inlines(tagged.children(), path),
                    span,
                }),
                RdTag::Code
                | RdTag::Verb
                | RdTag::File
                | RdTag::Pkg
                | RdTag::Samp
                | RdTag::Kbd
                | RdTag::Env
                | RdTag::Command
                | RdTag::Option => {
                    if let Some(value) = plain(tagged.children()) {
                        result.push(Inline::Code { value, span });
                    } else {
                        result.extend(self.inlines(tagged.children(), path));
                    }
                }
                RdTag::SQuote | RdTag::DQuote => {
                    let (open, close) = if tagged.tag() == &RdTag::SQuote {
                        ("‘", "’")
                    } else {
                        ("“", "”")
                    };
                    result.push(Inline::Text {
                        value: open.into(),
                        span,
                    });
                    result.extend(self.inlines(tagged.children(), path));
                    result.push(Inline::Text {
                        value: close.into(),
                        span,
                    });
                }
                RdTag::R | RdTag::Dots | RdTag::LDots => result.push(Inline::Text {
                    value: if tagged.tag() == &RdTag::R {
                        "R"
                    } else {
                        "..."
                    }
                    .into(),
                    span,
                }),
                RdTag::Cr => result.push(Inline::HardBreak { span }),
                RdTag::Url | RdTag::Email => {
                    if let Some(target) = plain(tagged.children()) {
                        result.push(Inline::Link {
                            inlines: vec![Inline::Text {
                                value: target.clone(),
                                span,
                            }],
                            target: if tagged.tag() == &RdTag::Email {
                                format!("mailto:{target}")
                            } else {
                                target
                            },
                            title: None,
                            attributes: Attributes::default(),
                            span,
                        });
                    } else {
                        result.push(self.unsupported_inline(tagged.tag().as_rd_tag(), path));
                    }
                }
                RdTag::Href => match tagged.inspect_href(path) {
                    Ok(link) => {
                        if let Some(target) = plain(link.url()) {
                            result.push(Inline::Link {
                                inlines: self.inlines(link.display(), path),
                                target,
                                title: None,
                                attributes: Attributes::default(),
                                span,
                            });
                        } else {
                            result.push(self.unsupported_inline("href", path));
                        }
                    }
                    Err(error) => {
                        self.shape(error);
                        result.push(self.unsupported_inline("href", path));
                    }
                },
                RdTag::Link if tagged.option().is_none() => {
                    if let Some(target) = plain(tagged.children()) {
                        result.push(Inline::SemanticReference {
                            target,
                            target_span: span,
                            span,
                        });
                    } else {
                        result.push(self.unsupported_inline("link", path));
                    }
                }
                _ => result.push(self.unsupported_inline(tagged.tag().as_rd_tag(), path)),
            },
            _ => result.push(self.unsupported_inline("node", path)),
        }
        result
    }

    fn unsupported_inline(&mut self, kind: &str, path: &RdPath) -> Inline {
        self.unsupported(kind, path);
        Inline::Unsupported {
            source_kind: format!("rd-{kind}"),
            raw: self.raw.into(),
            span: self.span(),
        }
    }

    fn code(
        &mut self,
        nodes: &[RdNode],
        path: &RdPath,
        methods: &mut BTreeMap<String, (String, String)>,
    ) -> Option<String> {
        let mut result = String::new();
        for (index, node) in nodes.iter().enumerate() {
            let path = path.with_child(index);
            match node {
                RdNode::Text(text) | RdNode::RCode(text) | RdNode::Verb(text) => {
                    result.push_str(text)
                }
                RdNode::Comment(_) => {}
                RdNode::Group(group) => {
                    result.push_str(&self.code(group.children(), &path, methods)?)
                }
                RdNode::Tagged(tagged) => match tagged.tag() {
                    RdTag::Method | RdTag::S3Method => match node.inspect_method(&path) {
                        Ok(Some(method)) => {
                            let name = format!("{}.{}", method.generic(), method.qualifier());
                            methods.insert(
                                name.clone(),
                                (method.generic().into(), method.qualifier().into()),
                            );
                            result.push_str(&r_name(&name));
                        }
                        Err(error) => {
                            self.shape(error);
                            return None;
                        }
                        _ => return None,
                    },
                    RdTag::Dots | RdTag::LDots => result.push_str("..."),
                    RdTag::Code | RdTag::DontRun | RdTag::DontTest | RdTag::DontDiff => {
                        if tagged.tag() != &RdTag::Code {
                            result.push_str(&format!("\n# {}\n", tagged.tag().as_rd_tag()));
                        }
                        result.push_str(&self.code(tagged.children(), &path, methods)?);
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
        Some(result)
    }

    fn usages(&mut self, nodes: &[RdNode], path: &RdPath) -> Vec<Usage> {
        let mut methods = BTreeMap::new();
        let Some(code) = self.code(nodes, path, &mut methods) else {
            self.unsupported("usage", path);
            return vec![];
        };
        self.uses_r = true;
        let parsed = parser::parse(&code);
        if !parsed.diagnostics.is_empty() {
            self.error(
                DiagnosticCode::RRdInformationLoss,
                "Rd usage is not supported R call syntax.",
            );
            return vec![];
        }
        let mut result = vec![];
        for expr in parsed.cst.children_with_tokens().filter_map(Expr::cast) {
            let Expr::Call(call) = expr else {
                self.error(
                    DiagnosticCode::RRdInformationLoss,
                    "Rd usage requires a named call.",
                );
                continue;
            };
            let Some(name) = source::callee(&call) else {
                self.error(
                    DiagnosticCode::RRdInformationLoss,
                    "Rd usage requires a static callable name.",
                );
                continue;
            };
            let mut parameters = vec![];
            let mut after_dots = false;
            let mut valid = true;
            for arg in call.args() {
                let Some(value) = arg.value() else {
                    valid = false;
                    continue;
                };
                let (parameter, default) = if let Some(name) = arg.name() {
                    (Some(name.to_string()), Some(source::expression(value)))
                } else {
                    (
                        Expr::cast(value).as_ref().and_then(source::static_name),
                        None,
                    )
                };
                let Some(name) = parameter else {
                    valid = false;
                    continue;
                };
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
                parameters.push(Parameter {
                    name,
                    kind,
                    default,
                    annotation: None,
                });
            }
            if !valid {
                self.error(
                    DiagnosticCode::RRdInformationLoss,
                    "Rd usage arguments must name formals or their defaults.",
                );
                continue;
            }
            let range = call.arg_list().unwrap().syntax().text_range();
            let arguments = code[usize::from(range.start())..usize::from(range.end())].to_owned();
            result.push(Usage {
                method: methods.get(&name).cloned(),
                name,
                arguments,
                parameters,
            });
        }
        result
    }
}

pub(super) fn attach(
    items: &mut BTreeMap<String, Item>,
    topics: &[Topic],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut names = BTreeMap::<String, String>::new();
    for (id, item) in items.iter() {
        names.insert(item.qualified_name.clone(), id.clone());
        for alias in &item.aliases {
            names.insert(alias.qualified_name.clone(), id.clone());
        }
    }
    let mut candidates = BTreeMap::<String, Vec<(usize, Vec<String>)>>::new();
    for (index, topic) in topics.iter().enumerate().filter(|(_, topic)| topic.valid) {
        for usage in &topic.usages {
            if !items
                .iter()
                .any(|(id, item)| usage_matches(usage, item, &names, id))
            {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RUnresolvedDefinition,
                    Severity::Error,
                    format!(
                        "Rd usage `{}` has no maintained public declaration.",
                        usage.name
                    ),
                    &topic.source,
                ));
            }
        }
        let mut targets = BTreeMap::<String, Vec<String>>::new();
        let mut unknown = vec![];
        for alias in &topic.aliases {
            if let Some(id) = names.get(alias) {
                targets.entry(id.clone()).or_default().push(alias.clone());
            } else {
                unknown.push(alias.clone());
            }
        }
        if !unknown.is_empty() {
            if targets.len() == 1 {
                targets.values_mut().next().unwrap().extend(unknown);
            } else {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RUnresolvedDefinition,
                    Severity::Error,
                    format!(
                        "Rd aliases have no unambiguous maintained target: {}.",
                        unknown.join(", ")
                    ),
                    &topic.source,
                ));
            }
        }
        let parameters: BTreeSet<_> = targets
            .keys()
            .flat_map(|id| {
                let Signature::Callable { parameters, .. } = &items[id].signatures[0].signature
                else {
                    unreachable!()
                };
                parameters.iter().map(|parameter| parameter.name.as_str())
            })
            .collect();
        for part in &topic.parts {
            if let Part::Arguments(arguments) = part {
                for (names, _) in arguments {
                    for name in names {
                        if !parameters.contains(name.as_str()) {
                            diagnostics.push(diagnostic(
                                DiagnosticCode::RRdInformationLoss,
                                Severity::Error,
                                format!("Rd argument `{name}` does not match any documented declaration's formals; its raw text is retained."),
                                &topic.source,
                            ));
                        }
                    }
                }
            }
        }
        for (id, aliases) in targets {
            candidates.entry(id).or_default().push((index, aliases));
        }
    }
    let mut alias_targets = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, id) in &names {
        alias_targets
            .entry(name.clone())
            .or_default()
            .insert(id.clone());
    }
    for (id, candidates) in &candidates {
        for (_, aliases) in candidates {
            for alias in aliases {
                alias_targets
                    .entry(alias.clone())
                    .or_default()
                    .insert(id.clone());
            }
        }
    }
    for (id, item) in items {
        let Some(candidates) = candidates.get(id) else {
            if let Some(source) = &item.source_location {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RMissingDocumentedAlias,
                    Severity::Warning,
                    format!("No valid Rd alias documents `{}`.", item.qualified_name),
                    source,
                ));
            }
            continue;
        };
        if candidates.len() != 1 {
            for (index, _) in candidates {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RConflictingSurface,
                    Severity::Error,
                    format!("Multiple Rd topics document `{}`.", item.qualified_name),
                    &topics[*index].source,
                ));
            }
            continue;
        }
        let (index, aliases) = &candidates[0];
        let topic = &topics[*index];
        for alias in aliases {
            if alias_targets[alias].len() != 1 {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RConflictingSurface,
                    Severity::Error,
                    format!("Rd alias `{alias}` refers to multiple canonical items."),
                    &topic.source,
                ));
                continue;
            }
            if !item
                .aliases
                .iter()
                .any(|a| a.qualified_name == *alias && a.kind == ItemAliasKind::RdAlias)
            {
                item.aliases.push(ItemAlias {
                    qualified_name: alias.clone(),
                    kind: ItemAliasKind::RdAlias,
                    sources: vec![SourceEvidence {
                        source: topic.source.clone(),
                        role: SourceRole::Documentation,
                        parsers: BTreeSet::from(["rd-source".into(), "rd-ast".into()]),
                    }],
                });
            }
        }
        let span = SourceSpan {
            start: 0,
            end: topic.raw.len(),
        };
        let usages: Vec<_> = topic
            .usages
            .iter()
            .filter(|usage| usage_matches(usage, item, &names, id))
            .collect();
        let Signature::Callable { parameters, .. } = &item.signatures[0].signature else {
            unreachable!()
        };
        if !topic.usages.is_empty() && usages.is_empty() {
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                format!(
                    "Rd topic has no matching usage for `{}`.",
                    item.qualified_name
                ),
                &topic.source,
            ));
        }
        for usage in &usages {
            if !same_parameters(parameters, &usage.parameters) {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RConflictingSurface,
                    Severity::Error,
                    format!(
                        "Rd usage disagrees with maintained formals for `{}`.",
                        item.qualified_name
                    ),
                    &topic.source,
                ));
            }
        }
        let mut blocks = vec![];
        for part in &topic.parts {
            match part {
                Part::Blocks(content) => blocks.extend(content.clone()),
                Part::Usage if !usages.is_empty() => {
                    blocks.push(heading("Usage", span));
                    for usage in &usages {
                        blocks.push(code_block(
                            format!("{}({})", r_name(&item.qualified_name), usage.arguments),
                            span,
                        ));
                    }
                }
                Part::Usage => {}
                Part::Arguments(arguments) => {
                    let entries: Vec<_> = arguments
                        .iter()
                        .filter_map(|(names, entry)| {
                            let selected: Vec<_> = names
                                .iter()
                                .filter(|name| {
                                    parameters.iter().any(|parameter| parameter.name == **name)
                                })
                                .map(|name| r_name(name))
                                .collect();
                            if selected.is_empty() {
                                return None;
                            }
                            let mut entry = entry.clone();
                            if selected.len() != names.len() {
                                let Block::Paragraph { inlines, .. } = &mut entry.blocks[0] else {
                                    unreachable!()
                                };
                                inlines[0] = Inline::Code {
                                    value: selected.join(", "),
                                    span,
                                };
                            }
                            Some(entry)
                        })
                        .collect();
                    if !entries.is_empty() {
                        blocks.push(heading("Arguments", span));
                        blocks.push(Block::List {
                            ordered: false,
                            items: entries,
                            span,
                        });
                    }
                }
            }
        }
        let mut document_provenance = provenance(&topic.source, true);
        if topic.uses_r {
            document_provenance
                .tools
                .insert("arity-parser".into(), "0.6.0".into());
        }
        item.documentation = Some(SourcedDocument {
            document: Document {
                span,
                frontmatter: None,
                blocks,
            },
            source_format: DocumentFormat::Extracted { name: "rd".into() },
            source_location: Some(topic.source.clone()),
            raw_source: Some(topic.raw.clone()),
            provenance: vec![document_provenance],
        });
    }
}

fn usage_matches(usage: &Usage, item: &Item, names: &BTreeMap<String, String>, id: &str) -> bool {
    if let Some((generic, class)) = &usage.method {
        if let Some(ItemLanguageData::R(data)) = &item.language_data
            && let RDeclaration::S3Method {
                generic: reference,
                class: declared,
                ..
            } = &data.declaration
        {
            return class == declared
                && match reference {
                    RGenericReference::External { name, .. } => name == generic,
                    RGenericReference::Workspace { item } => names.get(generic) == Some(&item.item),
                };
        }
        return false;
    }
    names.get(&usage.name).is_some_and(|target| target == id)
}

fn same_parameters(left: &[Parameter], right: &[Parameter]) -> bool {
    let normalize = |parameters: &[Parameter]| {
        parameters
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.kind.clone(),
                    p.default.clone().map(normalize_expression),
                )
            })
            .collect::<Vec<_>>()
    };
    normalize(left) == normalize(right)
}

fn normalize_expression(mut expression: SignatureExpression) -> SignatureExpression {
    match &mut expression {
        SignatureExpression::Literal { text } => {
            let parsed = parser::parse(text);
            if let Some(Expr::StringLiteral(string)) =
                parsed.cst.children_with_tokens().find_map(Expr::cast)
                && let Some(value) = string.unquote()
                && !value.contains('\\')
            {
                *text = format!("{value:?}");
            }
        }
        SignatureExpression::Apply {
            constructor,
            arguments,
        } => {
            **constructor = normalize_expression(*constructor.clone());
            for argument in arguments {
                *argument = normalize_expression(argument.clone());
            }
        }
        SignatureExpression::Sequence { items } => {
            for item in items {
                *item = normalize_expression(item.clone());
            }
        }
        SignatureExpression::LanguageSpecific {
            children, source, ..
        } => {
            *source = None;
            for child in children {
                *child = normalize_expression(child.clone());
            }
        }
        _ => {}
    }
    expression
}

fn heading(title: &str, span: SourceSpan) -> Block {
    Block::Heading {
        level: 2,
        attributes: Attributes::default(),
        inlines: vec![Inline::Text {
            value: title.into(),
            span,
        }],
        span,
    }
}

fn code_block(source: String, span: SourceSpan) -> Block {
    Block::CodeBlock {
        language: Some("r".into()),
        source,
        source_segments: vec![],
        span,
    }
}

fn flush(blocks: &mut Vec<Block>, inlines: &mut Vec<Inline>, span: SourceSpan) {
    if inlines
        .iter()
        .any(|inline| !matches!(inline, Inline::Text { value, .. } if value.trim().is_empty()))
    {
        blocks.push(Block::Paragraph {
            inlines: std::mem::take(inlines),
            span,
        });
    } else {
        inlines.clear();
    }
}

fn plain(nodes: &[RdNode]) -> Option<String> {
    let mut result = String::new();
    for node in nodes {
        match node {
            RdNode::Text(text) | RdNode::RCode(text) | RdNode::Verb(text) => result.push_str(text),
            RdNode::Comment(_) => {}
            RdNode::Group(group) => result.push_str(&plain(group.children())?),
            _ => return None,
        }
    }
    Some(result)
}

fn r_name(name: &str) -> String {
    if name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_'))
    {
        name.into()
    } else {
        format!("`{}`", name.replace('`', "\\`"))
    }
}
