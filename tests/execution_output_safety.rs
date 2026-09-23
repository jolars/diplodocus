use std::collections::BTreeSet;
use std::fs;

use diplodocus::configuration::ExecutionMode;
use diplodocus::diagnostics::DiagnosticPath;
use diplodocus::documents::{AuthoredFormat, MarkdownFragmentOrigin, parse_markdown_fragment};
use diplodocus::execution::assets::PageAssetStore;
use diplodocus::execution::output_safety::*;
use diplodocus::execution::{ExecutionFailureKind, ExecutionPage};
use diplodocus::ir::{SourceLocation, SourceSpan};
use diplodocus::provenance::fingerprint_bytes;

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="2" height="3"/></svg>"#;

fn page() -> ExecutionPage {
    ExecutionPage {
        source: SourceLocation {
            repository: "repo".into(),
            path: DiagnosticPath::try_from("guide/page.qmd").unwrap(),
            span: None,
        },
        collection: "guide".into(),
        working_directory: Some(DiagnosticPath::try_from("guide").unwrap()),
        source_fingerprint: fingerprint_bytes(b"source"),
        format: AuthoredFormat::Qmd,
        mode: ExecutionMode::Execute,
        page_veto: false,
        parser_version: "0.29.2".into(),
        qmd_policy: "qmd-mvp-v1".into(),
    }
}

fn setup() -> (tempfile::TempDir, PageAssetStore, AuthoredOutputContext) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("guide")).unwrap();
    fs::write(root.path().join("guide/image.svg"), SVG).unwrap();
    let assets =
        PageAssetStore::new(page(), root.path().to_owned(), root.path().join("staging")).unwrap();
    let context = AuthoredOutputContext::new(
        page().source,
        "guide".into(),
        BTreeSet::from(["authored".into()]),
    );
    (root, assets, context)
}

fn origin() -> OutputOrigin {
    OutputOrigin {
        cell: 2,
        slot: 4,
        cell_span: SourceSpan { start: 10, end: 90 },
        fragment: None,
    }
}

fn accepted<T: std::fmt::Debug>(value: Validation<T>) -> T {
    match value {
        Validation::Accepted { value, .. } => value,
        other => panic!("{other:?}"),
    }
}

#[test]
fn html_canonicalizes_inert_markup_and_rejects_active_content() {
    let (_root, mut assets, context) = setup();
    let safe = accepted(validate_html_live("<P class='x' id='x' data-x='x' title='&quot;&amp;'>a &amp; b<!--x--><BR></P><ol start='+0002'><li>c</li></ol>", &origin(), &context, &mut assets).unwrap());
    assert_eq!(
        safe.canonical_content().markup,
        "<p title=\"&quot;&amp;\">a &amp; b<br></p><ol start=\"2\"><li>c</li></ol>"
    );
    for markup in [
        "<script>alert(1)</script>",
        "<p style='color:red'>x</p>",
        "<svg></svg>",
        "<p onclick='x'>x</p>",
        "<iframe></iframe>",
        "<a href='java&#x73;cript:alert(1)'>x</a>",
        "<a href='%6aavascript:alert(1)'>x</a>",
        "<a href='//evil.test'>x</a>",
        "<img src='https://evil.test/x.png'>",
        "<a href='#generated'>x</a>",
    ] {
        assert!(
            matches!(validate_html_live(markup, &origin(), &context, &mut assets).unwrap(), Validation::Rejected { diagnostics } if diagnostics.len() == 1),
            "{markup}"
        );
    }
}

#[test]
fn html_images_are_typed_and_restore_only_from_verified_bytes() {
    let (root, mut assets, context) = setup();
    let value = accepted(
        validate_html_live(
            "<p><img src='image.svg'><img src='image.svg'></p>",
            &origin(),
            &context,
            &mut assets,
        )
        .unwrap(),
    );
    assert_eq!(value.referenced_assets().count(), 2);
    let asset = value.referenced_assets().next().unwrap().clone();
    let decoded = value.canonical_content().clone();
    assert!(matches!(
        restore_html(decoded.clone(), &origin(), &context, &VerifiedAssets::new()),
        Err(RestoreRejection::UnboundAsset)
    ));
    let mut verified = VerifiedAssets::new();
    verified.stage(&mut assets, &asset, SVG).unwrap();
    fs::remove_file(root.path().join("guide/image.svg")).unwrap();
    let restored = restore_html(decoded.clone(), &origin(), &context, &verified).unwrap();
    assert_eq!(restored.canonical_content(), &decoded);
    assert!(matches!(
        validate_html_live(&decoded.markup, &origin(), &context, &mut assets).unwrap(),
        Validation::Rejected { .. }
    ));
    assert!(matches!(
        restore_html(
            DecodedHtml {
                markup: "<P>x</P>".into()
            },
            &origin(),
            &context,
            &verified
        ),
        Err(RestoreRejection::NonCanonical)
    ));
}

#[test]
fn missing_and_escaping_images_are_fatal() {
    for target in ["missing.svg", "../../outside.svg", "/tmp/outside.svg"] {
        let (_root, mut assets, context) = setup();
        let error = validate_html_live(
            &format!("<img src='{target}'>"),
            &origin(),
            &context,
            &mut assets,
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            if target == "missing.svg" {
                ExecutionFailureKind::AssetMissing
            } else {
                ExecutionFailureKind::AssetOutsideBoundary
            }
        );
    }
}

#[test]
fn markdown_binds_repeated_nested_images_and_keeps_fragment_identity() {
    let (root, mut assets, context) = setup();
    let source = "- **![one](image.svg)** and ![two](image.svg)\n\n[link](#authored)\n";
    let mut origin = origin();
    origin.fragment = Some(FragmentIdentity {
        ordinal: 7,
        byte_length: source.len(),
    });
    let fragment = parse_markdown_fragment(
        source,
        MarkdownFragmentOrigin {
            collection: "guide".into(),
            cell: origin.cell,
            output: origin.slot,
            source: SourceLocation {
                span: Some(origin.cell_span),
                ..page().source
            },
        },
    );
    let value = accepted(validate_markdown_live(fragment, &origin, &context, &mut assets).unwrap());
    assert_eq!(value.referenced_assets().count(), 2);
    assert_ne!(
        value.image_bindings()[0].address(),
        value.image_bindings()[1].address()
    );
    let mut verified = VerifiedAssets::new();
    for asset in value.referenced_assets() {
        verified.stage(&mut assets, asset, SVG).unwrap();
    }
    fs::remove_file(root.path().join("guide/image.svg")).unwrap();
    let restored = restore_markdown(
        value.canonical_content().clone(),
        &origin,
        &context,
        &verified,
    )
    .unwrap();
    assert_eq!(restored.blocks(), value.blocks());
}

#[test]
fn validated_values_are_owned_send_and_sync() {
    fn assert_owned<T: Send + Sync + 'static>() {}
    assert_owned::<ValidatedHtml>();
    assert_owned::<ValidatedMarkdown>();
    assert_owned::<VerifiedAssets>();
}

fn parsed(source: &str, origin: &OutputOrigin) -> diplodocus::documents::MarkdownFragmentParse {
    parse_markdown_fragment(
        source,
        MarkdownFragmentOrigin {
            collection: "guide".into(),
            cell: origin.cell,
            output: origin.slot,
            source: SourceLocation {
                span: Some(origin.cell_span),
                ..page().source
            },
        },
    )
}

fn fragment_origin(source: &str) -> OutputOrigin {
    OutputOrigin {
        fragment: Some(FragmentIdentity {
            ordinal: 9,
            byte_length: source.len(),
        }),
        ..origin()
    }
}

#[test]
fn fragment_provenance_and_ranges_are_checked_before_granting_trust() {
    let (_root, mut assets, context) = setup();
    let source = "hello\n";
    let origin = fragment_origin(source);
    let mut fragment = parsed(source, &origin);
    fragment.provenance.span = None;
    assert!(matches!(
        validate_markdown_live(fragment, &origin, &context, &mut assets).unwrap(),
        Validation::Rejected { .. }
    ));
    let value = accepted(
        validate_markdown_live(parsed(source, &origin), &origin, &context, &mut assets).unwrap(),
    );
    let mut bad = value.canonical_content().clone();
    if let FragmentBlock::Paragraph { span, .. } = &mut bad.blocks[0] {
        span.end += 1;
    }
    assert_eq!(
        restore_markdown(bad, &origin, &context, &VerifiedAssets::new()).unwrap_err(),
        RestoreRejection::Structure
    );
    let mut bad = value.canonical_content().clone();
    if let FragmentBlock::Paragraph { inlines, .. } = &mut bad.blocks[0] {
        inlines.push(FragmentInline::SemanticReference {
            target: "repo::symbol".into(),
            span: SourceSpan { start: 0, end: 3 },
            target_span: SourceSpan { start: 0, end: 4 },
        });
    }
    assert_eq!(
        restore_markdown(bad, &origin, &context, &VerifiedAssets::new()).unwrap_err(),
        RestoreRejection::Structure
    );
}

#[test]
fn markdown_remains_inert_and_retains_typed_parser_warnings() {
    let (_root, mut assets, context) = setup();
    let source = "# Generated\n\n```{python}\n#| eval: true\nprint(1)\n```\n\n<div onclick='evil()'>raw</div>\n\n> [!NOTE]\n> safe\n";
    let origin = fragment_origin(source);
    let fragment = parsed(source, &origin);
    let original_provenance = fragment.provenance.clone();
    let original_diagnostics = fragment.diagnostics.clone();
    let Validation::Accepted { value, diagnostics } =
        validate_markdown_live(fragment, &origin, &context, &mut assets).unwrap()
    else {
        panic!("inert fragment rejected");
    };
    assert_eq!(value.provenance(), &original_provenance);
    assert_eq!(diagnostics.len(), original_diagnostics.len());
    assert!(!diagnostics.is_empty());
    for (warning, original) in diagnostics.iter().zip(original_diagnostics) {
        assert!(matches!(
            warning,
            ExecutionDiagnostic::FragmentUnsupported { .. }
        ));
        assert_eq!(warning.attribution().source, None);
        assert_eq!(warning.attribution().span, original.span);
        assert_eq!(warning.attribution().fragment, origin.fragment);
    }
    assert!(value.blocks().iter().any(|block| matches!(block, diplodocus::ir::Block::CodeBlock { source, .. } if source.contains("#| eval: true"))));
    assert!(!context.anchors().contains("generated"));
}

#[test]
fn decoded_url_attacks_fail_for_both_formats() {
    let (_root, mut assets, context) = setup();
    for target in [
        "javascript:alert",
        "java%0ascript:alert",
        "%256aavascript:alert",
        "&#x6a;avascript:alert",
        "file:///etc/passwd",
        "data:text/html,evil",
        "blob:https://example.org/id",
        "//evil.test/x",
        "%2f%2fevil.test/x",
        "https:%5c%5cevil.test",
        "../../outside",
        "%2e%2e/%2e%2e/outside",
        "#generated",
    ] {
        let markup = format!("<a href='{target}'>link</a>");
        assert!(
            matches!(
                validate_html_live(&markup, &origin(), &context, &mut assets).unwrap(),
                Validation::Rejected { .. }
            ),
            "HTML {target}"
        );
        let source = format!("[link]({target})\n");
        let origin = fragment_origin(&source);
        assert!(
            matches!(
                validate_markdown_live(parsed(&source, &origin), &origin, &context, &mut assets)
                    .unwrap(),
                Validation::Rejected { .. }
            ),
            "Markdown {target}"
        );
    }
    for target in [
        "https://example.org/x?a=1&amp;b=2",
        "http://example.org",
        "mailto:a@example.org",
        "../other.qmd",
        "sub/page.qmd#other",
        "#authored",
    ] {
        let markup = format!("<a href='{target}'>link</a>");
        assert!(
            matches!(
                validate_html_live(&markup, &origin(), &context, &mut assets).unwrap(),
                Validation::Accepted { .. }
            ),
            "{target}"
        );
    }
}

#[test]
fn html_checks_tokens_that_tree_construction_would_discard() {
    let (_root, mut assets, context) = setup();
    for markup in [
        "<html><p>safe</p></html>",
        "<head></head><p>safe</p>",
        "<body onload='evil()'><p>safe</p>",
        "<p title='a' title='b'>x</p>",
        "<table><input></table>",
        "<a xlink:href='x'>x</a>",
        "<img>",
        "<img srcset='evil 2x' src='image.svg'>",
        "<img src='image.svg' width='0'>",
        "<ol start='1e2'><li>x</li></ol>",
        "<td scope='evil'>x</td>",
    ] {
        assert!(
            matches!(
                validate_html_live(markup, &origin(), &context, &mut assets).unwrap(),
                Validation::Rejected { .. }
            ),
            "{markup}"
        );
    }
    let value = accepted(
        validate_html_live(
            "<table><tr><td title='A &amp; B'>x</td></tr></table>",
            &origin(),
            &context,
            &mut assets,
        )
        .unwrap(),
    );
    assert_eq!(
        value.canonical_content().markup,
        "<table><tbody><tr><td title=\"A &amp; B\">x</td></tr></tbody></table>"
    );
}

#[test]
fn invalid_image_bytes_reject_candidates_but_symlink_escapes_are_fatal() {
    let (root, mut assets, context) = setup();
    fs::write(
        root.path().join("guide/bad.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'><script/></svg>",
    )
    .unwrap();
    let rejected =
        validate_html_live("<img src='bad.svg'>", &origin(), &context, &mut assets).unwrap();
    assert!(
        matches!(rejected, Validation::Rejected { diagnostics } if matches!(&diagnostics[..], [ExecutionDiagnostic::SvgRejected { .. }]))
    );
    assert!(matches!(
        validate_html_live("<p>fallback</p>", &origin(), &context, &mut assets).unwrap(),
        Validation::Accepted { .. }
    ));
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("image.svg"), SVG).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("image.svg"),
            root.path().join("guide/link.svg"),
        )
        .unwrap();
        let source = "![escape](link.svg)";
        let origin = fragment_origin(source);
        assert_eq!(
            validate_markdown_live(parsed(source, &origin), &origin, &context, &mut assets)
                .unwrap_err()
                .kind,
            ExecutionFailureKind::AssetOutsideBoundary
        );
    }
}

#[test]
fn restore_rejects_wrong_asset_metadata_and_noncanonical_aliases() {
    let (_root, mut assets, context) = setup();
    let value = accepted(
        validate_html_live("<img src='image.svg'>", &origin(), &context, &mut assets).unwrap(),
    );
    let asset = value.referenced_assets().next().unwrap().clone();
    let mut verified = VerifiedAssets::new();
    let mut wrong = asset.clone();
    wrong.byte_size += 1;
    assert!(verified.stage(&mut assets, &wrong, SVG).is_err());
    assert_eq!(verified.assets().count(), 0);
    verified.stage(&mut assets, &asset, SVG).unwrap();
    for markup in [
        value
            .canonical_content()
            .markup
            .replace("diplodocus-asset:", "diplodocus-asset&#58;"),
        value.canonical_content().markup.replace("<img", "<IMG"),
        format!("{}<!--comment-->", value.canonical_content().markup),
    ] {
        assert_eq!(
            restore_html(DecodedHtml { markup }, &origin(), &context, &verified).unwrap_err(),
            RestoreRejection::NonCanonical
        );
    }
    let source = "![image](image.svg)";
    let origin = fragment_origin(source);
    let value = accepted(
        validate_markdown_live(parsed(source, &origin), &origin, &context, &mut assets).unwrap(),
    );
    let mut decoded = value.canonical_content().clone();
    if let FragmentBlock::Paragraph { inlines, .. } = &mut decoded.blocks[0]
        && let FragmentInline::Image { asset, .. } = &mut inlines[0]
    {
        asset.media_type = "image/png".into();
    }
    assert_eq!(
        restore_markdown(decoded, &origin, &context, &verified).unwrap_err(),
        RestoreRejection::AssetMismatch
    );
}

#[test]
fn foreign_page_assets_and_invalid_html_origins_are_rejected() {
    let (_root, mut assets, context) = setup();
    let value = accepted(
        validate_html_live("<img src='image.svg'>", &origin(), &context, &mut assets).unwrap(),
    );
    let mut verified = VerifiedAssets::new();
    verified
        .stage(&mut assets, value.referenced_assets().next().unwrap(), SVG)
        .unwrap();
    let other_context = AuthoredOutputContext::new(
        SourceLocation {
            path: DiagnosticPath::try_from("other.qmd").unwrap(),
            ..page().source
        },
        "guide".into(),
        BTreeSet::new(),
    );
    assert_eq!(
        restore_html(
            value.canonical_content().clone(),
            &origin(),
            &other_context,
            &verified
        )
        .unwrap_err(),
        RestoreRejection::AssetMismatch
    );
    let invalid_origin = OutputOrigin {
        fragment: Some(FragmentIdentity {
            ordinal: 1,
            byte_length: 1,
        }),
        ..origin()
    };
    assert!(matches!(
        validate_html_live("<p>x</p>", &invalid_origin, &context, &mut assets).unwrap(),
        Validation::Rejected { .. }
    ));
}

#[test]
fn fragment_warnings_survive_later_rejection_and_fatal_asset_failure() {
    for suffix in [
        "[bad](javascript:evil)",
        "![bad](missing.svg)",
        "![bad](../../escape.svg)",
    ] {
        let (_root, mut assets, context) = setup();
        let source = format!("<div>escaped raw HTML</div>\n\n{suffix}\n");
        let origin = fragment_origin(&source);
        let fragment = parsed(&source, &origin);
        assert_eq!(fragment.diagnostics.len(), 1);
        match validate_markdown_live(fragment, &origin, &context, &mut assets) {
            Ok(Validation::Rejected { diagnostics }) => {
                assert_eq!(diagnostics.len(), 2);
                assert!(matches!(
                    diagnostics[0],
                    ExecutionDiagnostic::FragmentUnsupported { .. }
                ));
                assert!(matches!(
                    diagnostics[1],
                    ExecutionDiagnostic::MarkdownRejected { .. }
                ));
            }
            Err(failure) => {
                assert_eq!(failure.diagnostics.len(), 2);
                assert_eq!(
                    failure.diagnostics[0].code,
                    diplodocus::diagnostics::DiagnosticCode::UnsupportedAuthoredSyntax
                );
                assert_eq!(failure.diagnostics[0].source, None);
            }
            other => panic!("unexpected validation: {other:?}"),
        }
    }
}

#[test]
fn structural_bindings_cover_captions_cells_and_nested_alt_with_equal_spans() {
    use diplodocus::ir::{Attributes, TableAlignment};
    let (_root, mut assets, context) = setup();
    let asset = assets.stage_bytes("image/svg+xml", SVG).unwrap();
    let mut verified = VerifiedAssets::new();
    verified.stage(&mut assets, &asset, SVG).unwrap();
    let span = SourceSpan { start: 0, end: 1 };
    let image = FragmentInline::Image {
        alt: vec![],
        asset: AssetUse::from(&asset),
        title: None,
        attributes: Attributes::default(),
        span,
    };
    let caption = FragmentInline::Image {
        alt: vec![image.clone()],
        asset: AssetUse::from(&asset),
        title: None,
        attributes: Attributes::default(),
        span,
    };
    let decoded = DecodedMarkdown {
        blocks: vec![FragmentBlock::Table {
            caption: vec![caption],
            alignments: vec![TableAlignment::Default, TableAlignment::Default],
            rows: vec![FragmentTableRow {
                header: true,
                cells: vec![
                    FragmentTableCell {
                        blocks: vec![FragmentBlock::Paragraph {
                            inlines: vec![image.clone(), image.clone()],
                            span,
                        }],
                        span,
                    },
                    FragmentTableCell {
                        blocks: vec![FragmentBlock::Paragraph {
                            inlines: vec![image],
                            span,
                        }],
                        span,
                    },
                ],
                span,
            }],
            span,
        }],
    };
    let value = restore_markdown(decoded, &fragment_origin("x"), &context, &verified).unwrap();
    use NodeEdge::*;
    let expected = [
        vec![(Blocks, 0), (Caption, 0)],
        vec![(Blocks, 0), (Caption, 0), (Alt, 0)],
        vec![
            (Blocks, 0),
            (Rows, 0),
            (Cells, 0),
            (Blocks, 0),
            (Inlines, 0),
        ],
        vec![
            (Blocks, 0),
            (Rows, 0),
            (Cells, 0),
            (Blocks, 0),
            (Inlines, 1),
        ],
        vec![
            (Blocks, 0),
            (Rows, 0),
            (Cells, 1),
            (Blocks, 0),
            (Inlines, 0),
        ],
    ];
    assert_eq!(
        value
            .image_bindings()
            .iter()
            .map(|b| b.address())
            .collect::<Vec<_>>(),
        expected.iter().map(|p| p.as_slice()).collect::<Vec<_>>()
    );
    assert_eq!(value.referenced_assets().count(), 5);
    assert_eq!(verified.assets().count(), 1);
}

#[test]
fn live_gfm_tables_preserve_nested_images() {
    let (_root, mut assets, context) = setup();
    for source in [
        "| A | B |\n|---|---|\n| ![x](image.svg) | **![y](image.svg)** |\n",
        "| A | B |\n|---|---|\n| ![x](image.svg) |\n",
        "| A | B |\n|---|---|\n| one | two | ![extra](image.svg) |\n",
    ] {
        let origin = fragment_origin(source);
        let value = accepted(
            validate_markdown_live(parsed(source, &origin), &origin, &context, &mut assets)
                .unwrap(),
        );
        assert!(value.referenced_assets().count() >= 1);
        let mut verified = VerifiedAssets::new();
        for asset in value.referenced_assets() {
            verified.stage(&mut assets, asset, SVG).unwrap();
        }
        let restored = restore_markdown(
            value.canonical_content().clone(),
            &origin,
            &context,
            &verified,
        )
        .unwrap();
        assert_eq!(restored.blocks(), value.blocks());
        assert_eq!(restored.image_bindings(), value.image_bindings());
    }
}

#[tokio::test]
async fn public_validation_can_cross_an_await_in_a_send_future() {
    fn require_send<T: std::future::Future + Send>(future: T) -> T {
        future
    }
    require_send(async {
        let (_root, mut assets, context) = setup();
        let html = accepted(
            validate_html_live("<img src='image.svg'>", &origin(), &context, &mut assets).unwrap(),
        );
        let source = "![x](image.svg)";
        let origin = fragment_origin(source);
        let markdown = accepted(
            validate_markdown_live(parsed(source, &origin), &origin, &context, &mut assets)
                .unwrap(),
        );
        tokio::task::yield_now().await;
        assert_eq!(html.referenced_assets().count(), 1);
        assert_eq!(markdown.referenced_assets().count(), 1);
    })
    .await;
}

#[test]
fn active_validators_match_the_frozen_artifact_including_hidden_images() {
    use serde_json::{Value, json};
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/spikes/fixtures/execution-artifact-v1");
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    let mut page = page();
    page.source.repository = "python".into();
    page.source.path = DiagnosticPath::try_from("docs/artifact.qmd").unwrap();
    page.working_directory = Some(DiagnosticPath::try_from("docs").unwrap());
    let context = AuthoredOutputContext::new(
        page.source.clone(),
        page.collection.clone(),
        BTreeSet::new(),
    );
    let mut store = PageAssetStore::new(
        page.clone(),
        root.path().to_owned(),
        root.path().join("staging"),
    )
    .unwrap();
    let mut verified = VerifiedAssets::new();
    for usage in manifest["result"]["assets"].as_array().unwrap() {
        let digest = usage["digest"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap();
        let bytes = fs::read(fixture.join("assets/sha256").join(digest)).unwrap();
        let asset = store
            .stage_bytes(usage["media_type"].as_str().unwrap(), &bytes)
            .unwrap();
        assert_eq!(asset.reference.fingerprint.value, digest);
        assert_eq!(asset.byte_size, usage["byte_size"].as_u64().unwrap());
        verified.stage(&mut store, &asset, &bytes).unwrap();
        let name = if bytes.len() == 106 {
            "figure.svg"
        } else {
            "hidden.svg"
        };
        fs::write(root.path().join("docs").join(name), bytes).unwrap();
    }
    let cells = manifest["result"]["cells"].as_array().unwrap();
    let mut referenced = BTreeSet::new();
    let mut markdown_count = 0;
    for cell in cells {
        for output in cell["outputs"].as_array().unwrap() {
            for rep in output["representations"].as_array().unwrap() {
                let producer = rep["producing_cell"].as_u64().unwrap() as usize;
                let mut origin = OutputOrigin {
                    cell: producer,
                    slot: output["slot"].as_u64().unwrap() as usize,
                    cell_span: serde_json::from_value(cells[producer]["span"].clone()).unwrap(),
                    fragment: None,
                };
                if rep["kind"] == "html-candidate" {
                    let html = restore_html(
                        DecodedHtml {
                            markup: rep["content"]["markup"].as_str().unwrap().into(),
                        },
                        &origin,
                        &context,
                        &verified,
                    )
                    .unwrap();
                    assert_eq!(
                        format!(
                            "sha256:{}",
                            fingerprint_bytes(html.canonical_content().markup.as_bytes()).value
                        ),
                        rep["content_digest"]
                    );
                    referenced.extend(
                        html.referenced_assets()
                            .map(|a| a.reference.fingerprint.value.clone()),
                    );
                } else if rep["kind"] == "markdown" {
                    let file = if markdown_count == 0 {
                        "fragment.md"
                    } else {
                        "hidden-fragment.md"
                    };
                    markdown_count += 1;
                    let source = fs::read_to_string(fixture.join(file)).unwrap();
                    origin.slot = rep["fragment"]["slot"].as_u64().unwrap() as usize;
                    origin.fragment = Some(FragmentIdentity {
                        ordinal: rep["fragment"]["ordinal"].as_u64().unwrap() as usize,
                        byte_length: source.len(),
                    });
                    let fragment = parse_markdown_fragment(
                        &source,
                        MarkdownFragmentOrigin {
                            collection: context.collection().into(),
                            cell: origin.cell,
                            output: origin.slot,
                            source: SourceLocation {
                                span: Some(origin.cell_span),
                                ..page.source.clone()
                            },
                        },
                    );
                    let value = accepted(
                        validate_markdown_live(fragment, &origin, &context, &mut store).unwrap(),
                    );
                    let mut canonical = json!({"blocks": value.blocks()});
                    for binding in value.image_bindings() {
                        let mut node = &mut canonical;
                        for (edge, index) in binding.address() {
                            let edge = match edge {
                                NodeEdge::Blocks => "blocks",
                                NodeEdge::Inlines => "inlines",
                                NodeEdge::Items => "items",
                                NodeEdge::Rows => "rows",
                                NodeEdge::Cells => "cells",
                                NodeEdge::Caption => "caption",
                                NodeEdge::Alt => "alt",
                            };
                            node = &mut node[edge][*index];
                        }
                        node.as_object_mut().unwrap().remove("target");
                        let asset = binding.asset();
                        node["asset"] = json!({"digest": format!("sha256:{}", asset.reference.fingerprint.value), "media_type": asset.media_type, "byte_size": asset.byte_size});
                    }
                    assert_eq!(canonical, rep["content"]);
                    let restored = restore_markdown(
                        value.canonical_content().clone(),
                        &origin,
                        &context,
                        &verified,
                    )
                    .unwrap();
                    assert_eq!(restored.blocks(), value.blocks());
                    referenced.extend(
                        value
                            .referenced_assets()
                            .map(|a| a.reference.fingerprint.value.clone()),
                    );
                }
            }
        }
    }
    assert_eq!(markdown_count, 2);
    assert_eq!(
        referenced.len(),
        2,
        "hidden, unselected images remain in the asset closure"
    );
}
