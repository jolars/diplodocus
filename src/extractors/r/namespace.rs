use std::collections::{BTreeMap, BTreeSet};

use arity_parser::{
    ast::Expr,
    namespace::{self, DirectiveKind},
    syntax::SyntaxKind,
};

use crate::ir::SourceLocation;

use super::{located, source::static_name};

#[derive(Default)]
pub(super) struct Namespace {
    pub valid: bool,
    pub exports: BTreeMap<String, Vec<SourceLocation>>,
    pub imports: BTreeMap<String, BTreeSet<String>>,
    pub unknown_imports: BTreeSet<String>,
    pub package_imports: BTreeMap<String, Vec<SourceLocation>>,
    pub methods: Vec<Registration>,
}

pub(super) struct Registration {
    pub generic: String,
    pub class: String,
    pub binding: String,
    pub source: SourceLocation,
}

pub(super) fn parse(text: &str, source: &SourceLocation) -> Namespace {
    let parsed = namespace::parse(text);
    let mut result = Namespace::default();
    for directive in parsed.document().directives() {
        let range = directive.text_range();
        let location = located(source, usize::from(range.start()), usize::from(range.end()));
        if !matches!(
            directive.kind(),
            DirectiveKind::Export
                | DirectiveKind::ImportFrom
                | DirectiveKind::Import
                | DirectiveKind::S3Method
        ) || directive
            .syntax()
            .ancestors()
            .any(|node| node.kind() == SyntaxKind::IF_EXPR)
        {
            continue;
        }
        let arguments: Option<Vec<String>> = directive
            .arguments()
            .map(|argument| {
                if argument.name().is_some() {
                    return None;
                }
                let range = argument.value_range()?;
                let value = &text[usize::from(range.start())..usize::from(range.end())];
                let parsed = arity_parser::parser::parse(value);
                if !parsed.diagnostics.is_empty() {
                    return None;
                }
                let expression = parsed.cst.children_with_tokens().find_map(Expr::cast)?;
                static_name(&expression)
            })
            .collect();
        let Some(args) = arguments else { continue };
        match directive.kind() {
            DirectiveKind::Export if !args.is_empty() => {
                for name in args {
                    result
                        .exports
                        .entry(name)
                        .or_default()
                        .push(location.clone());
                }
            }
            DirectiveKind::ImportFrom if args.len() >= 2 => {
                for name in &args[1..] {
                    result
                        .imports
                        .entry(name.clone())
                        .or_default()
                        .insert(args[0].clone());
                }
            }
            DirectiveKind::Import if !args.is_empty() => {
                for package in args {
                    result
                        .package_imports
                        .entry(package)
                        .or_default()
                        .push(location.clone());
                }
            }
            DirectiveKind::S3Method if (2..=3).contains(&args.len()) => {
                let generic_name = args[0]
                    .split_once("::")
                    .map_or(args[0].as_str(), |(_, name)| name);
                result.methods.push(Registration {
                    generic: args[0].clone(),
                    class: args[1].clone(),
                    binding: args
                        .get(2)
                        .cloned()
                        .unwrap_or_else(|| format!("{generic_name}.{}", args[1])),
                    source: location.clone(),
                });
            }
            _ => {}
        }
    }
    result
}
