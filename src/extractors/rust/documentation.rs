//! Resolve compiler-proven intra-doc links before shared Markdown validation.

use std::collections::{BTreeMap, BTreeSet};

use rustdoc_types as rd;

use crate::diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEntity, DiagnosticSource, Severity,
};
use crate::documents::{AuthoredFormat, parse_authored_document};
use crate::ir::*;

#[derive(Clone, Copy)]
pub(super) struct DocumentationContext<'a> {
    pub krate: &'a rd::Crate,
    pub references: &'a BTreeMap<rd::Id, ItemReference>,
    pub package: &'a str,
}

/// Parse without claiming doc-comment byte offsets that rustdoc does not emit.
pub(super) fn parse(
    context: DocumentationContext<'_>,
    docs: &str,
    native: &rd::Item,
    mut location: Option<SourceLocation>,
    item: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> SourcedDocument {
    let DocumentationContext {
        krate,
        references,
        package,
    } = context;
    if let Some(location) = &mut location {
        location.span = None;
    }
    let links: BTreeMap<_, _> = native
        .links
        .iter()
        .map(|(name, id)| (name.clone(), *id))
        .collect();
    let mut markdown = docs.to_owned();
    let mut substitutions = BTreeMap::new();
    // Definitions let the shared parser recognize ordinary shortcut links as
    // links while all original prose, block offsets, and labels stay intact.
    markdown.push_str("\n\n");
    for (index, (label, id)) in links.iter().enumerate() {
        if label.contains(['\n', '\r', '[', ']']) {
            continue;
        }
        let marker = format!("diplodocus-rustdoc-{index}");
        for label in [label.clone(), format!("`{label}`")] {
            markdown.push_str(&format!("[{label}]: {marker}\n"));
        }
        substitutions.insert(marker, *id);
    }
    let mut parsed = parse_authored_document(&markdown, AuthoredFormat::Gfm);
    parsed.document.span.end = docs.len();
    prepare_blocks(&mut parsed.document.blocks, &mut BTreeSet::new());
    let mut resolver = Resolver {
        krate,
        references,
        links: &links,
        substitutions: &substitutions,
        diagnostics: Vec::new(),
        layout_spans: BTreeSet::new(),
    };
    resolver.blocks(&mut parsed.document.blocks);
    for mut diagnostic in parsed.diagnostics.into_iter().chain(resolver.diagnostics) {
        if diagnostic.message == "unsupported authored syntax: LINE_PREFIX"
            && diagnostic
                .span
                .is_some_and(|span| resolver.layout_spans.contains(&(span.start, span.end)))
        {
            continue;
        }
        diagnostic.code = DiagnosticCode::RustDocumentation;
        diagnostic.related_entity = Some(DiagnosticEntity::Item {
            package: package.into(),
            id: item.into(),
        });
        diagnostic.source = location
            .as_ref()
            .map(|location| DiagnosticSource::Repository {
                repository: location.repository.clone(),
                path: location.path.clone(),
            });
        // Decoded Markdown ranges index this document, rather than the Rust
        // source file named by the diagnostic's source evidence.
        diagnostic.span = None;
        diagnostic.related_spans.clear();
        diagnostics.push(diagnostic);
    }
    SourcedDocument {
        document: parsed.document,
        source_format: DocumentFormat::Extracted {
            name: "rustdoc-markdown".into(),
        },
        source_location: location,
        raw_source: Some(docs.into()),
        generated_source: None,
        provenance: Vec::new(),
    }
}

struct Resolver<'a> {
    krate: &'a rd::Crate,
    references: &'a BTreeMap<rd::Id, ItemReference>,
    links: &'a BTreeMap<String, rd::Id>,
    substitutions: &'a BTreeMap<String, rd::Id>,
    diagnostics: Vec<Diagnostic>,
    layout_spans: BTreeSet<(usize, usize)>,
}

impl Resolver<'_> {
    fn target(&self, id: rd::Id) -> Option<String> {
        if let Some(reference) = self.references.get(&id) {
            return Some(format!("@ref {}::{}", reference.package, reference.item));
        }
        external_url(self.krate, id)
    }

    fn blocks(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                    self.inlines(inlines)
                }
                Block::Container { blocks, .. }
                | Block::Component { blocks, .. }
                | Block::Tabset { blocks, .. }
                | Block::BlockQuote { blocks, .. }
                | Block::Callout { blocks, .. } => self.blocks(blocks),
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&mut item.blocks);
                    }
                }
                Block::Table { caption, rows, .. } => {
                    self.inlines(caption);
                    for row in rows {
                        for cell in &mut row.cells {
                            self.blocks(&mut cell.blocks);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn inlines(&mut self, inlines: &mut [Inline]) {
        for inline in inlines {
            match inline {
                Inline::Link {
                    target, inlines, ..
                } => {
                    let id = self
                        .substitutions
                        .get(target)
                        .or_else(|| self.links.get(target));
                    if let Some(id) = id {
                        if let Some(resolved) = self.target(*id) {
                            *target = resolved;
                        } else {
                            self.diagnostics.push(Diagnostic::new(DiagnosticCode::RustDocumentation, Severity::Warning, "A compiler-resolved Rust link has no public or external documentation target."));
                            let span = inline_span(inline);
                            let value = inline_label(inline);
                            *inline = Inline::Code { value, span };
                            continue;
                        }
                    }
                    self.inlines(inlines);
                }
                Inline::SemanticReference {
                    target,
                    span,
                    target_span,
                } => {
                    let label = target.clone();
                    let link = self.links.get(target).and_then(|id| self.target(*id));
                    if let Some(target) = link {
                        *inline = Inline::Link {
                            inlines: vec![Inline::Code {
                                value: label,
                                span: *target_span,
                            }],
                            target,
                            title: None,
                            attributes: Attributes::default(),
                            span: *span,
                        };
                    } else {
                        // Rustdoc is the authority for Rust links. An unresolved
                        // shortcut must not accidentally bind to another API.
                        *inline = Inline::Code {
                            value: label,
                            span: *span,
                        };
                    }
                }
                Inline::Emphasis { inlines, .. }
                | Inline::Strong { inlines, .. }
                | Inline::Strikeout { inlines, .. }
                | Inline::Styled { inlines, .. }
                | Inline::Citation { inlines, .. } => self.inlines(inlines),
                Inline::Image { alt, .. } => self.inlines(alt),
                Inline::Unsupported {
                    source_kind,
                    raw,
                    span,
                } if source_kind == "LINE_PREFIX" && raw.trim().is_empty() => {
                    self.layout_spans.insert((span.start, span.end));
                    *inline = Inline::Text {
                        value: raw.clone(),
                        span: *span,
                    };
                }
                _ => {}
            }
        }
    }
}

fn inline_span(inline: &Inline) -> SourceSpan {
    match inline {
        Inline::Link { span, .. } => *span,
        _ => unreachable!(),
    }
}

fn inline_label(inline: &Inline) -> String {
    match inline {
        Inline::Link { inlines, .. }
        | Inline::Emphasis { inlines, .. }
        | Inline::Strong { inlines, .. }
        | Inline::Strikeout { inlines, .. }
        | Inline::Styled { inlines, .. }
        | Inline::Citation { inlines, .. } => inlines.iter().map(inline_label).collect(),
        Inline::Text { value, .. } | Inline::Code { value, .. } => value.clone(),
        Inline::Space { .. }
        | Inline::SoftBreak { .. }
        | Inline::HardBreak { .. }
        | Inline::NonbreakingSpace { .. } => " ".into(),
        Inline::SemanticReference { target, .. } => target.clone(),
        _ => String::new(),
    }
}

fn prepare_blocks(blocks: &mut [Block], identifiers: &mut BTreeSet<String>) {
    for block in blocks {
        match block {
            Block::Heading {
                attributes,
                inlines,
                span,
                ..
            } => {
                if let Some(identifier) = &attributes.identifier {
                    identifiers.insert(identifier.value.clone());
                    continue;
                }
                let label: String = inlines.iter().map(inline_label).collect();
                let slug = label
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
                let base = if slug.is_empty() {
                    "section".into()
                } else {
                    slug
                };
                let mut identifier = base.clone();
                let mut suffix = 2;
                while !identifiers.insert(identifier.clone()) {
                    identifier = format!("{base}-{suffix}");
                    suffix += 1;
                }
                attributes.identifier = Some(SpannedString {
                    value: identifier,
                    span: *span,
                });
            }
            Block::CodeBlock {
                language,
                source,
                source_segments,
                ..
            } if rust_fence(language.as_deref()) => {
                *language = Some("rust".into());
                let mut kept = Vec::new();
                let mut offset = 0;
                for line in source.split_inclusive('\n') {
                    let trimmed = line.trim_start_matches([' ', '\t']);
                    let indentation = line.len() - trimmed.len();
                    if !trimmed.starts_with("# ") && trimmed.trim_end_matches(['\n', '\r']) != "#" {
                        if trimmed.starts_with("##") {
                            if indentation > 0 {
                                kept.push(offset..offset + indentation);
                            }
                            kept.push(offset + indentation + 1..offset + line.len());
                        } else {
                            kept.push(offset..offset + line.len());
                        }
                    }
                    offset += line.len();
                }
                let mut visible = Vec::new();
                let mut decoded = 0;
                for segment in source_segments.iter() {
                    for range in &kept {
                        let start = range.start.max(decoded);
                        let end = range.end.min(decoded + segment.text.len());
                        if start < end {
                            visible.push(SourceSegment {
                                text: segment.text[start - decoded..end - decoded].into(),
                                span: SourceSpan {
                                    start: segment.span.start + start - decoded,
                                    end: segment.span.start + end - decoded,
                                },
                            });
                        }
                    }
                    decoded += segment.text.len();
                }
                *source = visible
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect();
                *source_segments = visible;
            }
            Block::Container { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::BlockQuote { blocks, .. }
            | Block::Callout { blocks, .. } => prepare_blocks(blocks, identifiers),
            Block::List { items, .. } => {
                for item in items {
                    prepare_blocks(&mut item.blocks, identifiers);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        prepare_blocks(&mut cell.blocks, identifiers);
                    }
                }
            }
            _ => {}
        }
    }
}

fn rust_fence(language: Option<&str>) -> bool {
    language.is_none_or(|language| {
        language.split([',', ' ']).all(|attribute| {
            matches!(
                attribute,
                "rust"
                    | "no_run"
                    | "should_panic"
                    | "compile_fail"
                    | "ignore"
                    | "edition2015"
                    | "edition2018"
                    | "edition2021"
                    | "edition2024"
                    | "standalone_crate"
            ) || attribute.starts_with("ignore-")
                || attribute.starts_with('E')
                    && attribute.len() == 5
                    && attribute[1..].bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

fn external_url(krate: &rd::Crate, id: rd::Id) -> Option<String> {
    let summary = krate.paths.get(&id)?;
    summary_url(krate, summary)
}

fn summary_url(krate: &rd::Crate, summary: &rd::ItemSummary) -> Option<String> {
    if summary.path.len() > 1 {
        let parent_path = &summary.path[..summary.path.len() - 1];
        if let Some(parent) = krate
            .paths
            .values()
            .find(|candidate| candidate.path == parent_path)
        {
            let anchor = match summary.kind {
                rd::ItemKind::Variant => Some("variant"),
                rd::ItemKind::AssocConst => Some("associatedconstant"),
                rd::ItemKind::AssocType => Some("associatedtype"),
                rd::ItemKind::StructField => Some("structfield"),
                rd::ItemKind::Function
                    if matches!(
                        parent.kind,
                        rd::ItemKind::Struct
                            | rd::ItemKind::Enum
                            | rd::ItemKind::Union
                            | rd::ItemKind::Trait
                            | rd::ItemKind::Primitive
                            | rd::ItemKind::TypeAlias
                    ) =>
                {
                    Some("method")
                }
                _ => None,
            };
            if let Some(anchor) = anchor {
                return Some(format!(
                    "{}#{anchor}.{}",
                    summary_url(krate, parent)?,
                    summary.path.last()?
                ));
            }
        }
    }
    let dependency = krate.external_crates.get(&summary.crate_id)?;
    let first = summary.path.first()?;
    let standard = matches!(
        first.as_str(),
        "std" | "core" | "alloc" | "proc_macro" | "test"
    );
    let mut root = if standard {
        format!("https://doc.rust-lang.org/{first}/")
    } else {
        dependency.html_root_url.clone().unwrap_or_else(|| {
            format!(
                "https://docs.rs/{}/latest/{}/",
                dependency.name, dependency.name
            )
        })
    };
    if !standard && root.trim_end_matches('/').rsplit('/').next() != Some(dependency.name.as_str())
    {
        root = format!("{}/{}", root.trim_end_matches('/'), dependency.name);
    }
    let mut path = summary.path.clone();
    if standard || first == &dependency.name {
        path.remove(0);
    }
    if summary.kind == rd::ItemKind::Module {
        path.push("index.html".into());
    } else {
        let name = path.pop()?;
        let prefix = match summary.kind {
            rd::ItemKind::Struct => "struct",
            rd::ItemKind::Enum => "enum",
            rd::ItemKind::Union => "union",
            rd::ItemKind::Trait => "trait",
            rd::ItemKind::TraitAlias => "traitalias",
            rd::ItemKind::Function => "fn",
            rd::ItemKind::Constant => "constant",
            rd::ItemKind::Static => "static",
            rd::ItemKind::TypeAlias => "type",
            rd::ItemKind::Macro => "macro",
            rd::ItemKind::ProcAttribute => "attr",
            rd::ItemKind::ProcDerive => "derive",
            rd::ItemKind::Primitive => "primitive",
            rd::ItemKind::Keyword => "keyword",
            _ => return None,
        };
        path.push(format!("{prefix}.{name}.html"));
    }
    Some(format!("{}/{}", root.trim_end_matches('/'), path.join("/")))
}
