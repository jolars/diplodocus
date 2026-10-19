use crate::ir::{Block, Inline};

pub(super) fn contains_math(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::DisplayMath { .. } => true,
        Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => inline_math(inlines),
        Block::BlockQuote { blocks, .. }
        | Block::Container { blocks, .. }
        | Block::Tabset { blocks, .. }
        | Block::Callout { blocks, .. } => contains_math(blocks),
        Block::List { items, .. } => items.iter().any(|item| contains_math(&item.blocks)),
        Block::Table { caption, rows, .. } => {
            inline_math(caption)
                || rows
                    .iter()
                    .any(|row| row.cells.iter().any(|cell| contains_math(&cell.blocks)))
        }
        _ => false,
    })
}

fn inline_math(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Math { .. } => true,
        Inline::Emphasis { inlines, .. }
        | Inline::Strong { inlines, .. }
        | Inline::Styled { inlines, .. }
        | Inline::Citation { inlines, .. }
        | Inline::Strikeout { inlines, .. }
        | Inline::Link { inlines, .. } => inline_math(inlines),
        Inline::Image { alt, .. } => inline_math(alt),
        _ => false,
    })
}

pub(super) fn render(
    context: &katex::KatexContext,
    latex: &str,
    fallback: Option<&str>,
    display: bool,
) -> String {
    let settings = katex::Settings::builder()
        .display_mode(display)
        .trust(katex::TrustSetting::Bool(false))
        .strict(katex::StrictSetting::Mode(katex::StrictMode::Error))
        .throw_on_error(true)
        .max_expand(1000)
        .max_size(20.0)
        .build();
    let tag = if display { "div" } else { "span" };
    let (class, content) = match katex::render_to_string(context, latex, &settings) {
        Ok(content) => ("math-rendered", canonical_attributes(&content)),
        Err(_) => (
            "math-fallback",
            super::html::escape(fallback.unwrap_or(latex)),
        ),
    };
    format!(
        "<{tag} class=\"math {class}{}\">{content}</{tag}>",
        if display { " math-display" } else { "" }
    )
}

fn canonical_attributes(markup: &str) -> String {
    use std::sync::OnceLock;
    static TAG: OnceLock<regex::Regex> = OnceLock::new();
    static ATTRIBUTE: OnceLock<regex::Regex> = OnceLock::new();
    let tags = TAG.get_or_init(|| {
        regex::Regex::new(r"<([A-Za-z][A-Za-z0-9:-]*)(\s+[^<>]*?)>").expect("math tag pattern")
    });
    let attributes = ATTRIBUTE.get_or_init(|| {
        regex::Regex::new(r#"\s+([^\s=/>]+)="([^"]*)""#).expect("math attribute pattern")
    });
    // KaTeX stores MathML attributes in hash maps, whose order varies between processes.
    tags.replace_all(markup, |tag: &regex::Captures<'_>| {
        let body = &tag[2];
        let mut end = 0;
        let mut values = Vec::new();
        for attribute in attributes.captures_iter(body) {
            let range = attribute.get(0).expect("attribute range");
            if !body[end..range.start()].trim().is_empty() {
                return tag[0].to_owned();
            }
            end = range.end();
            values.push((attribute[1].to_owned(), attribute[2].to_owned()));
        }
        let tail = body[end..].trim();
        if !tail.is_empty() && tail != "/" {
            return tag[0].to_owned();
        }
        values.sort_by(|a, b| a.0.cmp(&b.0));
        format!(
            "<{}{}{}>",
            &tag[1],
            values
                .iter()
                .map(|(key, value)| format!(" {key}=\"{value}\""))
                .collect::<String>(),
            if tail == "/" { " /" } else { "" }
        )
    })
    .into_owned()
}
