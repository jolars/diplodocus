use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::documents::{AuthoredFormat, parse_authored_document};
use crate::ir::{Attributes, Block, Inline, SourceSpan};

pub(super) fn parse_line(
    text: &str,
    offset: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Inline> {
    // Markdown interprets a reST role's payload as ordinary inline code,
    // which would silently discard its distinct reference semantics.
    if text.trim_start().starts_with(".. ") || has_rst_role(text) {
        return unsupported(text, offset, diagnostics);
    }
    let parsed = parse_authored_document(text, AuthoredFormat::Gfm);
    for mut diagnostic in parsed.diagnostics {
        diagnostic.code = DiagnosticCode::PythonUnsupportedDocstring;
        if let Some(span) = &mut diagnostic.span {
            shift(span, offset);
        }
        for span in &mut diagnostic.related_spans {
            shift(span, offset);
        }
        diagnostics.push(diagnostic);
    }
    if let [Block::Paragraph { inlines, .. }] = parsed.document.blocks.as_slice() {
        let mut inlines = inlines.clone();
        for inline in &mut inlines {
            shift_inline(inline, offset);
        }
        return inlines;
    }
    unsupported(text, offset, diagnostics)
}

fn has_rst_role(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let length = bytes[index..].iter().take_while(|b| **b == b'`').count();
            let delimiter = &text[index..index + length];
            let tail = &text[index + length..];
            index = tail
                .find(delimiter)
                .map_or(bytes.len(), |end| index + length + end + length);
            continue;
        }
        if bytes[index] == b':' {
            let tail = &text[index + 1..];
            let length = tail
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-'))
                .count();
            if length > 1 && tail[..length].ends_with(':') && tail[length..].starts_with('`') {
                return true;
            }
        }
        index += if bytes[index] == b'\\' { 2 } else { 1 };
    }
    false
}

fn unsupported(text: &str, offset: usize, diagnostics: &mut Vec<Diagnostic>) -> Vec<Inline> {
    let range = SourceSpan {
        start: offset,
        end: offset + text.len(),
    };
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::PythonUnsupportedDocstring,
        Severity::Warning,
        "unsupported docstring prose markup; source retained",
    );
    diagnostic.span = Some(range);
    diagnostics.push(diagnostic);
    vec![Inline::Unsupported {
        source_kind: "numpy-prose".into(),
        raw: text.into(),
        span: range,
    }]
}

fn shift(span: &mut SourceSpan, offset: usize) {
    span.start += offset;
    span.end += offset;
}

fn shift_attributes(attributes: &mut Attributes, offset: usize) {
    if let Some(id) = &mut attributes.identifier {
        shift(&mut id.span, offset);
    }
    for class in &mut attributes.classes {
        shift(&mut class.span, offset);
    }
    for pair in &mut attributes.key_values {
        shift(&mut pair.key.span, offset);
        shift(&mut pair.value.span, offset);
    }
}

fn shift_inline(inline: &mut Inline, offset: usize) {
    match inline {
        Inline::Text { span, .. }
        | Inline::Space { span }
        | Inline::SoftBreak { span }
        | Inline::HardBreak { span }
        | Inline::NonbreakingSpace { span }
        | Inline::Code { span, .. }
        | Inline::AutoLink { span, .. }
        | Inline::Unsupported { span, .. } => shift(span, offset),
        Inline::Emphasis { inlines, span }
        | Inline::Strong { inlines, span }
        | Inline::Strikeout { inlines, span } => {
            shift(span, offset);
            for inline in inlines {
                shift_inline(inline, offset);
            }
        }
        Inline::Link {
            inlines,
            attributes,
            span,
            ..
        } => {
            shift(span, offset);
            shift_attributes(attributes, offset);
            for inline in inlines {
                shift_inline(inline, offset);
            }
        }
        Inline::Image {
            alt,
            attributes,
            span,
            ..
        } => {
            shift(span, offset);
            shift_attributes(attributes, offset);
            for inline in alt {
                shift_inline(inline, offset);
            }
        }
        Inline::SemanticReference {
            span, target_span, ..
        } => {
            shift(span, offset);
            shift(target_span, offset);
        }
    }
}
