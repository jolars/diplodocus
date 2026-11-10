mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::documents::{
    AuthoredFormat, MarkdownFragmentOrigin, parse_authored_document, parse_evaluated_rmd,
    parse_markdown_fragment,
};
use diplodocus::ir::{Block, Inline, OutputRepresentation, SourceLocation};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

// Qualpalr release notes and conduct text and qualpal algorithm excerpts
// reproduce the reported failures alongside nested formatting cases.
const SOURCE: &str = include_str!("fixtures/markdown-continuations/index.md");

fn assert_continuations(blocks: &[Block]) {
    fn inlines(values: &[Inline]) {
        for inline in values {
            match inline {
                Inline::Unsupported { .. } => panic!("unexpected parser content: {inline:?}"),
                Inline::Text { value, .. } => assert!(!value.is_empty()),
                Inline::Code { value, .. } => assert!(!value.trim().is_empty()),
                Inline::Emphasis { inlines: inner, .. }
                | Inline::Strong { inlines: inner, .. }
                | Inline::Strikeout { inlines: inner, .. }
                | Inline::Link { inlines: inner, .. } => inlines(inner),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph { inlines: inner, .. } | Block::Heading { inlines: inner, .. } => {
                inlines(inner);
            }
            Block::List { items, .. } => {
                for item in items {
                    assert_continuations(&item.blocks);
                }
            }
            Block::BlockQuote { blocks, .. } => assert_continuations(blocks),
            other => panic!("unexpected block: {other:?}"),
        }
    }
}

#[test]
fn continuation_prefixes_are_trivia_in_authored_and_generated_markdown() {
    for source in [SOURCE.to_owned(), SOURCE.replace('\n', "\r\n")] {
        for format in [
            AuthoredFormat::Gfm,
            AuthoredFormat::Qmd,
            AuthoredFormat::Rmd,
        ] {
            let parsed = parse_authored_document(&source, format);
            assert!(
                parsed.diagnostics.is_empty(),
                "{format:?}: {:?}",
                parsed.diagnostics
            );
            assert_continuations(&parsed.document.blocks);
            let Block::List { items, .. } = &parsed.document.blocks[8] else {
                panic!("nested list: {:?}", parsed.document.blocks);
            };
            assert_eq!(items.len(), 2);
            assert!(
                items[0]
                    .blocks
                    .iter()
                    .any(|b| matches!(b, Block::List { ordered: false, .. }))
            );
            assert!(
                items[0]
                    .blocks
                    .iter()
                    .any(|b| matches!(b, Block::List { ordered: true, .. })),
                "{format:?}: {items:#?}"
            );
        }
        let evaluated = parse_evaluated_rmd(&source);
        assert!(
            evaluated.diagnostics.is_empty(),
            "{:?}",
            evaluated.diagnostics
        );
        assert_continuations(&evaluated.document.blocks);
        let fragment = parse_markdown_fragment(
            &source,
            MarkdownFragmentOrigin {
                collection: "guide".into(),
                cell: 0,
                output: 0,
                source: SourceLocation {
                    repository: "docs".into(),
                    path: "index.md".try_into().unwrap(),
                    span: None,
                },
            },
        );
        assert!(
            fragment.diagnostics.is_empty(),
            "{:?}",
            fragment.diagnostics
        );
        let OutputRepresentation::MarkdownBlocks { blocks, .. } = fragment.representation else {
            panic!("Markdown blocks");
        };
        assert_continuations(&blocks);
    }
}

#[test]
fn wrapped_markdown_renders_spacing_and_nesting_without_parser_content() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Continuations\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n");
    root.write("docs/index.md", SOURCE);
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("snapshot.sqlite");
    snapshot.publish(&database).unwrap();
    drop(sources);
    drop(root);
    let snapshot = Snapshot::load(database).unwrap();
    let site = diplodocus::site::Site::new(&snapshot).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    let markdown = std::str::from_utf8(&rendered.files()["index.md"].bytes).unwrap();
    for text in [
        "experimental continuous refinement",
        "electronic address, without explicit permission",
        "such as <code>&quot;ColorBrewer:Set2&quot;</code>",
        "<em>emphasized continuation</em>",
        "<strong>strong continuation</strong>",
        "<del>struck continuation</del>",
        ">linked continuation</a>",
        "Inner text continues with <code>literal  spaces</code>",
        "Quoted text continues.",
        "Deeper quote continues.",
        "Ordered text continues.",
    ] {
        assert!(html.contains(text), "missing {text:?} in {html}");
    }
    assert!(!html.contains("class=\"unsupported\""));
    assert!(!html.contains("LINE_PREFIX"));
    for text in [
        "experimental continuous refinement",
        "electronic address, without explicit permission",
        "*emphasized continuation*",
        "**strong continuation**",
        "~~struck continuation~~",
        "[linked continuation](<https://example.test>)",
        "  - Inner text continues with `literal  spaces`.",
        "    > Quoted text continues.",
        "    > > Deeper quote continues.",
        "  1. Ordered text continues.",
    ] {
        assert!(markdown.contains(text), "missing {text:?} in {markdown}");
    }
    assert!(!markdown.contains("`  `"));
    assert!(!markdown.contains("LINE_PREFIX"));
}
