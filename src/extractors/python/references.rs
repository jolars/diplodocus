use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSource, Severity};
use crate::ir::*;

use super::docstrings::DocstringParse;

pub(super) struct PythonReferences {
    names: BTreeMap<String, BTreeSet<String>>,
    inherited: BTreeMap<String, BTreeSet<String>>,
    canonical: BTreeMap<String, String>,
}

impl PythonReferences {
    pub(super) fn new(items: &BTreeMap<String, Item>) -> Self {
        let mut names = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, item) in items {
            if !item.discoverable() {
                continue;
            }
            for name in std::iter::once(&item.qualified_name)
                .chain(item.aliases.iter().map(|alias| &alias.qualified_name))
            {
                names.entry(name.clone()).or_default().insert(id.clone());
            }
        }
        let mut inherited = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, item) in items
            .iter()
            .filter(|(_, item)| item.kind == ItemKind::Class)
        {
            let members = class_members(id, items, &names, &mut BTreeSet::new());
            for class_name in std::iter::once(&item.qualified_name)
                .chain(item.aliases.iter().map(|alias| &alias.qualified_name))
            {
                for (member_name, targets) in &members {
                    inherited
                        .entry(format!("{class_name}.{member_name}"))
                        .or_default()
                        .extend(targets.iter().cloned());
                }
            }
        }
        Self {
            names,
            inherited,
            canonical: items
                .iter()
                .map(|(id, item)| (id.clone(), item.qualified_name.clone()))
                .collect(),
        }
    }

    pub(super) fn normalize(&self, parsed: &mut DocstringParse, owner: &str) {
        let mut diagnostics = vec![];
        self.blocks(
            &mut parsed.document.document.blocks,
            owner,
            &mut diagnostics,
        );
        let source = parsed
            .document
            .source_location
            .as_ref()
            .expect("extracted source");
        for mut diagnostic in diagnostics {
            diagnostic.source = Some(DiagnosticSource::Repository {
                repository: source.repository.clone(),
                path: source.path.clone(),
            });
            diagnostic.span = diagnostic
                .span
                .and_then(|span| {
                    parsed.source_map.iter().find_map(|segment| {
                        (segment.decoded.start <= span.start && segment.decoded.end >= span.end)
                            .then(|| SourceSpan {
                                start: segment.source.start + span.start - segment.decoded.start,
                                end: segment.source.start + span.end - segment.decoded.start,
                            })
                    })
                })
                .or(source.span);
            parsed.diagnostics.push(diagnostic);
        }
        parsed.diagnostics.sort();
        parsed.diagnostics.dedup();
    }

    fn target(&self, owner: &str, spelling: &str) -> Result<Option<String>, DiagnosticCode> {
        let mut candidates = vec![spelling.to_owned()];
        let mut context = owner;
        candidates.push(format!("{context}.{spelling}"));
        while let Some((prefix, _)) = context.rsplit_once('.') {
            candidates.push(format!("{prefix}.{spelling}"));
            context = prefix;
        }
        for name in candidates {
            match self.names.get(&name).map(BTreeSet::len) {
                Some(1) => return Ok(Some(name)),
                Some(_) => return Err(DiagnosticCode::AmbiguousItemReference),
                None => match self.inherited.get(&name) {
                    Some(targets) if targets.len() == 1 => {
                        return Ok(Some(self.canonical[targets.first().unwrap()].clone()));
                    }
                    Some(_) => return Err(DiagnosticCode::AmbiguousItemReference),
                    None => {}
                },
            }
        }
        Ok(None)
    }

    fn blocks(&self, blocks: &mut [Block], owner: &str, diagnostics: &mut Vec<Diagnostic>) {
        for block in blocks {
            match block {
                Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                    self.inlines(inlines, owner, diagnostics)
                }
                Block::BlockQuote { blocks, .. }
                | Block::Container { blocks, .. }
                | Block::Component { blocks, .. }
                | Block::Tabset { blocks, .. }
                | Block::Callout { blocks, .. } => self.blocks(blocks, owner, diagnostics),
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&mut item.blocks, owner, diagnostics);
                    }
                }
                Block::Table { caption, rows, .. } => {
                    self.inlines(caption, owner, diagnostics);
                    for row in rows {
                        for cell in &mut row.cells {
                            self.blocks(&mut cell.blocks, owner, diagnostics);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn inlines(&self, inlines: &mut [Inline], owner: &str, diagnostics: &mut Vec<Diagnostic>) {
        for inline in inlines {
            let reference = match inline {
                Inline::SemanticReference {
                    target,
                    span,
                    target_span,
                } => Some((
                    target.clone(),
                    *span,
                    vec![Inline::Code {
                        value: target.clone(),
                        span: *target_span,
                    }],
                )),
                Inline::Link {
                    target,
                    inlines,
                    span,
                    ..
                } if target.starts_with("@ref ") => {
                    Some((target[5..].to_owned(), *span, inlines.clone()))
                }
                _ => None,
            };
            if let Some((spelling, span, label)) = reference {
                // Explicit workspace qualifiers can refer to another package,
                // whose surface is not available until workspace assembly.
                if spelling.contains("::") {
                    continue;
                }
                match self.target(owner, &spelling) {
                    Ok(Some(target)) => {
                        if target != spelling {
                            *inline = Inline::Link {
                                inlines: label,
                                target: format!("@ref {target}"),
                                title: None,
                                attributes: Attributes::default(),
                                span,
                            };
                        }
                    }
                    result => {
                        let ambiguous = result.is_err();
                        let mut diagnostic = Diagnostic::new(
                            if ambiguous {
                                DiagnosticCode::AmbiguousItemReference
                            } else {
                                DiagnosticCode::PythonUnsupportedDocstring
                            },
                            if ambiguous {
                                Severity::Error
                            } else {
                                Severity::Warning
                            },
                            if ambiguous {
                                format!(
                                    "Python documentation reference `{spelling}` is ambiguous in `{owner}`."
                                )
                            } else {
                                format!(
                                    "Python documentation reference `{spelling}` in `{owner}` has no public local target; rendered as code."
                                )
                            },
                        );
                        diagnostic.span = Some(span);
                        diagnostics.push(diagnostic);
                        let value = label
                            .iter()
                            .map(|inline| match inline {
                                Inline::Code { value, .. } | Inline::Text { value, .. } => {
                                    value.as_str()
                                }
                                _ => "",
                            })
                            .collect();
                        *inline = Inline::Code { value, span };
                    }
                }
                continue;
            }
            match inline {
                Inline::Link { inlines, .. }
                | Inline::Emphasis { inlines, .. }
                | Inline::Strong { inlines, .. }
                | Inline::Styled { inlines, .. }
                | Inline::Citation { inlines, .. }
                | Inline::Strikeout { inlines, .. } => self.inlines(inlines, owner, diagnostics),
                Inline::Image { alt, .. } => self.inlines(alt, owner, diagnostics),
                _ => {}
            }
        }
    }
}

fn class_members(
    id: &str,
    items: &BTreeMap<String, Item>,
    names: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut members = BTreeMap::new();
    for owner in class_order(id, items, names, visited) {
        for child in &items[&owner].children {
            let member = &items[child];
            if member.discoverable() {
                members
                    .entry(member.name.clone())
                    .or_insert_with(|| BTreeSet::from([child.clone()]));
            }
        }
    }
    members
}

fn class_order(
    id: &str,
    items: &BTreeMap<String, Item>,
    names: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut BTreeSet<String>,
) -> Vec<String> {
    if !visited.insert(id.into()) {
        return vec![];
    }
    let item = &items[id];
    let mut direct = vec![];
    if let Some(ItemLanguageData::Python(data)) = &item.language_data
        && let PythonDeclaration::Class { bases, .. } = &data.declaration
    {
        for base in bases {
            let base = match base {
                SignatureExpression::Apply { constructor, .. } => constructor.as_ref(),
                base => base,
            };
            let SignatureExpression::Name { name, .. } = base else {
                continue;
            };
            let Some(targets) = names.get(name) else {
                continue;
            };
            if let Some(target) = targets.first().filter(|_| targets.len() == 1)
                && items[target].kind == ItemKind::Class
            {
                direct.push(target.clone());
            }
        }
    }
    let mut sequences: Vec<_> = direct
        .iter()
        .map(|base| class_order(base, items, names, visited))
        .collect();
    sequences.push(direct);
    let mut result = vec![id.into()];
    // C3 keeps a sibling's override ahead of a shared ancestor, matching Python.
    while sequences.iter().any(|sequence| !sequence.is_empty()) {
        let candidate = sequences
            .iter()
            .filter_map(|sequence| sequence.first())
            .find(|head| {
                !sequences
                    .iter()
                    .any(|sequence| sequence.iter().skip(1).any(|tail| tail == *head))
            })
            .cloned();
        let Some(candidate) = candidate else {
            break;
        };
        if !result.contains(&candidate) {
            result.push(candidate.clone());
        }
        for sequence in &mut sequences {
            if sequence.first() == Some(&candidate) {
                sequence.remove(0);
            }
        }
    }
    visited.remove(id);
    result
}
