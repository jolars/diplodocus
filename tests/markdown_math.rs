mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::documents::{
    AuthoredFormat, MarkdownFragmentOrigin, parse_authored_document, parse_markdown_fragment,
};
use diplodocus::ir::{Block, Inline, OutputRepresentation, SourceLocation};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

#[test]
fn gfm_equations_retain_math_and_leave_literals_unchanged() {
    let source = r#"# Math

Inline $\theta_1^2$ and escaped \$5, with `$literal$`.

$$
\frac{\|y-X\theta\|^2}{2n}
$$

> A nested $x+y$.

```rust
let literal = "$not_math$";
```
"#;
    let parsed = parse_authored_document(source, AuthoredFormat::Gfm);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Block::Paragraph { inlines, .. } = &parsed.document.blocks[1] else {
        panic!("inline equation");
    };
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::Math { latex, .. } if latex == "\\theta_1^2"))
    );
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::Code { value, .. } if value == "$literal$"))
    );
    let literal_text: String = inlines
        .iter()
        .filter_map(|inline| {
            if let Inline::Text { value, .. } = inline {
                Some(value.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(literal_text.contains("$5"));
    assert!(
        matches!(&parsed.document.blocks[2], Block::DisplayMath { latex, .. } if latex.contains("\\frac"))
    );
    let Block::BlockQuote { blocks, .. } = &parsed.document.blocks[3] else {
        panic!("nested equation");
    };
    assert!(
        matches!(&blocks[0], Block::Paragraph { inlines, .. } if inlines.iter().any(|i| matches!(i, Inline::Math { latex, .. } if latex == "x+y")))
    );
    assert!(
        matches!(&parsed.document.blocks[4], Block::CodeBlock { source, .. } if source.contains("$not_math$"))
    );
    let encoded = serde_json::to_string(&parsed.document).unwrap();
    assert_eq!(
        serde_json::from_str::<diplodocus::ir::Document>(&encoded).unwrap(),
        parsed.document
    );
}

#[test]
fn generated_markdown_equations_use_the_same_inert_representation() {
    let parsed = parse_markdown_fragment(
        "$x$\n\n$$y^2$$\n",
        MarkdownFragmentOrigin {
            collection: "guide".into(),
            cell: 0,
            output: 0,
            source: SourceLocation {
                repository: "demo".into(),
                path: "docs/index.md".try_into().unwrap(),
                span: None,
            },
        },
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let OutputRepresentation::MarkdownBlocks { blocks, .. } = parsed.representation else {
        panic!("Markdown blocks");
    };
    assert!(
        matches!(&blocks[0], Block::Paragraph { inlines, .. } if matches!(&inlines[0], Inline::Math { latex, .. } if latex == "x"))
    );
    assert!(matches!(&blocks[1], Block::DisplayMath { latex, .. } if latex == "y^2"));
}

#[test]
fn detached_markdown_snapshot_renders_local_katex_and_readable_fallbacks() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write("diplodocus.yaml", format!("{}\ncontent:\n  - id: guide\n    owner: project\n    repository: demo\n    path: docs\n    mount: guide\n    format: gfm\n", root.read("diplodocus.yaml")));
    root.write(
        "docs/index.md",
        "# Math\n\nInline $x^2$.\n\n$$\\frac{1}{n}\\sum_i x_i$$\n\n$\\unknowncommand{x}$\n",
    );
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
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    assert!(html.contains("class=\"math math-rendered\""));
    assert!(html.contains("class=\"math math-rendered math-display\""));
    assert!(html.contains("<math"));
    assert!(html.contains("math-fallback"));
    assert!(html.contains("\\unknowncommand{x}"));
    assert!(!html.contains("class=\"unsupported\""));
    assert!(rendered.files().contains_key("assets/katex/katex.min.css"));
    assert!(
        rendered
            .files()
            .contains_key("assets/katex/fonts/KaTeX_Main-Regular.woff2")
    );
    assert!(!rendered.files().contains_key("assets/katex/katex.min.js"));
    let fragments = diplodocus::rendering::render_fragments(&site).unwrap();
    let body = std::str::from_utf8(&fragments.files()["guide/index.html"].bytes).unwrap();
    assert!(body.contains("math-rendered"));
    assert!(body.contains("math-fallback"));
}
