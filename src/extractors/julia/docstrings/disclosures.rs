//! Paired raw HTML wrappers become semantic containers before leaving the parser.

use std::cell::RefCell;

use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::ir::{Block, DocumentContainerKind, Inline, SourceSpan};

enum Wrapper {
    Open { open: bool, summary: Vec<Inline> },
    Close,
}

#[derive(Default)]
struct Tokens(RefCell<Vec<Token>>);

impl TokenSink for Tokens {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        if !matches!(token, Token::EOFToken) {
            self.0.borrow_mut().push(token);
        }
        TokenSinkResult::Continue
    }
}

fn next_non_space(tokens: &mut impl Iterator<Item = Token>) -> Option<Token> {
    tokens.find(|token| !matches!(token, Token::CharacterTokens(text) if text.trim().is_empty()))
}

fn inlines(
    tokens: &mut impl Iterator<Item = Token>,
    end: &str,
    span: SourceSpan,
    depth: usize,
) -> Option<Vec<Inline>> {
    if depth > 32 {
        return None;
    }
    let mut result = Vec::new();
    while let Some(token) = tokens.next() {
        match token {
            Token::CharacterTokens(text) => result.push(Inline::Text {
                value: text.to_string(),
                span,
            }),
            Token::TagToken(tag) if tag.attrs.is_empty() && !tag.had_duplicate_attributes => {
                if tag.kind == TagKind::EndTag && tag.name.as_ref() == end && !tag.self_closing {
                    return Some(result);
                }
                if tag.kind != TagKind::StartTag {
                    return None;
                }
                if tag.name.as_ref() == "br" {
                    result.push(Inline::HardBreak { span });
                    continue;
                }
                if tag.self_closing
                    || !matches!(tag.name.as_ref(), "b" | "strong" | "i" | "em" | "code")
                {
                    return None;
                }
                let children = inlines(tokens, &tag.name, span, depth + 1)?;
                result.push(match tag.name.as_ref() {
                    "b" | "strong" => Inline::Strong {
                        inlines: children,
                        span,
                    },
                    "i" | "em" => Inline::Emphasis {
                        inlines: children,
                        span,
                    },
                    "code" => Inline::Code {
                        value: children
                            .into_iter()
                            .map(|inline| match inline {
                                Inline::Text { value, .. } => Some(value),
                                _ => None,
                            })
                            .collect::<Option<String>>()?,
                        span,
                    },
                    _ => unreachable!(),
                });
            }
            _ => return None,
        }
    }
    None
}

fn wrapper(markup: &str, span: SourceSpan) -> Option<Wrapper> {
    let input = BufferQueue::default();
    input.push_back(markup.into());
    let tokenizer = Tokenizer::new(Tokens::default(), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut tokens = tokenizer.sink.0.into_inner().into_iter();
    let Token::TagToken(details) = next_non_space(&mut tokens)? else {
        return None;
    };
    if details.name.as_ref() != "details"
        || details.self_closing
        || details.had_duplicate_attributes
    {
        return None;
    }
    if details.kind == TagKind::EndTag {
        return (details.attrs.is_empty() && next_non_space(&mut tokens).is_none())
            .then_some(Wrapper::Close);
    }
    if details.attrs.iter().any(|attribute| {
        attribute.name.local.as_ref() != "open"
            || !attribute.name.ns.as_ref().is_empty()
            || attribute.name.prefix.is_some()
    }) {
        return None;
    }
    let Token::TagToken(summary) = next_non_space(&mut tokens)? else {
        return None;
    };
    if summary.kind != TagKind::StartTag
        || summary.name.as_ref() != "summary"
        || summary.self_closing
        || !summary.attrs.is_empty()
    {
        return None;
    }
    let summary = inlines(&mut tokens, "summary", span, 0)?;
    next_non_space(&mut tokens)
        .is_none()
        .then_some(Wrapper::Open {
            open: !details.attrs.is_empty(),
            summary,
        })
}

struct Disclosure {
    open: bool,
    summary: Vec<Inline>,
    blocks: Vec<Block>,
    span: SourceSpan,
}

fn append(
    output: &mut Vec<Block>,
    stack: &mut [Disclosure],
    blocks: impl IntoIterator<Item = Block>,
) {
    if let Some(disclosure) = stack.last_mut() {
        disclosure.blocks.extend(blocks);
    } else {
        output.extend(blocks);
    }
}

fn unsupported(
    span: SourceSpan,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    message: &str,
) -> Block {
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::UnsupportedAuthoredSyntax,
        Severity::Warning,
        message,
    );
    diagnostic.span = Some(span);
    diagnostics.push(diagnostic);
    Block::Unsupported {
        source_kind: "documenter-raw-html".into(),
        raw: source[span.start..span.end].into(),
        span,
    }
}

pub(super) fn resolve(
    blocks: Vec<Block>,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Block> {
    let mut output = Vec::new();
    let mut stack = Vec::<Disclosure>::new();
    for mut block in blocks {
        match &mut block {
            Block::Container { blocks, .. }
            | Block::BlockQuote { blocks, .. }
            | Block::Callout { blocks, .. } => {
                *blocks = resolve(std::mem::take(blocks), source, diagnostics);
            }
            Block::List { items, .. } => {
                for item in items {
                    item.blocks = resolve(std::mem::take(&mut item.blocks), source, diagnostics);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &mut row.cells {
                        cell.blocks =
                            resolve(std::mem::take(&mut cell.blocks), source, diagnostics);
                    }
                }
            }
            _ => {}
        }
        if let Block::CodeBlock {
            language: Some(language),
            source: markup,
            span,
            ..
        } = &block
            && language == "@raw html"
        {
            match wrapper(markup, *span) {
                Some(Wrapper::Open { open, summary }) => {
                    stack.push(Disclosure {
                        open,
                        summary,
                        blocks: Vec::new(),
                        span: *span,
                    });
                    continue;
                }
                Some(Wrapper::Close) if !stack.is_empty() => {
                    let disclosure = stack.pop().unwrap();
                    let mut blocks = vec![Block::Paragraph {
                        inlines: disclosure.summary,
                        span: disclosure.span,
                    }];
                    blocks.extend(disclosure.blocks);
                    block = Block::Container {
                        kind: DocumentContainerKind::Disclosure {
                            open: disclosure.open,
                        },
                        identifier: None,
                        blocks,
                        span: SourceSpan {
                            start: disclosure.span.start,
                            end: span.end,
                        },
                    };
                }
                _ => {
                    block = unsupported(
                        *span,
                        source,
                        diagnostics,
                        "Raw HTML requires a paired `<details><summary>...</summary>` opening wrapper or `</details>` closing wrapper.",
                    )
                }
            }
        }
        append(&mut output, &mut stack, [block]);
    }
    while let Some(disclosure) = stack.pop() {
        let opening = unsupported(
            disclosure.span,
            source,
            diagnostics,
            "Raw HTML disclosure is missing its `</details>` closing wrapper.",
        );
        append(&mut output, &mut stack, [opening]);
        append(&mut output, &mut stack, disclosure.blocks);
    }
    output
}
