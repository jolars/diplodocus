use crate::ir::{Block, Inline};

pub(super) fn contains_math(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::DisplayMath { .. } => true,
        Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => inline_math(inlines),
        Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => contains_math(blocks),
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
        Ok(content) => ("math-rendered", content),
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
