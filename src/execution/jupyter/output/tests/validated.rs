use super::super::images::validate_with_assets;
use super::*;
use crate::execution::assets::PageAssetStore;
use crate::execution::{
    PageExecutionRecord, PageExecutionRequest, PreparedExecution, ValidatedPage,
};

const SOURCE: &str = "```{python}\n1\n```\n\n```{python}\n#| include: false\n2\n```\n";

fn prepared() -> PreparedExecution {
    let config = toml::from_str("id='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
    let preparation = prepare_collection_document(SOURCE, &config)
        .unwrap()
        .preparation
        .unwrap();
    let mut page = page();
    page.source_fingerprint = fingerprint_bytes(SOURCE.as_bytes());
    let context = AuthoredOutputContext::new(
        page.source.clone(),
        page.collection.clone(),
        Default::default(),
    );
    PreparedExecution::checked(
        PageExecutionRequest {
            page,
            kernel: "python3".into(),
            defaults: preparation.defaults,
            cells: preparation.cells,
            declared_environment_inputs: vec![],
        },
        SOURCE.as_bytes().to_vec(),
        context,
    )
    .unwrap()
}

fn checked(
    prepared: &PreparedExecution,
    reduced: ReducedPage,
    assets: &mut PageAssetStore,
) -> ValidatedPage {
    let record = PageExecutionRecord {
        page: prepared.request().page.clone(),
        defaults: prepared.request().defaults.clone(),
        cells: reduced.cells,
        diagnostics: reduced.diagnostics,
        assets: reduced.assets,
        provenance: None,
    };
    ValidatedPage::checked(
        prepared,
        record,
        reduced.slots,
        reduced.execution_diagnostics,
        assets,
    )
    .unwrap()
}

#[test]
fn repeated_updates_at_one_producer_slot_keep_distinct_validated_fragments() {
    let prepared = prepared();
    let request = prepared.request();
    let root = tempfile::tempdir().unwrap();
    let mut assets = PageAssetStore::new(
        request.page.clone(),
        root.path().into(),
        root.path().join("staging"),
    )
    .unwrap();
    let mut reducer = OutputReducer::with_context(
        request.page.clone(),
        ErrorContext::new(root.path().into()),
        prepared.context().clone(),
    );
    reducer
        .accept_cell(
            &request.cells[0],
            CellOutcome::Ok,
            vec![display(Some("a"), "old a"), display(Some("b"), "old b")],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    let update = |id: &str, text: &str| CellEvent::UpdateDisplay {
        bundle: bundle(json!({"text/markdown": text})),
        display_id: Some(id.into()),
    };
    reducer
        .accept_cell(
            &request.cells[1],
            CellOutcome::Ok,
            vec![
                update("a", "first"),
                update("b", "second"),
                update("a", "third"),
            ],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    let checked = checked(&prepared, reducer.finish().unwrap(), &mut assets);
    for (slot, fragment) in [(0, 2), (1, 1)] {
        let origin = checked.representation_origin(0, slot, 0).unwrap();
        assert_eq!(origin.cell, 1);
        assert_eq!(origin.slot, 0);
        assert_eq!(origin.fragment.unwrap().ordinal, fragment);
        let output = &checked.record().cells[0].outputs[slot];
        assert_eq!(output.producing_cell, 0);
        assert_eq!(output.updating_cell, Some(1));
        assert_eq!(
            checked
                .canonical_representation(0, slot, 0)
                .unwrap()
                .unwrap()
                .fingerprint,
            output.representations[0].content_fingerprint
        );
    }
    assert_ne!(
        checked.representation(0, 0, 0).map(|v| format!("{v:?}")),
        checked.representation(0, 1, 0).map(|v| format!("{v:?}"))
    );
}

#[test]
fn nested_image_rejections_remain_valid_typed_warnings_with_a_safe_fallback() {
    for (file, bytes) in [
        ("bad.png", "not a PNG"),
        ("bad.svg", "<svg><script>bad()</script></svg>"),
    ] {
        for html in [false, true] {
            let prepared = prepared();
            let request = prepared.request();
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("guide")).unwrap();
            std::fs::write(root.path().join("guide").join(file), bytes).unwrap();
            let mut assets = PageAssetStore::new(
                request.page.clone(),
                root.path().into(),
                root.path().join("staging"),
            )
            .unwrap();
            let mut reducer = OutputReducer::with_context(
                request.page.clone(),
                ErrorContext::new(root.path().into()),
                prepared.context().clone(),
            );
            let data = if html {
                json!({"text/html": format!("<img src='{file}'>"), "text/plain":"fallback"})
            } else {
                json!({"text/markdown": format!("![bad]({file})"), "text/plain":"fallback"})
            };
            reducer
                .accept_cell(
                    &request.cells[0],
                    CellOutcome::Ok,
                    vec![CellEvent::Display {
                        bundle: bundle(data),
                        display_id: None,
                    }],
                    &mut |candidate| validate_with_assets(candidate, &mut assets),
                )
                .unwrap();
            reducer
                .accept_cell(
                    &request.cells[1],
                    CellOutcome::Ok,
                    vec![],
                    &mut |candidate| validate_with_assets(candidate, &mut assets),
                )
                .unwrap();
            let checked = checked(&prepared, reducer.finish().unwrap(), &mut assets);
            assert_eq!(
                checked.record().cells[0].outputs[0]
                    .selected_mime_type
                    .as_deref(),
                Some("text/plain")
            );
            assert!(!checked.diagnostics().is_empty());
            assert!(checked.record().assets.is_empty());
        }
    }
}

#[test]
fn final_assets_include_nested_alternatives_and_exclude_replaced_and_cleared_images() {
    let prepared = prepared();
    let request = prepared.request();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("guide")).unwrap();
    let mut digests = Vec::new();
    for index in 0..4 {
        let bytes = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'><rect width='{index}' height='10'/></svg>"
        );
        std::fs::write(root.path().join(format!("guide/{index}.svg")), &bytes).unwrap();
        digests.push(fingerprint_bytes(bytes.as_bytes()));
    }
    let mut assets = PageAssetStore::new(
        request.page.clone(),
        root.path().into(),
        root.path().join("staging"),
    )
    .unwrap();
    let mut reducer = OutputReducer::with_context(
        request.page.clone(),
        ErrorContext::new(root.path().into()),
        prepared.context().clone(),
    );
    reducer
        .accept_cell(
            &request.cells[0],
            CellOutcome::Ok,
            vec![
                CellEvent::Display {
                    bundle: bundle(json!({"text/markdown": "![old](0.svg)"})),
                    display_id: Some("a".into()),
                },
                CellEvent::Display {
                    bundle: bundle(
                        json!({"text/markdown":"selected text", "text/html":"<img src='1.svg'>"}),
                    ),
                    display_id: Some("b".into()),
                },
            ],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    reducer
        .accept_cell(
            &request.cells[1],
            CellOutcome::Ok,
            vec![
                CellEvent::UpdateDisplay {
                    bundle: bundle(json!({"text/markdown": "> ![new](2.svg)"})),
                    display_id: Some("a".into()),
                },
                CellEvent::Display {
                    bundle: bundle(json!({"text/markdown":"![clear](3.svg)"})),
                    display_id: None,
                },
                CellEvent::Clear { wait: false },
            ],
            &mut |candidate| validate_with_assets(candidate, &mut assets),
        )
        .unwrap();
    let checked = checked(&prepared, reducer.finish().unwrap(), &mut assets);
    assert!(checked.record().cells[1].outputs.is_empty());
    let retained = crate::execution::PageExecutionResult::retain(checked, assets).unwrap();
    let mut expected = vec![digests[1].value.clone(), digests[2].value.clone()];
    expected.sort();
    assert_eq!(
        retained
            .validated()
            .record()
            .assets
            .iter()
            .map(|a| a.reference.fingerprint.value.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(retained.staged_assets().len(), 2);
    assert_eq!(
        std::fs::read_dir(retained.staged_assets()[0].path.parent().unwrap())
            .unwrap()
            .count(),
        2
    );
}
