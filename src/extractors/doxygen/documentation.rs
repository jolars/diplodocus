use std::collections::BTreeMap;

use roxmltree::Node;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::*;

use super::{child, text};

// XML does not provide byte offsets into the original C++ comment.
const SPAN: SourceSpan = SourceSpan { start: 0, end: 0 };

pub(super) fn document(
    xml: Node<'_, '_>,
    references: &BTreeMap<String, ItemReference>,
    source: Option<SourceLocation>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SourcedDocument> {
    let mut converter = Converter {
        references,
        diagnostics,
    };
    let mut blocks = Vec::new();
    let mut raw = String::new();
    for tag in [
        "briefdescription",
        "detaileddescription",
        "inbodydescription",
    ] {
        if let Some(description) = child(xml, tag) {
            raw.push_str(&text(description));
            raw.push('\n');
            blocks.extend(converter.blocks(description));
        }
    }
    if blocks.is_empty() {
        return None;
    }
    Some(SourcedDocument {
        document: Document {
            frontmatter: None,
            blocks,
            span: SPAN,
        },
        source_format: DocumentFormat::Extracted {
            name: "doxygen-xml".into(),
        },
        source_location: source,
        raw_source: Some(raw),
        generated_source: None,
        provenance: vec![],
    })
}

struct Converter<'a> {
    references: &'a BTreeMap<String, ItemReference>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

fn label(value: impl Into<String>) -> Inline {
    Inline::Text {
        value: value.into(),
        span: SPAN,
    }
}
fn paragraph(inlines: Vec<Inline>) -> Block {
    Block::Paragraph {
        inlines,
        span: SPAN,
    }
}
fn heading(value: &str) -> Block {
    Block::Heading {
        level: 2,
        attributes: Attributes::default(),
        inlines: vec![label(value)],
        span: SPAN,
    }
}

impl Converter<'_> {
    fn unsupported(&mut self, name: &str) {
        self.diagnostics.push(Diagnostic::new(
            DiagnosticCode::DoxygenUnsupported,
            Severity::Warning,
            format!("Doxygen documentation element `{name}` is retained as text."),
        ));
    }

    fn blocks(&mut self, xml: Node<'_, '_>) -> Vec<Block> {
        let mut blocks = Vec::new();
        for node in xml.children().filter(Node::is_element) {
            match node.tag_name().name() {
                "para" => {
                    let mut inlines = Vec::new();
                    for part in node.children() {
                        if part.is_element()
                            && matches!(
                                part.tag_name().name(),
                                "parameterlist"
                                    | "simplesect"
                                    | "programlisting"
                                    | "itemizedlist"
                                    | "orderedlist"
                                    | "table"
                                    | "verbatim"
                                    | "xrefsect"
                            )
                        {
                            if !inlines.is_empty() {
                                blocks.push(paragraph(std::mem::take(&mut inlines)));
                            }
                            blocks.extend(self.block(part));
                        } else {
                            inlines.extend(self.inline(part));
                        }
                    }
                    if inlines.iter().any(|inline| !matches!(inline, Inline::Text { value, .. } if value.trim().is_empty())) { blocks.push(paragraph(inlines)); }
                }
                _ => blocks.extend(self.block(node)),
            }
        }
        blocks
    }

    fn block(&mut self, xml: Node<'_, '_>) -> Vec<Block> {
        match xml.tag_name().name() {
            "briefdescription"
            | "detaileddescription"
            | "inbodydescription"
            | "parameterdescription"
            | "description"
            | "xrefdescription" => self.blocks(xml),
            "sect1" | "sect2" | "sect3" | "sect4" => {
                let mut blocks = Vec::new();
                if let Some(title) = child(xml, "title") {
                    blocks.push(heading(&text(title)));
                }
                for node in xml
                    .children()
                    .filter(|n| n.is_element() && !n.has_tag_name("title"))
                {
                    if node.has_tag_name("para") {
                        blocks.extend(self.blocks(xml));
                        break;
                    }
                    blocks.extend(self.block(node));
                }
                blocks
            }
            "title" => vec![],
            "parameterlist" => {
                let title = match xml.attribute("kind") {
                    Some("templateparam") => "Template parameters",
                    Some("exception") => "Throws",
                    Some("retval") => "Return values",
                    _ => "Parameters",
                };
                let mut blocks = vec![heading(title)];
                for item in xml.children().filter(|n| n.has_tag_name("parameteritem")) {
                    let names = item
                        .descendants()
                        .filter(|n| n.has_tag_name("parametername"))
                        .map(text)
                        .collect::<Vec<_>>()
                        .join(", ");
                    blocks.push(paragraph(vec![Inline::Strong {
                        inlines: vec![Inline::Code {
                            value: names,
                            span: SPAN,
                        }],
                        span: SPAN,
                    }]));
                    if let Some(description) = child(item, "parameterdescription") {
                        blocks.extend(self.blocks(description));
                    }
                }
                blocks
            }
            "simplesect" => {
                let title = match xml.attribute("kind") {
                    Some("return") => "Returns",
                    Some("see") => "See also",
                    Some("note") => "Note",
                    Some("warning") => "Warning",
                    Some("pre") => "Preconditions",
                    Some("post") => "Postconditions",
                    Some("since") => "Since",
                    Some("deprecated") => "Deprecated",
                    _ => "Details",
                };
                let mut blocks = vec![heading(title)];
                blocks.extend(self.blocks(xml));
                blocks
            }
            "xrefsect" => {
                let mut blocks = vec![heading(
                    &child(xml, "xreftitle")
                        .map(text)
                        .unwrap_or_else(|| "Details".into()),
                )];
                if let Some(description) = child(xml, "xrefdescription") {
                    blocks.extend(self.blocks(description));
                }
                blocks
            }
            "programlisting" => {
                let source = xml
                    .children()
                    .filter(|n| n.has_tag_name("codeline"))
                    .map(code)
                    .collect::<Vec<_>>()
                    .join("\n");
                let language = xml
                    .attribute("filename")
                    .and_then(|name| name.rsplit('.').next())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("cpp");
                vec![Block::CodeBlock {
                    language: Some(language.into()),
                    source,
                    source_segments: vec![],
                    span: SPAN,
                }]
            }
            "verbatim" => vec![Block::CodeBlock {
                language: None,
                source: text(xml),
                source_segments: vec![],
                span: SPAN,
            }],
            "itemizedlist" | "orderedlist" => vec![Block::List {
                ordered: xml.has_tag_name("orderedlist"),
                items: xml
                    .children()
                    .filter(|n| n.has_tag_name("listitem"))
                    .map(|item| ListItem {
                        checked: None,
                        blocks: self.blocks(item),
                        span: SPAN,
                    })
                    .collect(),
                span: SPAN,
            }],
            "table" => {
                let rows = xml
                    .children()
                    .filter(|n| n.has_tag_name("row"))
                    .map(|row| TableRow {
                        header: row.children().any(|n| {
                            n.has_tag_name("entry") && n.attribute("thead") == Some("yes")
                        }),
                        cells: row
                            .children()
                            .filter(|n| n.has_tag_name("entry"))
                            .map(|entry| TableCell {
                                blocks: self.blocks(entry),
                                span: SPAN,
                            })
                            .collect(),
                        span: SPAN,
                    })
                    .collect();
                vec![Block::Table {
                    caption: vec![],
                    alignments: vec![],
                    rows,
                    span: SPAN,
                }]
            }
            "formula" => vec![Block::DisplayMath {
                latex: math(&text(xml)),
                fallback: None,
                span: SPAN,
            }],
            _ => {
                self.unsupported(xml.tag_name().name());
                vec![paragraph(vec![label(text(xml))])]
            }
        }
    }

    fn inlines(&mut self, xml: Node<'_, '_>) -> Vec<Inline> {
        xml.children().flat_map(|part| self.inline(part)).collect()
    }

    fn inline(&mut self, xml: Node<'_, '_>) -> Vec<Inline> {
        if xml.is_text() {
            let source = xml.text().unwrap_or("");
            let mut value = source.split_whitespace().collect::<Vec<_>>().join(" ");
            if source.starts_with(char::is_whitespace) {
                value.insert(0, ' ');
            }
            if source.ends_with(char::is_whitespace) && !value.ends_with(' ') {
                value.push(' ');
            }
            return vec![label(value)];
        }
        if !xml.is_element() {
            return vec![];
        }
        let inline = match xml.tag_name().name() {
            "bold" => Inline::Strong {
                inlines: self.inlines(xml),
                span: SPAN,
            },
            "emphasis" => Inline::Emphasis {
                inlines: self.inlines(xml),
                span: SPAN,
            },
            "strike" => Inline::Strikeout {
                inlines: self.inlines(xml),
                span: SPAN,
            },
            "computeroutput" => Inline::Code {
                value: text(xml),
                span: SPAN,
            },
            "linebreak" => Inline::HardBreak { span: SPAN },
            "sp" => label(" "),
            "formula" => Inline::Math {
                latex: math(&text(xml)),
                fallback: None,
                span: SPAN,
            },
            "ref" => {
                if xml.attribute("external").is_none()
                    && let Some(reference) = xml
                        .attribute("refid")
                        .and_then(|id| self.references.get(id))
                {
                    Inline::Link {
                        inlines: self.inlines(xml),
                        target: format!("@ref {}::{}", reference.package, reference.item),
                        title: None,
                        attributes: Attributes::default(),
                        span: SPAN,
                    }
                } else {
                    label(text(xml))
                }
            }
            "ulink" => Inline::Link {
                inlines: self.inlines(xml),
                target: xml.attribute("url").unwrap_or("").into(),
                title: None,
                attributes: Attributes::default(),
                span: SPAN,
            },
            "superscript" | "subscript" => Inline::Styled {
                style: if xml.has_tag_name("superscript") {
                    InlineStyle::Superscript
                } else {
                    InlineStyle::Subscript
                },
                inlines: self.inlines(xml),
                span: SPAN,
            },
            _ => {
                self.unsupported(xml.tag_name().name());
                label(text(xml))
            }
        };
        vec![inline]
    }
}

fn code(xml: Node<'_, '_>) -> String {
    if xml.is_text() {
        return xml.text().unwrap_or("").into();
    }
    if xml.has_tag_name("sp") {
        return " ".into();
    }
    xml.children().map(code).collect()
}

fn math(source: &str) -> String {
    source
        .trim()
        .trim_start_matches("\\[")
        .trim_end_matches("\\]")
        .trim_matches('$')
        .trim()
        .to_owned()
}
