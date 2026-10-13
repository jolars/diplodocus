use super::*;
use crate::paths::ResolvedRepositoryPaths;
use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::citationberg::{
    FontStyle, FontVariant, FontWeight, IndependentStyle, VerticalAlign,
};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, CitePurpose, Elem,
    ElemChild, ElemChildren, ElemMeta, Formatted, Library, LocatorPayload, SpecificLocator,
};

pub(crate) fn metadata<'a>(document: &'a Document, key: &str) -> Option<&'a MetadataValue> {
    let MetadataValue::Mapping { entries, .. } = document.frontmatter.as_ref()? else {
        return None;
    };
    entries
        .iter()
        .find(|e| e.key.value == key)
        .map(|e| &e.value)
}

pub(in crate::assembly) fn inputs(
    document: &Document,
    path: &Path,
    repository: &ResolvedRepositoryPaths,
) -> Result<Vec<PathBuf>, AssemblyError> {
    let mut result = Vec::new();
    for key in ["bibliography", "csl"] {
        let Some(value) = metadata(document, key) else {
            continue;
        };
        let values: Vec<_> = match value {
            MetadataValue::Scalar { value, .. } => vec![value],
            MetadataValue::Sequence { items, .. } if key == "bibliography" => items
                .iter()
                .map(|v| match v {
                    MetadataValue::Scalar { value, .. } => Ok(value),
                    _ => Err(AssemblyError::InputsChanged),
                })
                .collect::<Result<_, _>>()?,
            _ => {
                return Err(AssemblyError::Diagnostics(vec![diagnostic(
                    DiagnosticCode::UnsupportedAuthoredSyntax,
                    "Bibliography must name local files; csl must name one independent local style.",
                )]));
            }
        };
        for value in values {
            result.push(crate::paths::resolve_input_file(
                &repository.path,
                key,
                Path::new(value),
                path.parent().expect("article parent"),
            )?);
        }
    }
    Ok(result)
}

pub(in crate::assembly) fn resolve(
    document: &mut Document,
    path: &Path,
    repository: &ResolvedRepositoryPaths,
) -> Result<(), AssemblyError> {
    let files = inputs(document, path, repository)?;
    let error = |message: String| {
        AssemblyError::Diagnostics(vec![
            Diagnostic::new(
                DiagnosticCode::UnresolvedDocumentReference,
                Severity::Error,
                message,
            )
            .with_source(DiagnosticSource::Repository {
                repository: repository.id.clone(),
                path: repository
                    .source_location(path, None)
                    .expect("article source")
                    .path,
            }),
        ])
    };
    let mut library = Library::new();
    let custom_style = metadata(document, "csl").is_some();
    let mut style = None;
    for (i, file) in files.iter().enumerate() {
        let text = fs::read_to_string(file)?;
        if custom_style && i == files.len() - 1 {
            style = Some(
                IndependentStyle::from_xml(&text)
                    .map_err(|e| error(format!("Invalid independent CSL style: {e}")))?,
            );
        } else {
            let parsed = hayagriva::io::from_biblatex_str(&text)
                .map_err(|e| error(format!("Invalid BibTeX bibliography: {e:?}")))?;
            for entry in parsed.iter() {
                if library.get(entry.key()).is_some() {
                    return Err(error(format!(
                        "Duplicate bibliography key: {}",
                        entry.key()
                    )));
                }
                library.push(entry);
            }
        }
    }
    let style = style.unwrap_or_else(
        || match hayagriva::archive::ArchivedStyle::ChicagoAuthorDate.get() {
            hayagriva::citationberg::Style::Independent(style) => style,
            _ => unreachable!(),
        },
    );
    let locales = hayagriva::archive::locales();
    let mut specs = Vec::new();
    visit(&mut document.blocks, &mut |inline| {
        if let Inline::Citation {
            raw, keys, span, ..
        } = inline
        {
            specs.push((raw.clone(), keys.clone(), *span));
        }
    });
    if specs.is_empty() {
        return Ok(());
    }
    let locator_regex =
        regex::Regex::new(r",\s*(?:pp?\.|pages?)\s*([\d–—-]+)").expect("locator pattern");
    let locators: Vec<Vec<Option<String>>> = specs
        .iter()
        .map(|(raw, keys, _)| {
            keys.iter()
                .map(|key| {
                    let after = raw
                        .split_once(&format!("@{key}"))
                        .map(|(_, s)| s.split(';').next().unwrap_or(s));
                    after
                        .and_then(|s| locator_regex.captures(s))
                        .and_then(|c| c.get(1))
                        .map(|s| s.as_str().to_owned())
                })
                .collect()
        })
        .collect();
    let mut driver = BibliographyDriver::new();
    for (i, (raw, keys, _)) in specs.iter().enumerate() {
        let mut items = Vec::new();
        for (j, key) in keys.iter().enumerate() {
            let entry = library
                .get(key)
                .ok_or_else(|| error(format!("Unknown bibliography key: {key}")))?;
            let mut item = CitationItem::with_entry(entry);
            if raw.contains(&format!("-@{key}")) {
                item.purpose = Some(CitePurpose::Year);
            } else if !raw.starts_with('[') {
                item.purpose = Some(CitePurpose::Prose);
            }
            item.locator = locators[i][j]
                .as_ref()
                .map(|s| SpecificLocator(Locator::Page, LocatorPayload::Str(s)));
            items.push(item);
        }
        driver.citation(CitationRequest::new(
            items,
            &style,
            None,
            &locales,
            Some(i + 1),
        ));
    }
    let result = driver.finish(BibliographyRequest {
        style: &style,
        locale: None,
        locale_files: &locales,
    });
    let mut rendered = result
        .citations
        .into_iter()
        .zip(&specs)
        .map(|(citation, (raw, keys, span))| {
            let mut children = elements(&citation.citation, keys, *span);
            if raw.starts_with('[') && raw.contains("-@") {
                children.insert(
                    0,
                    Inline::Text {
                        value: style.citation.layout.prefix.clone().unwrap_or_default(),
                        span: *span,
                    },
                );
                children.push(Inline::Text {
                    value: style.citation.layout.suffix.clone().unwrap_or_default(),
                    span: *span,
                });
            }
            if let Some(number) = citation.note_number {
                let note = Block::Container {
                    kind: DocumentContainerKind::Footnote,
                    identifier: Some(format!("cite-note-{number}")),
                    blocks: vec![Block::Paragraph {
                        inlines: children,
                        span: *span,
                    }],
                    span: *span,
                };
                (
                    vec![Inline::Styled {
                        style: InlineStyle::Superscript,
                        inlines: vec![link(
                            format!("#cite-note-{number}"),
                            vec![Inline::Text {
                                value: number.to_string(),
                                span: *span,
                            }],
                            *span,
                        )],
                        span: *span,
                    }],
                    Some(note),
                )
            } else {
                (children, None)
            }
        })
        .collect::<Vec<_>>()
        .into_iter();
    let mut notes = Vec::new();
    visit(&mut document.blocks, &mut |inline| {
        if let Inline::Citation { inlines, .. } = inline
            && let Some((children, note)) = rendered.next()
        {
            *inlines = children;
            if let Some(note) = note {
                notes.push(note);
            }
        }
    });
    if !notes.is_empty() {
        document.blocks.push(Block::Container {
            kind: DocumentContainerKind::Footnotes,
            identifier: None,
            blocks: notes,
            span: document.span,
        });
    }
    if let Some(bibliography) = result.bibliography
        && !bibliography.items.is_empty()
    {
        let heading = document.blocks.iter().position(|b| matches!(b, Block::Heading { inlines, .. } if authored::inline_text(inlines).eq_ignore_ascii_case("references")));
        let heading = heading.unwrap_or_else(|| {
            document.blocks.push(Block::Heading {
                level: 2,
                attributes: Attributes {
                    identifier: Some(SpannedString {
                        value: "references".into(),
                        span: document.span,
                    }),
                    ..Attributes::default()
                },
                inlines: vec![Inline::Text {
                    value: "References".into(),
                    span: document.span,
                }],
                span: document.span,
            });
            document.blocks.len() - 1
        });
        let insertion = document
            .blocks
            .iter()
            .enumerate()
            .skip(heading + 1)
            .find(|(_, block)| {
                matches!(
                    block,
                    Block::Heading { .. }
                        | Block::Container {
                            kind: DocumentContainerKind::Footnotes,
                            ..
                        }
                )
            })
            .map(|(i, _)| i)
            .unwrap_or(document.blocks.len());
        let mut entries = Vec::new();
        for entry in bibliography.items {
            let mut content = entry.content;
            if let Some(first) = entry.first_field {
                content.0.insert(0, first);
            }
            entries.push(Block::Container {
                kind: DocumentContainerKind::BibliographyEntry,
                identifier: Some(reference_id(&entry.key)),
                blocks: vec![Block::Paragraph {
                    inlines: elements(&content, &[], document.span),
                    span: document.span,
                }],
                span: document.span,
            });
        }
        document.blocks.splice(insertion..insertion, entries);
    }
    Ok(())
}

fn reference_id(key: &str) -> String {
    format!(
        "ref-{}",
        key.as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn link(target: String, inlines: Vec<Inline>, span: SourceSpan) -> Inline {
    Inline::Link {
        target,
        inlines,
        title: None,
        attributes: Attributes::default(),
        span,
    }
}
fn elements(children: &ElemChildren, keys: &[String], span: SourceSpan) -> Vec<Inline> {
    children
        .0
        .iter()
        .flat_map(|child| match child {
            ElemChild::Text(text) => vec![formatted(text, span)],
            ElemChild::Link { text, url } => {
                vec![link(url.clone(), vec![formatted(text, span)], span)]
            }
            ElemChild::Elem(Elem { children, meta, .. }) => {
                let children = elements(children, keys, span);
                if let Some(ElemMeta::Entry(i)) = meta
                    && let Some(key) = keys.get(*i)
                {
                    vec![link(format!("#{}", reference_id(key)), children, span)]
                } else {
                    children
                }
            }
            ElemChild::Markup(text) => vec![Inline::Text {
                value: text.clone(),
                span,
            }],
            ElemChild::Transparent { .. } => Vec::new(),
        })
        .collect()
}
fn formatted(text: &Formatted, span: SourceSpan) -> Inline {
    let mut value = Inline::Text {
        value: text.text.clone(),
        span,
    };
    if matches!(text.formatting.font_style, FontStyle::Italic) {
        value = Inline::Emphasis {
            inlines: vec![value],
            span,
        };
    }
    if text.formatting.font_weight == FontWeight::Bold {
        value = Inline::Strong {
            inlines: vec![value],
            span,
        };
    }
    if text.formatting.font_variant == FontVariant::SmallCaps {
        value = Inline::Styled {
            style: InlineStyle::SmallCaps,
            inlines: vec![value],
            span,
        };
    }
    let style = match text.formatting.vertical_align {
        VerticalAlign::Sup => Some(InlineStyle::Superscript),
        VerticalAlign::Sub => Some(InlineStyle::Subscript),
        _ => None,
    };
    if let Some(style) = style {
        value = Inline::Styled {
            style,
            inlines: vec![value],
            span,
        };
    }
    value
}

fn visit(blocks: &mut [Block], f: &mut impl FnMut(&mut Inline)) {
    for block in blocks {
        match block {
            Block::Paragraph { inlines: nodes, .. } | Block::Heading { inlines: nodes, .. } => {
                inlines_visit(nodes, f)
            }
            Block::Container { blocks, .. }
            | Block::BlockQuote { blocks, .. }
            | Block::Callout { blocks, .. } => visit(blocks, f),
            Block::List { items, .. } => {
                for item in items {
                    visit(&mut item.blocks, f);
                }
            }
            Block::Table { rows, caption, .. } => {
                inlines_visit(caption, f);
                for row in rows {
                    for cell in &mut row.cells {
                        visit(&mut cell.blocks, f);
                    }
                }
            }
            _ => {}
        }
    }
}
fn inlines_visit(nodes: &mut [Inline], f: &mut impl FnMut(&mut Inline)) {
    for node in nodes {
        f(node);
        match node {
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. }
            | Inline::Styled { inlines, .. }
            | Inline::Link { inlines, .. } => inlines_visit(inlines, f),
            Inline::Image { alt, .. } => inlines_visit(alt, f),
            _ => {}
        }
    }
}
