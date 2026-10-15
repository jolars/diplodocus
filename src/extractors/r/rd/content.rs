use rd_ast::{RdColumnAlign, RdLinkDestination, RdLinkTopic, RdTagged};

use super::*;
use crate::ir::{TableAlignment, TableCell, TableRow};

const DOI_PATH: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'#')
    .add(b'?')
    .add(b'%');

impl Context<'_> {
    pub(super) fn describe_blocks(&mut self, cursor: &RdNodeRef<'_>) -> Vec<Block> {
        let mut blocks = vec![];
        let mut items = vec![];
        let list = cursor.inspect_list().expect("describe container").unwrap();
        let mut entries = list.items();
        let children = cursor.children();
        for (index, child) in children.iter().enumerate() {
            let node = child.node();
            let child_path = child.path();
            if matches!(node, RdNode::Comment(_))
                || matches!(node, RdNode::Text(text) if text.trim().is_empty())
            {
                continue;
            }
            let entry = entries.next().expect("nontrivia describe entry");
            if matches!(node, RdNode::Tagged(item) if item.tag() == &RdTag::Item) {
                match entry {
                    Ok(RdListItem::Described(item)) => {
                        let mut body = vec![Block::Paragraph {
                            inlines: self.inlines(item.label_ref()),
                            span: self.span(),
                        }];
                        body.extend(self.blocks(item.body_ref()));
                        items.push(ListItem {
                            checked: None,
                            blocks: body,
                            span: self.span(),
                        });
                    }
                    Err(error) => {
                        self.shape(error);
                        items.push(ListItem {
                            checked: None,
                            blocks: vec![self.unsupported_block("list-item", child_path)],
                            span: self.span(),
                        });
                    }
                    _ => unreachable!("describe item"),
                }
            } else {
                // R permits prose between described entries; preserve its position.
                if !items.is_empty() {
                    blocks.push(Block::List {
                        ordered: false,
                        items: std::mem::take(&mut items),
                        span: self.span(),
                    });
                }
                blocks.extend(self.blocks(children.slice(index..index + 1).unwrap()));
            }
        }
        if !items.is_empty() {
            blocks.push(Block::List {
                ordered: false,
                items,
                span: self.span(),
            });
        }
        blocks
    }

    pub(super) fn content_block(&mut self, cursor: &RdNodeRef<'_>) -> Vec<Block> {
        let tagged = cursor.node().as_tagged().expect("block markup");
        let path = cursor.path();
        let span = self.span();
        match tagged.tag() {
            RdTag::Tabular => match cursor.inspect_tabular() {
                Ok(Some(table)) => {
                    for error in table.diagnostics() {
                        self.error(DiagnosticCode::RRdInformationLoss, error.to_string());
                    }
                    vec![Block::Table {
                        caption: vec![],
                        alignments: table
                            .columns()
                            .iter()
                            .map(|column| match column {
                                RdColumnAlign::Left => TableAlignment::Left,
                                RdColumnAlign::Center => TableAlignment::Center,
                                RdColumnAlign::Right => TableAlignment::Right,
                                _ => TableAlignment::Default,
                            })
                            .collect(),
                        rows: table
                            .rows()
                            .iter()
                            .map(|row| TableRow {
                                header: false,
                                cells: row
                                    .cells()
                                    .iter()
                                    .map(|cell| TableCell {
                                        blocks: self.blocks(cell.nodes_ref()),
                                        span,
                                    })
                                    .collect(),
                                span,
                            })
                            .collect(),
                        span,
                    }]
                }
                Ok(None) => unreachable!("tabular node"),
                Err(error) => {
                    self.shape(error);
                    vec![self.unsupported_block("tabular", path)]
                }
            },
            RdTag::Deqn => match self.equation(cursor) {
                Some((latex, fallback)) => vec![Block::DisplayMath {
                    latex,
                    fallback,
                    span,
                }],
                None => vec![self.unsupported_block("deqn", path)],
            },
            RdTag::Section | RdTag::Subsection | RdTag::Item => {
                let children = tagged.children();
                if tagged.option().is_some()
                    || children.len() != 2
                    || children.iter().any(|node| node.as_group().is_none())
                {
                    self.error(
                        DiagnosticCode::RRdInformationLoss,
                        format!("Rd {} requires a label and body.", tagged.tag().as_rd_tag()),
                    );
                    return vec![self.unsupported_block(tagged.tag().as_rd_tag(), path)];
                }
                let label = cursor.children().get(0).unwrap().children();
                let body = cursor.children().get(1).unwrap().children();
                if tagged.tag() == &RdTag::Item {
                    let mut inlines = self.inlines(label);
                    code_style(&mut inlines);
                    let mut blocks = vec![Block::Paragraph { inlines, span }];
                    blocks.extend(self.blocks(body));
                    vec![Block::List {
                        ordered: false,
                        items: vec![ListItem {
                            checked: None,
                            blocks,
                            span,
                        }],
                        span,
                    }]
                } else {
                    let mut blocks = vec![Block::Heading {
                        level: if tagged.tag() == &RdTag::Section {
                            2
                        } else {
                            3
                        },
                        attributes: Attributes::default(),
                        inlines: self.inlines(label),
                        span,
                    }];
                    blocks.extend(self.blocks(body));
                    blocks
                }
            }
            _ => vec![self.unsupported_block(tagged.tag().as_rd_tag(), path)],
        }
    }

    fn equation(&mut self, cursor: &RdNodeRef<'_>) -> Option<(String, Option<String>)> {
        match cursor.inspect_equation() {
            Ok(Some(equation)) => {
                let fallback = match equation.ascii() {
                    Some(nodes) => Some(plain(nodes)?),
                    None => None,
                };
                Some((plain(equation.latex())?, fallback))
            }
            Ok(None) => unreachable!("equation node"),
            Err(error) => {
                self.shape(error);
                None
            }
        }
    }

    pub(super) fn equation_inline(&mut self, cursor: &RdNodeRef<'_>) -> Inline {
        match self.equation(cursor) {
            Some((latex, fallback)) => Inline::Math {
                latex,
                fallback,
                span: self.span(),
            },
            None => self.unsupported_inline("eqn", cursor.path()),
        }
    }

    pub(super) fn help_link(&mut self, cursor: &RdNodeRef<'_>) -> Inline {
        let path = cursor.path();
        let link = match cursor.inspect_link() {
            Ok(Some(link)) => link,
            Ok(None) => unreachable!("link node"),
            Err(error) => {
                self.shape(error);
                return self.unsupported_inline("link", path);
            }
        };
        let target = match link.destination() {
            RdLinkDestination::DisplayText { nodes } => plain(nodes),
            RdLinkDestination::Explicit { topic } => Some((*topic).into()),
            RdLinkDestination::Package { package, topic } => {
                let topic = match topic {
                    RdLinkTopic::Explicit(topic) => Some((*topic).into()),
                    RdLinkTopic::DisplayText(nodes) => plain(nodes),
                    _ => None,
                };
                topic.map(|topic| format!("{package}::{topic}"))
            }
            _ => None,
        };
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            self.error(
                DiagnosticCode::RRdInformationLoss,
                "Rd link requires a nonempty topic.",
            );
            return self.unsupported_inline("link", path);
        };
        Inline::Link {
            inlines: self.inlines(link.display_ref()),
            target: format!("@ref {target}"),
            title: None,
            attributes: Attributes::default(),
            span: self.span(),
        }
    }

    pub(super) fn doi_link(&mut self, tagged: &RdTagged, path: &RdAstPath) -> Inline {
        match plain(tagged.children()).filter(|doi| {
            doi.starts_with("10.") && doi.contains('/') && !doi.chars().any(char::is_whitespace)
        }) {
            Some(doi) => Inline::Link {
                target: format!(
                    "https://doi.org/{}",
                    percent_encoding::utf8_percent_encode(&doi, DOI_PATH)
                ),
                inlines: vec![Inline::Text {
                    value: doi,
                    span: self.span(),
                }],
                title: None,
                attributes: Attributes::default(),
                span: self.span(),
            },
            None => {
                self.error(
                    DiagnosticCode::RRdInformationLoss,
                    "Rd DOI requires an identifier beginning with 10. and containing a slash.",
                );
                self.unsupported_inline("doi", path)
            }
        }
    }
}

pub(super) fn code_style(inlines: &mut [Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text { value, span } => {
                *inline = Inline::Code {
                    value: value.clone(),
                    span: *span,
                }
            }
            Inline::Link { inlines, .. }
            | Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. } => code_style(inlines),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_link_errors_retain_canonical_paths() {
        for content in [
            r"\arguments{\item{x}{\link{\emph{bad}}}}",
            r"\description{{\itemize{\item First\item {\link{\emph{bad}}}}}}",
            r"\description{\describe{\item{First}{Before.}Between.\item{Second}{\link{\emph{bad}}}}}",
            r"\details{\tabular{ll}{\tab \link{\emph{bad}}\cr}}",
        ] {
            let text = format!(r"\name{{topic}}\title{{Topic}}{content}");
            let parsed = rd_source::parse(text.as_bytes()).unwrap();
            let expected: Vec<_> = parsed
                .document()
                .walk()
                .filter_map(|cursor| cursor.inspect_link().err())
                .map(|error| error.to_string())
                .collect();
            assert_eq!(expected.len(), 1, "{content}");

            let source = SourceLocation {
                repository: "r".into(),
                path: "man/topic.Rd".try_into().unwrap(),
                span: None,
            };
            let mut diagnostics = vec![];
            assert!(parse(&text, &source, &mut diagnostics, "unix").is_some());
            let actual: Vec<_> = diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == DiagnosticCode::RRdInformationLoss)
                .map(|diagnostic| diagnostic.message.clone())
                .collect();
            assert_eq!(actual, expected, "{content}");
        }
    }
}
