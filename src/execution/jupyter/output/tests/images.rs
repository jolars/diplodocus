use super::super::images::validate_with_assets;
use super::*;
use crate::execution::assets::PageAssetStore;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>";

#[test]
fn safe_html_is_validated_and_selected_before_plain_text() {
    let root = tempfile::tempdir().unwrap();
    let mut assets = assets(root.path());
    let mut reducer = new_reducer();
    reducer
        .accept_cell(
            &cell(0),
            CellOutcome::Ok,
            vec![CellEvent::Display {
                bundle: bundle(
                    json!({"text/html": "<p><strong>safe</strong></p>", "text/plain": "safe"}),
                ),
                display_id: None,
            }],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    let reduced = reducer.finish().unwrap();
    assert_eq!(
        reduced.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("text/html")
    );
    assert!(reduced.diagnostics.is_empty());
}

#[test]
fn nested_markdown_images_are_staged_before_later_cells_and_retained() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("guide")).unwrap();
    let image = root.path().join("guide/plot.svg");
    std::fs::write(&image, SVG).unwrap();
    let mut assets = assets(root.path());
    let mut reducer = new_reducer();
    let mut hidden = cell(0);
    hidden.options.execution.include.value = false;
    reducer
        .accept_cell(
            &hidden,
            CellOutcome::Ok,
            vec![CellEvent::Display {
                bundle: bundle(
                    json!({"text/markdown": "> ![nested](plot.svg)\n", "text/plain": "plot"}),
                ),
                display_id: None,
            }],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    std::fs::remove_file(image).unwrap();
    accept(&mut reducer, 1, vec![display(None, "later")]);
    let reduced = reducer.finish().unwrap();
    assert_eq!(reduced.retained_assets.len(), 1);
    let retained = assets.retain(&reduced.retained_assets).unwrap();
    assert_eq!(
        std::fs::read(&retained.staged_assets[0].path).unwrap(),
        SVG.as_bytes()
    );
}

fn assets(root: &std::path::Path) -> PageAssetStore {
    PageAssetStore::new(page(), root.to_owned(), root.join("staging")).unwrap()
}

fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::new(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn real_image_validator_preserves_alternatives_and_prunes_updated_and_cleared_assets() {
    let root = tempfile::tempdir().unwrap();
    let mut assets = assets(root.path());
    let mut reducer = new_reducer();
    let png = png();
    let mut prepared = cell(0);
    prepared.options.execution.include.value = false;
    reducer.accept_cell(&prepared, CellOutcome::Ok, vec![CellEvent::Display {
        bundle: MimeBundle { data: json!({"text/plain": "plot", "image/png": STANDARD.encode(&png), "image/svg+xml": SVG}), metadata: Map::from_iter([("filename".into(), json!("../../never-write.svg"))]) },
        display_id: Some("figure".into()),
    }], &mut |c| validate_with_assets(c, &mut assets)).unwrap();
    assert_eq!(
        reducer.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(reducer.cells[0].outputs[0].output.representations.len(), 3);
    reducer
        .accept_cell(
            &cell(1),
            CellOutcome::Ok,
            vec![
                CellEvent::UpdateDisplay {
                    bundle: bundle(
                        json!({"image/png": STANDARD.encode(&png), "text/plain": "updated"}),
                    ),
                    display_id: Some("figure".into()),
                },
                CellEvent::Display {
                    bundle: bundle(json!({"image/svg+xml": "<svg><circle r='1'/></svg>"})),
                    display_id: None,
                },
                CellEvent::Clear { wait: false },
            ],
            &mut |c| validate_with_assets(c, &mut assets),
        )
        .unwrap();
    let reduced = reducer.finish().unwrap();
    assert_eq!(reduced.retained_assets.len(), 1);
    assert!(reduced.diagnostics.is_empty());
    assert_eq!(reduced.cells[0].outputs[0].updating_cell, Some(1));
    assert_eq!(
        reduced.cells[0].outputs[0].representations[0].content_fingerprint,
        fingerprint_bytes(&png)
    );
    let result = assets.retain(&reduced.retained_assets).unwrap();
    assert_eq!(result.assets.len(), 1);
    assert_eq!(result.assets[0].media_type, "image/png");
    assert_eq!(std::fs::read(&result.staged_assets[0].path).unwrap(), png);
    assert_eq!(
        std::fs::read_dir(result.staged_assets[0].path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn invalid_images_warn_and_fall_back_even_when_hidden() {
    let root = tempfile::tempdir().unwrap();
    let mut assets = assets(root.path());
    let mut reducer = new_reducer();
    let mut prepared = cell(0);
    prepared.options.execution.output.value = OutputVisibility::Hide;
    reducer.accept_cell(&prepared, CellOutcome::Ok, vec![CellEvent::Display {
        bundle: bundle(json!({"image/svg+xml": "<svg><script/></svg>", "image/png": "not base64", "text/plain": "safe"})),
        display_id: None,
    }], &mut |c| validate_with_assets(c, &mut assets)).unwrap();
    let result = reducer.finish().unwrap();
    assert_eq!(
        result.cells[0].outputs[0].selected_mime_type.as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [
            DiagnosticCode::UnsafeKernelSvg,
            DiagnosticCode::InvalidCellOutput
        ]
    );
    assert!(result.retained_assets.is_empty());
    assert!(!root.path().join("staging").exists());
}

#[test]
fn base64_is_strict_and_string_arrays_are_concatenated() {
    let root = tempfile::tempdir().unwrap();
    let mut assets = assets(root.path());
    let prepared = cell(0);
    let page = page();
    let png = png();
    let base64 = STANDARD.encode(&png);
    let metadata = Map::new();
    let context = AuthoredOutputContext::new(
        page.source.clone(),
        page.collection.clone(),
        Default::default(),
    );
    let make = |data| OutputCandidate {
        context: &context,
        fragment_ordinal: 0,
        page: &page,
        cell: &prepared,
        slot: 0,
        media_type: "image/png",
        data,
        metadata: &metadata,
    };
    let data = json!([&base64[..10], &base64[10..]]);
    assert!(
        validate_with_assets(make(&data), &mut assets)
            .unwrap()
            .accepted
            .is_some()
    );
    let payloads = [
        json!(format!("{base64}\n")),
        json!("AB=="),
        json!("AA"),
        json!(["AA==", 1]),
        json!(null),
    ];
    for payload in &payloads {
        let result = validate_with_assets(make(payload), &mut assets).unwrap();
        assert!(result.accepted.is_none());
        assert_eq!(
            result.diagnostics[0].code(),
            DiagnosticCode::InvalidCellOutput
        );
    }
}

#[cfg(unix)]
#[test]
fn fatal_image_staging_failure_cannot_use_plain_text_fallback_or_be_cleared() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("staging")).unwrap();
    let mut assets = assets(root.path());
    let mut reducer = new_reducer();
    let failure = reducer
        .accept_cell(
            &cell(0),
            CellOutcome::Ok,
            vec![
                CellEvent::Display {
                    bundle: bundle(json!({"image/svg+xml": SVG, "text/plain": "safe"})),
                    display_id: None,
                },
                CellEvent::Clear { wait: false },
            ],
            &mut |c| validate_with_assets(c, &mut assets),
        )
        .unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::AssetOutsideBoundary);
    assert!(reducer.finish().is_err());
    assert!(assets.retain(&[]).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
