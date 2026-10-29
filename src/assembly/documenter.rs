//! Static Documenter assembly over already extracted Julia declarations.

use std::collections::{BTreeMap, BTreeSet};

use fatou_parser::ast::{AstNode, AstToken, Expr, StringLiteral};

use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::documents::AuthoredFormat;
use crate::ir::*;

#[derive(Default)]
struct Options {
    modules: Vec<String>,
    pages: Vec<String>,
    private: Option<bool>,
    public: Option<bool>,
}

fn options(source: &str) -> Result<Options, &'static str> {
    let parsed = fatou_parser::parser::parse(source);
    if !parsed.diagnostics.is_empty() {
        return Err("Malformed Documenter directive options.");
    }
    let module_name = regex::Regex::new(
        r"^[\p{XID_Start}_][\p{XID_Continue}]*(?:\.[\p{XID_Start}_][\p{XID_Continue}]*)*$",
    )
    .unwrap();
    let mut options = Options::default();
    let mut seen = BTreeSet::new();
    for node in parsed.cst.children() {
        let Some(Expr::AssignmentExpr(assignment)) = Expr::cast(node) else {
            return Err("Documenter options must be literal assignments.");
        };
        if assignment.op().is_none_or(|op| op.text() != "=") {
            return Err("Documenter options require `=` assignments.");
        }
        let Some(Expr::Name(name)) = assignment.lhs() else {
            return Err("Documenter options require simple names.");
        };
        let name = name.syntax().text().to_string();
        if !seen.insert(name.clone()) {
            return Err("Duplicate Documenter directive option.");
        }
        let value = assignment.rhs().ok_or("Missing Documenter option value.")?;
        match name.as_str() {
            "Modules" => {
                let Expr::VectExpr(modules) = value else {
                    return Err("`Modules` must be a literal vector of module names.");
                };
                for module in modules.syntax().children() {
                    let name = module.text().to_string();
                    if !module_name.is_match(&name) || options.modules.contains(&name) {
                        return Err("`Modules` requires distinct, qualified module names.");
                    }
                    options.modules.push(name);
                }
                if options.modules.is_empty() {
                    return Err("`Modules` must contain at least one module.");
                }
            }
            "Private" | "Public" => {
                let boolean = match value.syntax().text().to_string().as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err("`Private` and `Public` must be literal booleans."),
                };
                if name == "Private" {
                    options.private = Some(boolean);
                } else {
                    options.public = Some(boolean);
                }
            }
            "Pages" => {
                let Expr::VectExpr(pages) = value else {
                    return Err("`Pages` must be a literal vector of source-file suffix strings.");
                };
                for page in pages.syntax().children() {
                    let literal = page
                        .children()
                        .next()
                        .and_then(StringLiteral::cast)
                        .filter(|literal| {
                            literal.prefix().is_none()
                                && literal.suffix().is_none()
                                && literal.interpolations().next().is_none()
                        })
                        .ok_or("`Pages` requires ordinary strings without interpolation.")?;
                    let path = fatou_parser::parser::string_value(
                        &literal
                            .content_tokens()
                            .map(|token| token.text().to_owned())
                            .collect::<String>(),
                    )
                    .map_err(|_| "Invalid string escape in `Pages`.")?;
                    if path.is_empty() {
                        return Err("`Pages` entries must not be empty.");
                    }
                    let path = normalized_suffix(&path);
                    if options.pages.contains(&path) {
                        return Err("`Pages` requires distinct source-file suffixes.");
                    }
                    options.pages.push(path);
                }
            }
            _ => {
                return Err(
                    "Supported `@autodocs` options are `Modules`, `Pages`, `Private`, and `Public`.",
                );
            }
        }
    }
    Ok(options)
}

fn normalized_suffix(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|part| *part != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        if parts.is_empty() && !path.starts_with('/') {
            ".".into()
        } else {
            parts.join("/")
        }
    )
}

fn members(item: &Item) -> &[ItemReference] {
    match &item.language_data {
        Some(ItemLanguageData::Julia(data)) => match &data.declaration {
            JuliaDeclaration::Callable {
                role: JuliaCallableRole::Family { methods },
                ..
            } => methods,
            JuliaDeclaration::Type { constructors, .. } => constructors,
            _ => &[],
        },
        _ => &[],
    }
}

struct Entry {
    item: ItemReference,
    selection: Vec<ItemReference>,
}

struct CatalogEntry {
    page: String,
    entry: Entry,
    anchor: String,
}

fn entry(workspace: &Workspace, item: ItemReference) -> Entry {
    let selection = std::iter::once(&item)
        .chain(members(
            &workspace.packages[&item.package].items[&item.item],
        ))
        .cloned()
        .collect();
    Entry { item, selection }
}

fn documented(workspace: &Workspace, reference: &ItemReference) -> bool {
    let item = &workspace.packages[&reference.package].items[&reference.item];
    item.documentation.is_some()
        || members(item)
            .iter()
            .any(|member| documented(workspace, member))
}

fn explicit(
    workspace: &Workspace,
    owner: &ContentOwner,
    source: &str,
) -> Result<Vec<Entry>, String> {
    let parsed = fatou_parser::parser::parse(source);
    if !parsed.diagnostics.is_empty() {
        return Err("Malformed `@docs` selectors.".into());
    }
    let binding = regex::Regex::new(
        r"^(?:@?[\p{XID_Start}_][\p{XID_Continue}!]*)(?:\.@?[\p{XID_Start}_][\p{XID_Continue}!]*)*$",
    ).unwrap();
    let owner = match owner {
        ContentOwner::Package { package } => Some(package.as_str()),
        ContentOwner::Project => None,
    };
    let mut entries = Vec::new();
    for node in parsed.cst.children() {
        let target = node.text().to_string();
        let target = target.trim();
        if !binding.is_match(target)
            && !crate::extractors::julia::method_selector(target)
                .is_some_and(|(name, _)| binding.is_match(&name))
        {
            return Err(
                "`@docs` supports binding names and exact static method selectors only.".into(),
            );
        }
        let item = crate::validation::resolve_item(workspace, owner, target)
            .or_else(|code| {
                // Julia modules bind their own name, as in `SLOPE.SLOPE`.
                if code == DiagnosticCode::UnresolvedItemReference
                    && let Some((parent, name)) = target.rsplit_once('.')
                    && parent.rsplit('.').next() == Some(name)
                    && let Ok(reference) = crate::validation::resolve_item(workspace, owner, parent)
                    && matches!(&workspace.packages[&reference.package].items[&reference.item].language_data,
                        Some(ItemLanguageData::Julia(data)) if matches!(data.declaration, JuliaDeclaration::Module { .. })) {
                    return Ok(reference);
                }
                Err(code)
            })
            .map_err(|code| format!("`@docs` selector `{target}` must identify exactly one extracted Julia item ({code:?})."))?;
        if !matches!(
            workspace.packages[&item.package].items[&item.item].language_data,
            Some(ItemLanguageData::Julia(_))
        ) {
            return Err(format!(
                "`@docs` selector `{target}` must identify a Julia item."
            ));
        }
        if !documented(workspace, &item) {
            return Err(format!(
                "`@docs` selector `{target}` has no retained documentation."
            ));
        }
        entries.push(entry(workspace, item));
    }
    if entries.is_empty() {
        return Err("`@docs` requires at least one binding or method selector.".into());
    }
    Ok(entries)
}

fn select(workspace: &Workspace, options: &Options) -> Result<Vec<Entry>, &'static str> {
    if options.modules.is_empty() {
        return Err("`@autodocs` requires `Modules = [Module, ...]`.");
    }
    let mut selected = Vec::new();
    for module in &options.modules {
        let packages: Vec<_> = workspace.packages.iter().filter(|(_, package)| {
            package.items.values().any(|item| matches!(&item.language_data,
                Some(ItemLanguageData::Julia(data)) if data.defining_module == *module
                    || matches!(data.declaration, JuliaDeclaration::Module { .. }) && item.qualified_name == *module))
        }).collect();
        let [(package_id, package)] = packages.as_slice() else {
            return Err("Each `Modules` entry must identify one extracted Julia module.");
        };
        let mut entries: Vec<_> = package
            .items
            .iter()
            .filter_map(|(id, item)| {
                let Some(ItemLanguageData::Julia(data)) = &item.language_data else {
                    return None;
                };
                if data.defining_module != *module
                    || matches!(
                        data.declaration,
                        JuliaDeclaration::Field { .. }
                            | JuliaDeclaration::Callable {
                                role: JuliaCallableRole::Method { .. },
                                ..
                            }
                    )
                    || (data.visibility == JuliaVisibility::Private
                        && options.private == Some(false))
                    || (data.visibility != JuliaVisibility::Private
                        && options.public == Some(false))
                    || (item.documentation.is_none()
                        && !members(item)
                            .iter()
                            .any(|member| package.items[&member.item].documentation.is_some()))
                {
                    return None;
                }
                let order = match data.declaration {
                    JuliaDeclaration::Module { .. } => 0,
                    JuliaDeclaration::Constant => 1,
                    JuliaDeclaration::Type { .. } => 2,
                    JuliaDeclaration::Callable {
                        binding: JuliaCallableKind::Macro,
                        ..
                    } => 4,
                    _ => 3,
                };
                let mut entry = entry(
                    workspace,
                    ItemReference {
                        package: (*package_id).clone(),
                        item: id.clone(),
                    },
                );
                let mut page_order = 0;
                if !options.pages.is_empty() {
                    let mut matched: Vec<_> = entry
                        .selection
                        .into_iter()
                        .filter_map(|reference| {
                            let document = package.items[&reference.item].documentation.as_ref()?;
                            let path = document.source_location.as_ref()?.path.as_str();
                            let index = options
                                .pages
                                .iter()
                                .position(|suffix| path.ends_with(suffix))?;
                            Some((index, reference))
                        })
                        .collect();
                    matched.sort_by_key(|(index, _)| *index);
                    page_order = matched.first()?.0;
                    entry.selection = matched
                        .into_iter()
                        .map(|(_, reference)| reference)
                        .collect();
                }
                Some((page_order, order, &item.qualified_name, entry))
            })
            .collect();
        entries.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
        selected.extend(entries.into_iter().map(|(_, _, _, entry)| entry));
    }
    Ok(selected)
}

pub(super) fn assemble(
    workspace: &mut Workspace,
    prepared: &BTreeMap<String, super::PreparedPage>,
) {
    let pages: Vec<_> = workspace
        .pages
        .iter()
        .filter_map(|(id, page)| {
            let PageKind::Authored { collection } = &page.kind else {
                return None;
            };
            (workspace.content_collections[collection].format == AuthoredFormat::Documenter)
                .then(|| (id.clone(), collection.clone()))
        })
        .collect();
    let mut catalog: BTreeMap<String, Vec<CatalogEntry>> = BTreeMap::new();
    for (id, collection) in &pages {
        let mut page = workspace.pages.remove(id).unwrap();
        let location = page.document.source_location.clone().unwrap();
        rewrite(&mut page.document.document.blocks, &mut |block| {
            let Block::CodeBlock {
                language: Some(language),
                source,
                span,
                ..
            } = block
            else {
                return None;
            };
            if !matches!(language.as_str(), "@docs" | "@autodocs" | "@index") {
                return None;
            }
            let result = match language.as_str() {
                "@docs" => explicit(workspace, &page.owner, source).map(Some),
                "@autodocs" => options(source)
                    .and_then(|options| select(workspace, &options))
                    .map(Some)
                    .map_err(str::to_owned),
                _ => {
                    if source.trim().is_empty() {
                        Ok(None)
                    } else {
                        Err("`@index` currently accepts an empty body only.".into())
                    }
                }
            };
            match result {
                Ok(Some(items)) => {
                    let entries = catalog.entry(collection.clone()).or_default();
                    let mut blocks = Vec::new();
                    for entry in items {
                        if entries.iter().any(|other| {
                            other.entry.item == entry.item
                                || entry
                                    .selection
                                    .iter()
                                    .any(|reference| other.entry.selection.contains(reference))
                        }) {
                            workspace.diagnostics.insert(error(&location, collection, &prepared[id].relative, *span, "An API item or its documentation may appear in only one `@docs` or `@autodocs` selection per collection."));
                            continue;
                        }
                        let digest = crate::provenance::fingerprint_bytes(
                            format!("{}:{}", entry.item.package, entry.item.item).as_bytes(),
                        );
                        let identifier = format!("api-{}", digest.value);
                        blocks.push(Block::ApiDocumentation {
                            item: entry.item.clone(),
                            selection: entry.selection.clone(),
                            identifier: identifier.clone(),
                            span: *span,
                        });
                        entries.push(CatalogEntry {
                            page: id.clone(),
                            entry,
                            anchor: identifier,
                        });
                    }
                    Some(blocks)
                }
                Ok(None) => None,
                Err(message) => {
                    workspace.diagnostics.insert(error(
                        &location,
                        collection,
                        &prepared[id].relative,
                        *span,
                        &message,
                    ));
                    None
                }
            }
        });
        workspace.pages.insert(id.clone(), page);
    }
    for (id, collection) in pages {
        let mut page = workspace.pages.remove(&id).unwrap();
        let from = page.document.source_location.as_ref().unwrap();
        rewrite(&mut page.document.document.blocks, &mut |block| {
            let Block::CodeBlock {
                language: Some(language),
                span,
                ..
            } = block
            else {
                return None;
            };
            if language != "@index" {
                return None;
            }
            let mut entries: Vec<_> = catalog.get(&collection).into_iter().flatten().collect();
            entries.sort_by_key(|entry| {
                let item = &entry.entry.item;
                &workspace.packages[&item.package].items[&item.item].qualified_name
            });
            Some(vec![Block::List {
                ordered: false,
                items: entries
                    .into_iter()
                    .map(|entry| {
                        let item = &entry.entry.item;
                        let anchor = &entry.anchor;
                        let target = workspace
                            .pages
                            .get(&entry.page)
                            .map_or(from, |page| page.document.source_location.as_ref().unwrap());
                        let target = format!(
                            "{}#{anchor}",
                            relative_link(from.path.as_str(), target.path.as_str())
                        );
                        ListItem {
                            checked: None,
                            blocks: vec![Block::Paragraph {
                                inlines: vec![Inline::Link {
                                    inlines: vec![Inline::Code {
                                        value: workspace.packages[&item.package].items[&item.item]
                                            .qualified_name
                                            .clone(),
                                        span: *span,
                                    }],
                                    target,
                                    title: None,
                                    attributes: Attributes::default(),
                                    span: *span,
                                }],
                                span: *span,
                            }],
                            span: *span,
                        }
                    })
                    .collect(),
                span: *span,
            }])
        });
        workspace.pages.insert(id, page);
    }
}

fn error(
    source: &SourceLocation,
    collection: &str,
    path: &crate::diagnostics::DiagnosticPath,
    span: SourceSpan,
    message: &str,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::UnsupportedAuthoredSyntax,
        Severity::Error,
        message,
    )
    .with_entity(DiagnosticEntity::Document {
        collection: collection.into(),
        path: path.clone(),
    })
    .with_source(DiagnosticSource::Repository {
        repository: source.repository.clone(),
        path: source.path.clone(),
    });
    diagnostic.span = Some(span);
    diagnostic
}

fn relative_link(from: &str, to: &str) -> String {
    let mut from: Vec<_> = from.split('/').collect();
    from.pop();
    let to: Vec<_> = to.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let path = std::iter::repeat_n("..", from.len() - common)
        .chain(to[common..].iter().copied())
        .collect::<Vec<_>>()
        .join("/");
    const PATH: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'#')
        .add(b'?')
        .add(b'%');
    percent_encoding::utf8_percent_encode(&path, PATH).to_string()
}

fn rewrite(blocks: &mut Vec<Block>, f: &mut impl FnMut(&Block) -> Option<Vec<Block>>) {
    let mut result = Vec::new();
    for mut block in std::mem::take(blocks) {
        if let Some(replacement) = f(&block) {
            result.extend(replacement);
            continue;
        }
        match &mut block {
            Block::BlockQuote { blocks, .. }
            | Block::Callout { blocks, .. }
            | Block::Container { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. } => rewrite(blocks, f),
            Block::List { items, .. } => {
                for item in items {
                    rewrite(&mut item.blocks, f);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        rewrite(&mut cell.blocks, f);
                    }
                }
            }
            _ => {}
        }
        result.push(block);
    }
    *blocks = result;
}
