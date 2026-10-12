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
    pub(super) fn content_block(&mut self, tagged: &RdTagged, path: &RdPath) -> Vec<Block> {
        let span = self.span();
        match tagged.tag() {
            RdTag::Tabular => {
                let mut table = tagged.clone();
                if let Some(body) = tagged.children().get(1).and_then(RdNode::as_group) {
                    let mut nodes = body.children().to_vec();
                    let end = nodes.iter().rposition(|node| {
                        !matches!(node, RdNode::Text(text) if text.trim().is_empty())
                            && !matches!(node, RdNode::Comment(_))
                    });
                    // R ignores whitespace after the final row separator.
                    if end.is_some_and(|index| matches!(&nodes[index], RdNode::Tagged(tagged) if tagged.tag() == &RdTag::Cr)) {
                        nodes.truncate(end.unwrap() + 1);
                        let mut children = tagged.children().to_vec();
                        children[1] = RdNode::group(nodes);
                        table = RdTagged::new(tagged.tag().clone(), tagged.option().map(<[RdNode]>::to_vec), children);
                    }
                }
                match table.inspect_tabular(path) {
                    Ok(table) => {
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
                                            blocks: self.blocks(cell.nodes(), cell.path()),
                                            span,
                                        })
                                        .collect(),
                                    span,
                                })
                                .collect(),
                            span,
                        }]
                    }
                    Err(error) => {
                        self.shape(error);
                        vec![self.unsupported_block("tabular", path)]
                    }
                }
            }
            RdTag::Deqn => match self.equation(tagged, path) {
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
                let label = children[0].as_group().unwrap().children();
                let body = children[1].as_group().unwrap().children();
                if tagged.tag() == &RdTag::Item {
                    let mut inlines = self.inlines(label, path);
                    code_style(&mut inlines);
                    let mut blocks = vec![Block::Paragraph { inlines, span }];
                    blocks.extend(self.blocks(body, path));
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
                        inlines: self.inlines(label, path),
                        span,
                    }];
                    blocks.extend(self.blocks(body, path));
                    blocks
                }
            }
            _ => vec![self.unsupported_block(tagged.tag().as_rd_tag(), path)],
        }
    }

    fn equation(&mut self, tagged: &RdTagged, path: &RdPath) -> Option<(String, Option<String>)> {
        match tagged.inspect_equation(path) {
            Ok(equation) => {
                let fallback = match equation.ascii() {
                    Some(nodes) => Some(plain(nodes)?),
                    None => None,
                };
                Some((plain(equation.latex())?, fallback))
            }
            Err(error) => {
                self.shape(error);
                None
            }
        }
    }

    pub(super) fn equation_inline(&mut self, tagged: &RdTagged, path: &RdPath) -> Inline {
        match self.equation(tagged, path) {
            Some((latex, fallback)) => Inline::Math {
                latex,
                fallback,
                span: self.span(),
            },
            None => self.unsupported_inline("eqn", path),
        }
    }

    pub(super) fn help_link(&mut self, tagged: &RdTagged, path: &RdPath) -> Inline {
        let link = match tagged.inspect_link(path) {
            Ok(link) => link,
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
            inlines: self.inlines(link.display(), path),
            target: format!("@ref {target}"),
            title: None,
            attributes: Attributes::default(),
            span: self.span(),
        }
    }

    pub(super) fn doi_link(&mut self, tagged: &RdTagged, path: &RdPath) -> Inline {
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
