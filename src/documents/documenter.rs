//! Pure preparation and presentation of native Documenter examples.

use fatou_parser::ast::AstNode;
use fatou_parser::documentation::{self, ast as md};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::*;

pub(crate) const POLICY: &str = "documenter-examples-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Example {
    pub span: SourceSpan,
    pub session: Option<String>,
    pub source: String,
    pub visible: Vec<SourceSegment>,
}

pub(crate) fn examples(source: &str) -> Result<Vec<Example>, Vec<Diagnostic>> {
    let parsed = documentation::parse(source);
    let mut examples = Vec::new();
    let mut diagnostics = Vec::new();
    for node in parsed.cst.descendants().filter_map(md::CodeBlock::cast) {
        let md::FenceKind::Documenter { directive, .. } = node.fence_kind() else {
            continue;
        };
        let span = SourceSpan {
            start: usize::from(node.syntax().text_range().start()),
            end: usize::from(node.syntax().text_range().end()),
        };
        let info = node.info();
        if directive == md::DocumenterDirective::Example && !info.contains(';') {
            let session = info["@example".len()..].trim();
            let source = node.content();
            let segments =
                crate::extractors::julia::docstrings::code_segments(node.syntax(), &source);
            examples.push(Example {
                span,
                session: (!session.is_empty()).then(|| session.to_owned()),
                visible: visible_source(&source, &segments),
                source,
            });
        } else if matches!(
            directive,
            md::DocumenterDirective::Example
                | md::DocumenterDirective::Repl
                | md::DocumenterDirective::Setup
                | md::DocumenterDirective::Eval
                | md::DocumenterDirective::Meta
        ) {
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::UnsupportedAuthoredSyntax,
                Severity::Error,
                "Native Documenter execution supports `@example` with an optional session name; executable options and other execution directives are unsupported.",
            );
            diagnostic.span = Some(span);
            diagnostics.push(diagnostic);
        }
    }
    if diagnostics.is_empty() {
        Ok(examples)
    } else {
        Err(diagnostics)
    }
}

fn visible_source(source: &str, segments: &[SourceSegment]) -> Vec<SourceSegment> {
    use fatou_parser::syntax::SyntaxKind;
    // Julia tokens distinguish hide comments from strings containing `# hide`.
    let parsed = fatou_parser::parser::parse(source);
    let hidden: Vec<_> = parsed
        .cst
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| {
            token.kind() == SyntaxKind::COMMENT
                && token
                    .text()
                    .strip_prefix('#')
                    .is_some_and(|comment| comment.trim() == "hide")
        })
        .map(|token| usize::from(token.text_range().start()))
        .collect();
    let mut offset = 0;
    let mut lines: Vec<_> = source
        .split_inclusive('\n')
        .filter_map(|line| {
            let start = offset;
            offset += line.len();
            (!hidden
                .iter()
                .any(|&position| start <= position && position < offset))
            .then_some((start, line.trim_end().len(), line))
        })
        .collect();
    while lines.first().is_some_and(|(_, length, _)| *length == 0) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|(_, length, _)| *length == 0) {
        lines.pop();
    }
    let mut ranges = Vec::new();
    for (index, &(start, length, line)) in lines.iter().enumerate() {
        ranges.push(start..start + length);
        if index + 1 < lines.len() && line.ends_with('\n') {
            ranges.push(start + line.len() - 1..start + line.len());
        }
    }
    let mut result = Vec::new();
    // Container prefixes split code tokens, so map visible bytes back through
    // their original segments instead of inventing new authored ranges.
    for range in ranges {
        let mut offset = 0;
        for segment in segments {
            let start = range.start.max(offset);
            let end = range.end.min(offset + segment.text.len());
            if start < end {
                let relative = start - offset;
                result.push(SourceSegment {
                    text: segment.text[relative..end - offset].into(),
                    span: SourceSpan {
                        start: segment.span.start + relative,
                        end: segment.span.start + end - offset,
                    },
                });
            }
            offset += segment.text.len();
        }
    }
    result
}

pub(crate) fn rendered(example: &Example, outcome: &DocumenterExampleOutcome) -> Block {
    let span = example.span;
    let mut blocks = Vec::new();
    if !example.visible.is_empty() {
        blocks.push(Block::CodeBlock {
            language: Some("julia".into()),
            source: example
                .visible
                .iter()
                .map(|segment| segment.text.as_str())
                .collect(),
            source_segments: example.visible.clone(),
            span,
        });
    }
    let outputs = outcome
        .outputs
        .iter()
        .map(|output| match output {
            DocumenterOutput::Text { text } => Block::CodeBlock {
                language: None,
                source: text.clone(),
                source_segments: Vec::new(),
                span,
            },
            DocumenterOutput::Image { asset, .. } => Block::Paragraph {
                inlines: vec![Inline::Image {
                    alt: Vec::new(),
                    target: asset.path.as_str().into(),
                    title: None,
                    attributes: Attributes::default(),
                    span,
                }],
                span,
            },
        })
        .collect::<Vec<_>>();
    if !outputs.is_empty() {
        blocks.push(Block::Container {
            kind: DocumentContainerKind::GeneratedOutput,
            identifier: None,
            blocks: outputs,
            span,
        });
    }
    Block::Container {
        kind: DocumentContainerKind::GeneratedChunk,
        identifier: None,
        blocks,
        span,
    }
}

pub(crate) fn rewrite(blocks: &mut [Block], transform: &mut impl FnMut(&mut Block)) {
    for block in blocks {
        transform(block);
        match block {
            Block::BlockQuote { blocks, .. }
            | Block::Container { blocks, .. }
            | Block::Component { blocks, .. }
            | Block::Tabset { blocks, .. }
            | Block::Callout { blocks, .. } => rewrite(blocks, transform),
            Block::List { items, .. } => {
                for item in items {
                    rewrite(&mut item.blocks, transform);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        rewrite(&mut cell.blocks, transform);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_lines_preserve_exact_visible_ranges_in_nested_unicode_source() {
        let source = "- Nested:\n\n  ```@example 1\n  \n  α = 1 # hide\n  literal = \"# hide\"   \n  # hide\n  α + 2\n  \n  ```\n";
        let examples = examples(source).unwrap();
        let example = &examples[0];
        assert_eq!(example.session.as_deref(), Some("1"));
        assert!(example.source.contains("α = 1 # hide"));
        let visible: String = example
            .visible
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(visible, "literal = \"# hide\"\nα + 2");
        for segment in &example.visible {
            assert_eq!(&source[segment.span.start..segment.span.end], segment.text);
        }
    }
}
