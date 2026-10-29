use super::*;
use crate::documents::{AuthoredFormat, parse_authored_document};

impl Context<'_> {
    pub(super) fn structured_prose(&mut self, lines: &[SourceSpan]) -> Option<Vec<Block>> {
        let lines: Vec<_> = lines
            .iter()
            .copied()
            .filter(|line| line.start != line.end)
            .collect();
        for (index, line) in lines.iter().enumerate() {
            if self.text[line.start..line.end].trim_end().ends_with("::")
                && lines
                    .get(index + 1)
                    .is_some_and(|next| self.indent(*next) > self.indent(*line))
            {
                let mut blocks = self.prose_lines(lines[..=index].iter().copied());
                if let Some(Block::Paragraph { inlines, .. }) = blocks.last_mut()
                    && let Some(Inline::Text { value, .. }) = inlines.last_mut()
                    && value.ends_with("::")
                {
                    value.pop();
                }
                let end = lines[index + 1..]
                    .iter()
                    .position(|next| self.indent(*next) <= self.indent(*line))
                    .map_or(lines.len(), |offset| index + 1 + offset);
                blocks.push(self.example(SourceSpan {
                    start: lines[index + 1].start,
                    end: lines[end - 1].end,
                }));
                blocks.extend(self.prose_lines(lines[end..].iter().copied()));
                return Some(blocks);
            }
        }
        if !lines
            .iter()
            .any(|line| list_marker(&self.text[line.start..line.end]))
        {
            return None;
        }
        let baseline = lines.iter().map(|line| self.indent(*line)).min()?;
        let mut text = String::new();
        let mut offsets = Vec::new();
        let mut previous = lines[0].start;
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                for (offset, byte) in self.text.as_bytes()[previous..line.start]
                    .iter()
                    .enumerate()
                {
                    if *byte == b'\n' {
                        text.push('\n');
                        offsets.push(previous + offset);
                    }
                }
            }
            let start = line.start - self.indent(*line).saturating_sub(baseline);
            text.push_str(&self.text[start..line.end]);
            offsets.extend(start..line.end);
            previous = line.end;
        }
        offsets.push(previous);
        self.panache_used = true;
        let mut parsed = parse_authored_document(&text, AuthoredFormat::Gfm);
        // Paragraphs are reparsed below with Sphinx roles and container prefixes
        // handled explicitly, so their generic inline diagnostics are replaced.
        let mut diagnostics: Vec<_> = parsed
            .diagnostics
            .into_iter()
            .filter(|diagnostic| {
                !matches!(
                    diagnostic.message.as_str(),
                    "unsupported authored syntax: LINE_PREFIX"
                        | "unsupported authored syntax: EMOJI"
                )
            })
            .collect();
        for block in &mut parsed.document.blocks {
            paragraph_roles(block, &text, &mut diagnostics, &mut self.panache_used);
            remap_block(block, &offsets);
        }
        for mut diagnostic in diagnostics {
            diagnostic.code = DiagnosticCode::PythonUnsupportedDocstring;
            if let Some(span) = &mut diagnostic.span {
                remap(span, &offsets);
            }
            for span in &mut diagnostic.related_spans {
                remap(span, &offsets);
            }
            self.diagnostics.push(diagnostic);
        }
        Some(parsed.document.blocks)
    }

    fn indent(&self, line: SourceSpan) -> usize {
        line.start
            - self.text[..line.start]
                .rfind('\n')
                .map_or(0, |offset| offset + 1)
    }
}

fn list_marker(text: &str) -> bool {
    let text = text.trim_start();
    ["* ", "- ", "+ "]
        .iter()
        .any(|marker| text.starts_with(marker))
        || text
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .strip_prefix(". ")
            .is_some()
}

fn paragraph_roles(
    block: &mut Block,
    text: &str,
    diagnostics: &mut Vec<Diagnostic>,
    panache_used: &mut bool,
) {
    match block {
        Block::Paragraph { inlines, span } => {
            inlines.clear();
            let mut offset = span.start;
            let mut previous_end = span.start;
            for line in text[span.start..span.end].split_inclusive('\n') {
                let value = line.trim();
                let start = offset + line.len() - line.trim_start().len();
                if start > previous_end && !inlines.is_empty() {
                    inlines.push(Inline::SoftBreak {
                        span: SourceSpan {
                            start: previous_end,
                            end: start,
                        },
                    });
                }
                inlines.extend(inline::parse_line(value, start, diagnostics, panache_used));
                previous_end = start + value.len();
                offset += line.len();
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &mut item.blocks {
                    paragraph_roles(block, text, diagnostics, panache_used);
                }
            }
        }
        Block::BlockQuote { blocks, .. }
        | Block::Container { blocks, .. }
        | Block::Component { blocks, .. }
        | Block::Tabset { blocks, .. }
        | Block::Callout { blocks, .. } => {
            for block in blocks {
                paragraph_roles(block, text, diagnostics, panache_used);
            }
        }
        _ => {}
    }
}

fn remap(span: &mut SourceSpan, offsets: &[usize]) {
    span.start = offsets[span.start];
    span.end = offsets[span.end];
}

fn remap_attributes(attributes: &mut Attributes, offsets: &[usize]) {
    if let Some(identifier) = &mut attributes.identifier {
        remap(&mut identifier.span, offsets);
    }
    for class in &mut attributes.classes {
        remap(&mut class.span, offsets);
    }
    for pair in &mut attributes.key_values {
        remap(&mut pair.key.span, offsets);
        remap(&mut pair.value.span, offsets);
    }
}

fn remap_inlines(inlines: &mut [Inline], offsets: &[usize]) {
    for inline in inlines {
        match inline {
            Inline::Emphasis { inlines, span }
            | Inline::Strong { inlines, span }
            | Inline::Styled { inlines, span, .. }
            | Inline::Citation { inlines, span, .. }
            | Inline::Strikeout { inlines, span } => {
                remap(span, offsets);
                remap_inlines(inlines, offsets);
            }
            Inline::Link {
                inlines,
                attributes,
                span,
                ..
            } => {
                remap(span, offsets);
                remap_attributes(attributes, offsets);
                remap_inlines(inlines, offsets);
            }
            Inline::Image {
                alt,
                attributes,
                span,
                ..
            } => {
                remap(span, offsets);
                remap_attributes(attributes, offsets);
                remap_inlines(alt, offsets);
            }
            Inline::SemanticReference {
                span, target_span, ..
            } => {
                remap(span, offsets);
                remap(target_span, offsets);
            }
            Inline::Text { span, .. }
            | Inline::Space { span }
            | Inline::SoftBreak { span }
            | Inline::HardBreak { span }
            | Inline::NonbreakingSpace { span }
            | Inline::Code { span, .. }
            | Inline::AutoLink { span, .. }
            | Inline::Math { span, .. }
            | Inline::Unsupported { span, .. } => remap(span, offsets),
        }
    }
}

fn remap_block(block: &mut Block, offsets: &[usize]) {
    match block {
        Block::Paragraph { inlines, span } => {
            remap(span, offsets);
            remap_inlines(inlines, offsets);
        }
        Block::Heading {
            attributes,
            inlines,
            span,
            ..
        } => {
            remap(span, offsets);
            remap_attributes(attributes, offsets);
            remap_inlines(inlines, offsets);
        }
        Block::List { items, span, .. } => {
            remap(span, offsets);
            for item in items {
                remap(&mut item.span, offsets);
                for block in &mut item.blocks {
                    remap_block(block, offsets);
                }
            }
        }
        Block::Callout {
            attributes,
            blocks,
            span,
            ..
        }
        | Block::Component {
            attributes,
            blocks,
            span,
        }
        | Block::Tabset {
            attributes,
            blocks,
            span,
        } => {
            remap(span, offsets);
            remap_attributes(attributes, offsets);
            for block in blocks {
                remap_block(block, offsets);
            }
        }
        Block::BlockQuote { blocks, span } | Block::Container { blocks, span, .. } => {
            remap(span, offsets);
            for block in blocks {
                remap_block(block, offsets);
            }
        }
        Block::CodeBlock {
            source_segments,
            span,
            ..
        } => {
            remap(span, offsets);
            for segment in source_segments {
                remap(&mut segment.span, offsets);
            }
        }
        Block::Table {
            caption,
            rows,
            span,
            ..
        } => {
            remap(span, offsets);
            remap_inlines(caption, offsets);
            for row in rows {
                remap(&mut row.span, offsets);
                for cell in &mut row.cells {
                    remap(&mut cell.span, offsets);
                    for block in &mut cell.blocks {
                        remap_block(block, offsets);
                    }
                }
            }
        }
        Block::ThematicBreak { span }
        | Block::DisplayMath { span, .. }
        | Block::Unsupported { span, .. } => remap(span, offsets),
        Block::ApiDocumentation { .. } | Block::CodeCell(_) => {
            unreachable!("GFM prose cannot create executable cells")
        }
    }
}
