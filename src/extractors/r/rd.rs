use std::collections::{BTreeMap, BTreeSet};

use arity_parser::{
    ast::{AstNode, Expr, HasArgList},
    parser,
};
use rd_ast::{
    RdAstPath, RdListItem, RdListKind, RdNode, RdNodeRef, RdNodesRef, RdShapeError, RdTag,
};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{
    Attributes, Block, DescriptionEntry, Document, DocumentFormat, IdentityRegistry, Inline, Item,
    ItemAlias, ItemAliasKind, ItemKind, ItemLanguageData, ItemReference, ListItem, Parameter,
    ParameterKind, RDeclaration, RGenericReference, RHelpTopic, RItemData, SemanticIdentity,
    Signature, SignatureExpression, SourceEvidence, SourceLocation, SourceRole, SourceSpan,
    SourcedDocument, SourcedSignature,
};

use super::{diagnostic, located, namespace::Namespace, provenance, source};

mod conditionals;
mod content;

pub(super) struct Topic {
    name: String,
    pub(super) package: bool,
    pub(super) dataset: bool,
    deprecated: bool,
    pub(super) aliases: Vec<String>,
    pub(super) internal: bool,
    parts: Vec<Part>,
    usages: Vec<Usage>,
    source: SourceLocation,
    raw: String,
    pub(super) valid: bool,
    pub(super) uses_r: bool,
}

impl Topic {
    pub(super) fn has_value_usage(&self) -> bool {
        self.usages.is_empty() || self.usages.iter().any(|usage| usage.arguments.is_none())
    }
}

enum Part {
    Title(Box<Block>),
    Introduction(Vec<Block>),
    Blocks(Vec<Block>),
    Usage,
    Arguments(Vec<(Vec<String>, DescriptionEntry)>),
}

struct Usage {
    name: String,
    method: Option<(String, String)>,
    arguments: Option<String>,
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
    os_type: &str,
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
    let mut context = Context {
        raw: text,
        source,
        diagnostics,
        uses_r: false,
    };
    let document =
        rd_ast::RdDocument::new(context.prepare_nodes(parsed.document().top_level(), os_type));
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
        document.inspect_doc_type(),
        document.inspect_format(),
        document.inspect_source(),
    ] {
        if let Err(error) = view {
            context.shape(error);
            valid = false;
        }
    }
    let name = document
        .inspect_name()
        .ok()
        .flatten()
        .and_then(|field| plain(field.body()))
        .unwrap_or_default();
    if name.trim().is_empty() {
        context.error(
            DiagnosticCode::RRdInformationLoss,
            "Rd requires a static topic name.",
        );
        valid = false;
    }
    let doc_type = document
        .inspect_doc_type()
        .ok()
        .flatten()
        .and_then(|field| plain(field.body()));
    let package = doc_type
        .as_ref()
        .is_some_and(|kind| kind.trim() == "package");
    let dataset = doc_type.as_ref().is_some_and(|kind| kind.trim() == "data");
    let deprecated = (name.ends_with("_deprecated") || name.ends_with("-deprecated"))
        && document.inspect_usage().ok().flatten().is_none();
    let internal = !package
        && document.nodes().iter().any(|node| {
            matches!(node, RdNode::Tagged(tagged) if *tagged.tag() == RdTag::Keyword
            && plain(tagged.children()).is_some_and(|keyword| keyword.trim() == "internal"))
        });
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
                        let Some(names) = argument_label(argument.name()) else {
                            context.error(
                                DiagnosticCode::RRdInformationLoss,
                                "Rd argument names must be static text.",
                            );
                            valid = false;
                            continue;
                        };
                        let term = vec![Block::Paragraph {
                            inlines: vec![Inline::Code {
                                value: names.trim().into(),
                                span: context.span(),
                            }],
                            span: context.span(),
                        }];
                        let description = context.blocks(argument.description_ref());
                        arguments.push((
                            names
                                .split(',')
                                .map(|name| name.trim().trim_matches('`').to_owned())
                                .collect(),
                            DescriptionEntry {
                                term,
                                description,
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
    for (index, cursor) in document.top_level().iter().enumerate() {
        let node = cursor.node();
        let path = cursor.path();
        let RdNode::Tagged(tagged) = node else {
            if !matches!(node, RdNode::Comment(_))
                && plain(std::slice::from_ref(node)).is_none_or(|s| !s.trim().is_empty())
            {
                parts.push(Part::Blocks(
                    context.blocks(document.top_level().slice(index..index + 1).unwrap()),
                ));
            }
            continue;
        };
        match tagged.tag() {
            RdTag::Name | RdTag::Alias => {}
            RdTag::DocType if package || dataset => {}
            RdTag::Usage => {
                let parsed = context.usages(cursor.children(), dataset);
                if parsed.is_empty() && !tagged.children().is_empty() {
                    parts.push(Part::Blocks(vec![context.unsupported_block("usage", path)]));
                } else {
                    usages.extend(parsed);
                    parts.push(Part::Usage);
                }
            }
            RdTag::Arguments => parts.push(Part::Arguments(arguments.clone())),
            RdTag::Title => parts.push(Part::Title(Box::new(Block::Heading {
                level: 2,
                attributes: Attributes::default(),
                inlines: context.inlines(cursor.children()),
                span: context.span(),
            }))),
            RdTag::Description => parts.push(Part::Introduction(context.blocks(cursor.children()))),
            RdTag::Details
            | RdTag::Value
            | RdTag::References
            | RdTag::Note
            | RdTag::SeeAlso
            | RdTag::Author
            | RdTag::Format
            | RdTag::Source
            | RdTag::Keyword
            | RdTag::Concept => {
                let title = match tagged.tag() {
                    RdTag::Details => "Details",
                    RdTag::Value => "Value",
                    RdTag::References => "References",
                    RdTag::Note => "Notes",
                    RdTag::SeeAlso => "See also",
                    RdTag::Author => "Author",
                    RdTag::Format => "Format",
                    RdTag::Source => "Source",
                    RdTag::Keyword => "Keywords",
                    _ => "Concepts",
                };
                let mut blocks = vec![heading(title, context.span())];
                blocks.extend(context.blocks(cursor.children()));
                parts.push(Part::Blocks(blocks));
            }
            RdTag::Examples => {
                let mut title = heading("Examples", context.span());
                if let Block::Heading { attributes, .. } = &mut title {
                    attributes.classes.push(crate::ir::SpannedString {
                        value: "rd-examples".into(),
                        span: context.span(),
                    });
                }
                let mut blocks = vec![title];
                match context.code(cursor.children(), &mut BTreeMap::new()) {
                    Some(code) => blocks.push(code_block(code.trim().into(), context.span())),
                    None => blocks.push(context.unsupported_block("examples", path)),
                }
                parts.push(Part::Blocks(blocks));
            }
            _ => parts.push(Part::Blocks(
                context.blocks(document.top_level().slice(index..index + 1).unwrap()),
            )),
        }
    }
    Some(Topic {
        name: name.trim().into(),
        package,
        dataset,
        deprecated,
        internal,
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

    fn unsupported(&mut self, kind: &str, path: &RdAstPath) {
        self.diagnostics.push(diagnostic(
            DiagnosticCode::UnsupportedRd,
            Severity::Warning,
            format!("Unsupported Rd `{kind}` at {path}; content remains unevaluated."),
            self.source,
        ));
    }

    fn unsupported_block(&mut self, kind: &str, path: &RdAstPath) -> Block {
        self.unsupported(kind, path);
        Block::Unsupported {
            source_kind: format!("rd-{kind}"),
            // Exact node ranges are unavailable; the enclosing document retains the source.
            raw: String::new(),
            span: self.span(),
        }
    }

    fn blocks(&mut self, nodes: RdNodesRef<'_>) -> Vec<Block> {
        let mut result = vec![];
        let mut paragraph = vec![];
        let mut line_has_content = false;
        // Bare braces group Rd content without creating a paragraph boundary.
        fn ungroup<'a>(nodes: RdNodesRef<'a>, output: &mut Vec<RdNodeRef<'a>>) {
            for cursor in nodes {
                match cursor.node() {
                    RdNode::Group(_) => ungroup(cursor.children(), output),
                    RdNode::Tagged(tagged) if tagged.tag().as_rd_tag() == "LIST" => {
                        ungroup(cursor.children(), output);
                    }
                    _ => output.push(cursor),
                }
            }
        }
        let mut content = vec![];
        ungroup(nodes, &mut content);
        for cursor in content {
            let node = cursor.node();
            let child_path = cursor.path();
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
                    RdTag::Section
                    | RdTag::Subsection
                    | RdTag::Tabular
                    | RdTag::Deqn
                    | RdTag::Item => {
                        flush(&mut result, &mut paragraph, self.span());
                        line_has_content = false;
                        result.extend(self.content_block(&cursor));
                        continue;
                    }
                    RdTag::Itemize | RdTag::Enumerate | RdTag::Describe => {
                        flush(&mut result, &mut paragraph, self.span());
                        line_has_content = false;
                        if tagged.tag() == &RdTag::Describe && tagged.option().is_none() {
                            result.extend(self.describe_blocks(&cursor));
                            continue;
                        }
                        match cursor.inspect_list() {
                            Ok(Some(list)) => {
                                let mut items = vec![];
                                for entry in list.items() {
                                    let blocks = match entry {
                                        Ok(RdListItem::Delimited(item)) => {
                                            self.blocks(item.body_ref())
                                        }
                                        Ok(RdListItem::Described(item)) => {
                                            let mut blocks = vec![Block::Paragraph {
                                                inlines: self.inlines(item.label_ref()),
                                                span: self.span(),
                                            }];
                                            blocks.extend(self.blocks(item.body_ref()));
                                            blocks
                                        }
                                        Err(error) => {
                                            self.shape(error);
                                            vec![self.unsupported_block("list-item", child_path)]
                                        }
                                        _ => vec![self.unsupported_block("list-item", child_path)],
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
                            Ok(None) => unreachable!("list node"),
                            Err(error) => {
                                self.shape(error);
                                result.push(self.unsupported_block("list", child_path));
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
                            result.push(self.unsupported_block("preformatted", child_path));
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            let inlines = self.inline_node(&cursor);
            line_has_content |= !inlines.is_empty();
            paragraph.extend(inlines);
        }
        flush(&mut result, &mut paragraph, self.span());
        result
    }

    fn inlines(&mut self, nodes: RdNodesRef<'_>) -> Vec<Inline> {
        nodes
            .iter()
            .flat_map(|cursor| self.inline_node(&cursor))
            .collect()
    }

    fn inline_node(&mut self, cursor: &RdNodeRef<'_>) -> Vec<Inline> {
        let node = cursor.node();
        let path = cursor.path();
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
            RdNode::Group(_) => result.extend(self.inlines(cursor.children())),
            RdNode::Tagged(tagged) => match tagged.tag() {
                RdTag::Emph | RdTag::Var | RdTag::Dfn => result.push(Inline::Emphasis {
                    inlines: self.inlines(cursor.children()),
                    span,
                }),
                RdTag::Strong | RdTag::Bold => result.push(Inline::Strong {
                    inlines: self.inlines(cursor.children()),
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
                        let mut inlines = self.inlines(cursor.children());
                        if tagged.tag() == &RdTag::Code {
                            content::code_style(&mut inlines);
                        }
                        result.extend(inlines);
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
                    result.extend(self.inlines(cursor.children()));
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
                RdTag::Href => match cursor.inspect_href() {
                    Ok(Some(link)) => {
                        if let Some(target) = plain(link.url()) {
                            result.push(Inline::Link {
                                inlines: self.inlines(link.display_ref()),
                                target,
                                title: None,
                                attributes: Attributes::default(),
                                span,
                            });
                        } else {
                            result.push(self.unsupported_inline("href", path));
                        }
                    }
                    Ok(None) => unreachable!("href node"),
                    Err(error) => {
                        self.shape(error);
                        result.push(self.unsupported_inline("href", path));
                    }
                },
                RdTag::Link => result.push(self.help_link(cursor)),
                RdTag::Eqn => result.push(self.equation_inline(cursor)),
                RdTag::Doi => result.push(self.doi_link(tagged, path)),
                _ => result.push(self.unsupported_inline(tagged.tag().as_rd_tag(), path)),
            },
            _ => result.push(self.unsupported_inline("node", path)),
        }
        result
    }

    fn unsupported_inline(&mut self, kind: &str, path: &RdAstPath) -> Inline {
        self.unsupported(kind, path);
        Inline::Unsupported {
            source_kind: format!("rd-{kind}"),
            // Exact node ranges are unavailable; the enclosing document retains the source.
            raw: String::new(),
            span: self.span(),
        }
    }

    fn code(
        &mut self,
        nodes: RdNodesRef<'_>,
        methods: &mut BTreeMap<String, (String, String)>,
    ) -> Option<String> {
        let mut result = String::new();
        for cursor in nodes {
            match cursor.node() {
                RdNode::Text(text) | RdNode::RCode(text) | RdNode::Verb(text) => {
                    result.push_str(text)
                }
                RdNode::Comment(_) => {}
                RdNode::Group(_) => result.push_str(&self.code(cursor.children(), methods)?),
                RdNode::Tagged(tagged) => match tagged.tag() {
                    RdTag::Method | RdTag::S3Method => match cursor.inspect_method() {
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
                        result.push_str(&self.code(cursor.children(), methods)?);
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
        Some(result)
    }

    fn usages(&mut self, nodes: RdNodesRef<'_>, dataset: bool) -> Vec<Usage> {
        let mut methods = BTreeMap::new();
        let Some(code) = self.code(nodes.clone(), &mut methods) else {
            self.unsupported("usage", nodes.container_path());
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
            if matches!(&expr, Expr::Name(name) if !name.is_reserved_constant())
                || matches!(&expr, Expr::StringLiteral(name) if name.is_backtick())
                || dataset
            {
                let bare = matches!(&expr, Expr::Name(name) if !name.is_reserved_constant())
                    || matches!(&expr, Expr::StringLiteral(name) if name.is_backtick());
                if let Some(name) = bare.then(|| source::static_name(&expr)).flatten() {
                    result.push(Usage {
                        name,
                        method: None,
                        arguments: None,
                        parameters: vec![],
                    });
                } else {
                    self.error(
                        DiagnosticCode::RRdInformationLoss,
                        "Dataset usage requires a bare object name.",
                    );
                }
                continue;
            }
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
                arguments: Some(arguments),
                parameters,
            });
        }
        result
    }
}

pub(super) fn attach(
    package: &str,
    items: &mut BTreeMap<String, Item>,
    topics: &[Topic],
    namespace: &Namespace,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !namespace.valid {
        return;
    }
    for topic in topics
        .iter()
        .filter(|topic| topic.valid && (topic.dataset || topic.deprecated))
    {
        let identity = if topic.dataset {
            SemanticIdentity::r_dataset(&topic.name)
        } else {
            SemanticIdentity::r_deprecated_topic(&topic.name)
        }
        .expect("static topic name");
        let id = identity.item_id().to_owned();
        if items.contains_key(&id) {
            continue;
        }
        if items.values().any(|item| {
            std::iter::once(&topic.name)
                .chain(&topic.aliases)
                .any(|name| {
                    item.qualified_name == *name
                        || item
                            .aliases
                            .iter()
                            .any(|alias| alias.qualified_name == *name)
                })
        }) {
            // Existing deprecated functions keep their maintained identities.
            if topic.deprecated {
                continue;
            }
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                format!(
                    "Dataset topic `{}` conflicts with another declaration or alias.",
                    topic.name
                ),
                &topic.source,
            ));
            continue;
        }
        items.insert(
            id,
            Item {
                kind: if topic.dataset {
                    ItemKind::Constant
                } else {
                    ItemKind::Namespace
                },
                name: topic.name.clone(),
                qualified_name: topic.name.clone(),
                language_data: Some(ItemLanguageData::R(RItemData {
                    exported: std::iter::once(&topic.name)
                        .chain(&topic.aliases)
                        .any(|name| namespace.exports.contains_key(name)),
                    internal: topic.internal,
                    help_topic: None,
                    declaration: if topic.dataset {
                        RDeclaration::Dataset
                    } else {
                        RDeclaration::DeprecatedTopic
                    },
                })),
                aliases: topic
                    .aliases
                    .iter()
                    .map(|alias| ItemAlias {
                        qualified_name: alias.clone(),
                        kind: ItemAliasKind::RdAlias,
                        sources: vec![SourceEvidence {
                            source: topic.source.clone(),
                            role: SourceRole::Documentation,
                            parsers: BTreeSet::from(["rd-source".into(), "rd-ast".into()]),
                        }],
                    })
                    .collect(),
                signatures: if topic.deprecated {
                    vec![]
                } else {
                    vec![SourcedSignature {
                        signature: Signature::Value {
                            annotation: None,
                            value: None,
                        },
                        sources: vec![SourceEvidence {
                            source: topic.source.clone(),
                            role: SourceRole::Documentation,
                            parsers: BTreeSet::from(["rd-source".into(), "rd-ast".into()]),
                        }],
                    }]
                },
                documentation: None,
                source_location: Some(topic.source.clone()),
                children: vec![],
                provenance: std::iter::once(provenance(&topic.source, true))
                    .chain(
                        namespace
                            .exports
                            .iter()
                            .filter(|(name, _)| {
                                *name == &topic.name || topic.aliases.contains(name)
                            })
                            .flat_map(|(_, sources)| sources)
                            .map(|source| provenance(source, false)),
                    )
                    .collect(),
            },
        );
    }
    let mut package_topic_seen = false;
    for topic in topics.iter().filter(|topic| topic.valid && topic.package) {
        if package_topic_seen {
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                "Multiple package Rd topics claim the same package.",
                &topic.source,
            ));
            continue;
        }
        package_topic_seen = true;
        if topic.aliases.iter().any(|alias| {
            items.values().any(|item| {
                item.qualified_name == *alias
                    || item
                        .aliases
                        .iter()
                        .any(|item_alias| item_alias.qualified_name == *alias)
            })
        }) {
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                "A package Rd alias also names a public API declaration.",
                &topic.source,
            ));
        }
        let Ok(identity) = SemanticIdentity::r_package(&topic.name) else {
            continue;
        };
        let id = IdentityRegistry::new(package)
            .and_then(|mut registry| registry.register(&identity))
            .expect("configured package ID")
            .item;
        if items.contains_key(&id) {
            diagnostics.push(diagnostic(
                DiagnosticCode::RConflictingSurface,
                Severity::Error,
                format!("Multiple package Rd topics document `{}`.", topic.name),
                &topic.source,
            ));
            continue;
        }
        let span = SourceSpan {
            start: 0,
            end: topic.raw.len(),
        };
        let blocks = topic
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Title(title) => Some(vec![title.as_ref().clone()]),
                Part::Introduction(blocks) | Part::Blocks(blocks) => Some(blocks.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        let document = SourcedDocument {
            document: Document {
                span,
                frontmatter: None,
                blocks,
            },
            source_format: DocumentFormat::Extracted { name: "rd".into() },
            source_location: Some(topic.source.clone()),
            generated_source: None,
            raw_source: Some(topic.raw.clone()),
            provenance: vec![provenance(&topic.source, true)],
        };
        items.insert(
            id,
            Item {
                kind: ItemKind::Namespace,
                name: topic.name.clone(),
                qualified_name: topic.name.clone(),
                language_data: None,
                aliases: topic
                    .aliases
                    .iter()
                    .map(|alias| ItemAlias {
                        qualified_name: alias.clone(),
                        kind: ItemAliasKind::RdAlias,
                        sources: vec![SourceEvidence {
                            source: topic.source.clone(),
                            role: SourceRole::Documentation,
                            parsers: BTreeSet::from(["rd-source".into(), "rd-ast".into()]),
                        }],
                    })
                    .collect(),
                signatures: vec![],
                documentation: Some(document),
                source_location: Some(topic.source.clone()),
                children: vec![],
                provenance: vec![provenance(&topic.source, true)],
            },
        );
    }
    let mut names = BTreeMap::<String, String>::new();
    for (id, item) in items.iter() {
        if item.kind == ItemKind::Namespace && item.language_data.is_none() {
            continue;
        }
        names.insert(item.qualified_name.clone(), id.clone());
        for alias in &item.aliases {
            names.insert(alias.qualified_name.clone(), id.clone());
        }
    }
    let mut candidates = BTreeMap::<String, Vec<(usize, Vec<String>)>>::new();
    let mut shared_topics = BTreeMap::new();
    for (index, topic) in topics
        .iter()
        .enumerate()
        .filter(|(_, topic)| topic.valid && !topic.package)
    {
        for usage in &topic.usages {
            if !items
                .iter()
                .any(|(id, item)| usage_matches(usage, item, &names, id))
            {
                diagnostics.push(diagnostic(
                    DiagnosticCode::RUnresolvedDefinition,
                    Severity::Error,
                    format!(
                        "Rd usage `{}` has no unambiguous maintained declaration.",
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
                let dataset = matches!(&items[id].language_data,
                    Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::Dataset));
                if topic.dataset != dataset {
                    diagnostics.push(diagnostic(
                        DiagnosticCode::RConflictingSurface,
                        Severity::Error,
                        format!("Rd topic type disagrees with declaration `{alias}`."),
                        &topic.source,
                    ));
                    continue;
                }
                targets.entry(id.clone()).or_default().push(alias.clone());
            } else {
                unknown.push(alias.clone());
            }
        }
        let primary = names.get(&topic.name).filter(|id| targets.contains_key(*id))
            .or_else(|| targets.keys().find(|id| matches!(&items[*id].language_data,
                Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::S3Generic { .. }))))
            .or_else(|| topic.usages.iter().find_map(|usage| targets.keys().find(|id|
                usage_matches(usage, &items[*id], &names, id))))
            .or_else(|| targets.keys().next()).cloned();
        if !unknown.is_empty() {
            if let Some(primary) = &primary {
                // A topic alias names the whole document even when several declarations share it.
                targets.get_mut(primary).unwrap().extend(unknown);
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
        if targets.len() > 1 {
            shared_topics.insert(
                index,
                RHelpTopic {
                    name: topic.name.clone(),
                    primary: ItemReference {
                        package: package.into(),
                        item: primary.unwrap(),
                    },
                    members: targets
                        .keys()
                        .map(|id| ItemReference {
                            package: package.into(),
                            item: id.clone(),
                        })
                        .collect(),
                },
            );
        }
        let parameters: BTreeSet<_> = targets
            .keys()
            .flat_map(|id| {
                item_parameters(&items[id])
                    .iter()
                    .map(|parameter| parameter.name.as_str())
            })
            .collect();
        // Standalone deprecated topics document removed APIs without callable formals.
        // Topics attached to maintained declarations still require matching arguments.
        let standalone_deprecated = !targets.is_empty()
            && targets.keys().all(|id| matches!(&items[id].language_data,
                Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::DeprecatedTopic)));
        for part in &topic.parts {
            if let Part::Arguments(arguments) = part
                && !standalone_deprecated
            {
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
        if item.kind == ItemKind::Namespace && item.language_data.is_none() {
            continue;
        }
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
        if let Some(ItemLanguageData::R(data)) = &mut item.language_data {
            data.internal = topic.internal;
            data.help_topic = shared_topics.get(index).cloned();
        }
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
        let parameters = item_parameters(item);
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
        // Rd generators often place the description after usage and arguments.
        // Readers need the topic's purpose before its calling conventions.
        for part in &topic.parts {
            if let Part::Title(title) = part {
                blocks.push(title.as_ref().clone());
            }
        }
        for part in &topic.parts {
            if let Part::Introduction(content) = part {
                blocks.extend(content.clone());
            }
        }
        for part in &topic.parts {
            match part {
                Part::Title(_) | Part::Introduction(_) => {}
                Part::Blocks(content) => blocks.extend(content.clone()),
                Part::Usage if !topic.usages.is_empty() => {
                    blocks.push(heading("Usage", span));
                    for usage in &topic.usages {
                        let name = r_name(&usage.name);
                        let code = usage.arguments.as_ref().map_or_else(
                            || name.clone(),
                            |arguments| format!("{name}({arguments})"),
                        );
                        blocks.push(code_block(code, span));
                    }
                }
                Part::Usage => {}
                Part::Arguments(arguments) => {
                    let entries: Vec<_> =
                        arguments.iter().map(|(_, entry)| entry.clone()).collect();
                    if !entries.is_empty() {
                        blocks.push(heading("Arguments", span));
                        blocks.push(Block::description_list(entries, span));
                    }
                }
            }
        }
        let mut document_provenance = provenance(&topic.source, true);
        if topic.uses_r {
            document_provenance.tools.insert(
                "arity-parser".into(),
                env!("DIPLODOCUS_ARITY_VERSION").into(),
            );
        }
        item.documentation = Some(SourcedDocument {
            document: Document {
                span,
                frontmatter: None,
                blocks,
            },
            source_format: DocumentFormat::Extracted { name: "rd".into() },
            source_location: Some(topic.source.clone()),
            generated_source: None,
            raw_source: Some(topic.raw.clone()),
            provenance: vec![document_provenance],
        });
    }
}

fn usage_matches(usage: &Usage, item: &Item, names: &BTreeMap<String, String>, id: &str) -> bool {
    let dataset = matches!(&item.language_data,
        Some(ItemLanguageData::R(data)) if matches!(data.declaration, RDeclaration::Dataset | RDeclaration::Constant));
    if dataset != usage.arguments.is_none() {
        return false;
    }
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

fn item_parameters(item: &Item) -> &[Parameter] {
    match item
        .signatures
        .first()
        .map(|signature| &signature.signature)
    {
        Some(Signature::Callable { parameters, .. }) => parameters,
        _ => &[],
    }
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

pub(super) fn same_signature(left: &Signature, right: &Signature) -> bool {
    match (left, right) {
        (
            Signature::Callable {
                parameters: left,
                returns: left_returns,
            },
            Signature::Callable {
                parameters: right,
                returns: right_returns,
            },
        ) => {
            same_parameters(left, right)
                && left_returns.clone().map(normalize_expression)
                    == right_returns.clone().map(normalize_expression)
        }
        _ => left == right,
    }
}

fn hexadecimal_number(text: &str) -> Option<f64> {
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    let (mantissa, exponent) =
        text.split_once(['p', 'P'])
            .map_or(Some((text, 0)), |(mantissa, exponent)| {
                exponent
                    .parse::<i32>()
                    .ok()
                    .map(|exponent| (mantissa, exponent))
            })?;
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let digits = mantissa.replace('.', "");
    let value = u128::from_str_radix(digits.trim_start_matches('0'), 16)
        .ok()
        .or_else(|| digits.chars().all(|digit| digit == '0').then_some(0))?;
    if value == 0 {
        return Some(0.0);
    }
    let exponent = exponent.checked_sub(i32::try_from(fraction).ok()?.checked_mul(4)?)?;
    let highest = 127 - value.leading_zeros();
    let mut binary_exponent = i64::from(highest) + i64::from(exponent);
    if binary_exponent > 1023 {
        return Some(f64::INFINITY);
    }
    // Round the exact hexadecimal significand once, including subnormal values.
    // Accumulating through floating-point arithmetic can round it twice.
    let bits = if binary_exponent < -1022 {
        let shift = i64::from(exponent) + 1074;
        if shift >= 0 {
            value.checked_shl(u32::try_from(shift).ok()?)?
        } else {
            round_binary(value, u32::try_from(-shift).ok()?)
        }
    } else {
        let mut significand = if highest > 52 {
            round_binary(value, highest - 52)
        } else {
            value << (52 - highest)
        };
        if significand == 1 << 53 {
            significand >>= 1;
            binary_exponent += 1;
        }
        if binary_exponent > 1023 {
            return Some(f64::INFINITY);
        }
        (((binary_exponent + 1023) as u128) << 52) | (significand & ((1 << 52) - 1))
    };
    Some(f64::from_bits(u64::try_from(bits).ok()?))
}

fn round_binary(value: u128, shift: u32) -> u128 {
    if shift > 128 {
        return 0;
    }
    if shift == 128 {
        return u128::from(value > 1 << 127);
    }
    if shift == 0 {
        return value;
    }
    let rounded = value >> shift;
    let remainder = value & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    rounded + u128::from(remainder > halfway || (remainder == halfway && rounded & 1 == 1))
}

fn normalized_number(text: &str, kind: &str) -> Option<String> {
    let text = text.strip_suffix(['L', 'i']).unwrap_or(text);
    let value = if text == "Inf" {
        f64::INFINITY
    } else if text.starts_with("0x") || text.starts_with("0X") {
        hexadecimal_number(text)?
    } else {
        text.parse::<f64>().ok()?
    };
    Some(format!("r-{kind}:{:016x}", value.to_bits()))
}

fn string_escape_digits(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    radix: u32,
    limit: usize,
) -> Option<u32> {
    let mut value = None;
    for _ in 0..limit {
        let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(radix)) else {
            break;
        };
        chars.next();
        value = Some(value.unwrap_or(0) * radix + digit);
    }
    value
}

fn string_unicode_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    limit: usize,
) -> Option<u32> {
    let braced = chars.next_if_eq(&'{').is_some();
    let value = string_escape_digits(chars, 16, limit)?;
    if braced && chars.next()? != '}' {
        return None;
    }
    Some(value)
}

fn decoded_string(text: &str) -> Option<Vec<u8>> {
    let mut chars = text.chars().peekable();
    let mut bytes = vec![];
    let mut unicode_escape = false;
    let mut byte_escape = false;
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            bytes.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        let escaped = chars.next()?;
        let byte = match escaped {
            'a' => b'\x07',
            'b' => b'\x08',
            'f' => b'\x0c',
            'n' | '\n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            'v' => b'\x0b',
            '\\' | '\'' | '"' | '`' => escaped as u8,
            'x' => {
                byte_escape = true;
                u8::try_from(string_escape_digits(&mut chars, 16, 2)?).ok()?
            }
            '0'..='7' => {
                byte_escape = true;
                let mut value = escaped.to_digit(8)?;
                for _ in 0..2 {
                    let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(8)) else {
                        break;
                    };
                    chars.next();
                    value = value * 8 + digit;
                }
                u8::try_from(value).ok()?
            }
            'u' | 'U' => {
                unicode_escape = true;
                let mut value =
                    string_unicode_escape(&mut chars, if escaped == 'u' { 4 } else { 8 })?;
                if (0xd800..=0xdbff).contains(&value) {
                    if chars.next()? != '\\' || chars.next()? != 'u' {
                        return None;
                    }
                    let low = string_unicode_escape(&mut chars, 4)?;
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return None;
                    }
                    value = 0x10000 + ((value - 0xd800) << 10) + (low - 0xdc00);
                }
                bytes.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut [0; 4]).as_bytes());
                continue;
            }
            _ => return None,
        };
        bytes.push(byte);
    }
    // R forbids NULs and mixing Unicode escapes with byte escapes.
    if bytes.contains(&0) || (unicode_escape && byte_escape) {
        return None;
    }
    Some(bytes)
}

fn normalize_expression(mut expression: SignatureExpression) -> SignatureExpression {
    match &mut expression {
        SignatureExpression::Literal { text } => {
            let parsed = parser::parse(text);
            match parsed.cst.children_with_tokens().find_map(Expr::cast) {
                Some(Expr::StringLiteral(string)) => {
                    if let Some(value) = string.inner().and_then(decoded_string) {
                        *text = format!("r-string:{value:?}");
                    }
                }
                Some(Expr::FloatLiteral(_) | Expr::IntLiteral(_)) => {
                    let kind = if text.ends_with('L') {
                        "integer"
                    } else {
                        "double"
                    };
                    if let Some(number) = normalized_number(text, kind) {
                        *text = number;
                    }
                }
                Some(Expr::ComplexLiteral(_)) => {
                    if let Some(number) = normalized_number(text, "complex") {
                        *text = number;
                    }
                }
                Some(Expr::Name(name)) if name.name() == "Inf" => {
                    if let Some(number) = normalized_number(text, "double") {
                        *text = number;
                    }
                }
                _ => {}
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

fn argument_label(nodes: &[RdNode]) -> Option<String> {
    let mut result = String::new();
    for node in nodes {
        match node {
            RdNode::Group(group) => result.push_str(&argument_label(group.children())?),
            RdNode::Tagged(tagged) if tagged.option().is_none() => match tagged.tag() {
                RdTag::Dots | RdTag::LDots if tagged.children().is_empty() => {
                    result.push_str("...");
                }
                RdTag::List => result.push_str(&argument_label(tagged.children())?),
                _ => return None,
            },
            _ => result.push_str(&plain(std::slice::from_ref(node))?),
        }
    }
    Some(result)
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

#[cfg(test)]
mod comparison_tests {
    use super::*;

    fn literal(text: &str) -> SignatureExpression {
        normalize_expression(SignatureExpression::Literal { text: text.into() })
    }

    #[test]
    fn string_comparison_decodes_r_escapes_without_changing_values() {
        for (left, right) in [
            (r#""\u0041""#, "\"A\""),
            (r#""\u41""#, "\"A\""),
            (r#""\u{41}""#, "\"A\""),
            (r#""\U0001f600""#, "\"😀\""),
            (r#""\uD83D\uDE00""#, "\"😀\""),
            (r#""\x41""#, "\"A\""),
            (r#""\101""#, "\"A\""),
            (r#""\xc3\xa9""#, "\"é\""),
            (r#""\xff""#, r#""\377""#),
            (r#""\a\b\f\n\r\t\v""#, "\"\x07\x08\x0c\n\r\t\x0b\""),
            (r#"'\"'"#, r#""\"""#),
            (r#"'\\'"#, r#""\\""#),
        ] {
            assert_eq!(literal(left), literal(right), "{left} and {right}");
        }
        for (left, right) in [
            (r#""\u0041""#, "\"B\""),
            (r#""\\u0041""#, "\"A\""),
            (r#""\\n""#, r#""\n""#),
            (r#""\q""#, "\"q\""),
        ] {
            assert_ne!(literal(left), literal(right), "{left} and {right}");
        }
        for invalid in [r"\u{}", r"\u{41", r"\x", r"\777", r"\0", r"\u0041\101"] {
            assert_eq!(decoded_string(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn numeric_comparison_accepts_equivalent_spellings_and_preserves_types() {
        for (left, right) in [
            ("1e-8", "0.00000001"),
            ("1.0", "1"),
            ("0xff", "255"),
            ("0x1.8p1", "3"),
            ("0x0.0000000000001p-1022", "5e-324"),
            ("0x1.fffffffffffffp1023", "1.7976931348623157e308"),
            ("0x100000000000008", "72057594037927940"),
        ] {
            assert_eq!(literal(left), literal(right), "{left} and {right}");
        }
        assert_ne!(literal("1L"), literal("1.0"));
        assert_ne!(literal("1i"), literal("1"));
        assert_ne!(literal("NULL"), literal("0"));
        assert_ne!(literal("\"1\""), literal("1"));
    }
}
