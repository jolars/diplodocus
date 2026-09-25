use super::*;

#[tokio::test]
async fn unsupported_mime_bundles_survive_the_protocol_and_cache_without_payloads() {
    let root = TempDir::new().unwrap();
    fixture_kernel(root.path(), "execute-unsupported-mime").await;
    let engine = engine(root.path()).with_cache_root(root.path().join("cache"));
    let (context, request) = request(root.path(), SOURCE);
    let fresh = engine.execute_page(context, &request).await.unwrap();
    let record = fresh.validated().record();
    assert_eq!(record.diagnostics.len(), 2);
    for (ordinal, cell) in record.cells.iter().enumerate() {
        assert_eq!(cell.outcome, crate::execution::CellOutcome::Ok);
        assert_eq!(cell.outputs.len(), 2);
        let unsupported = &cell.outputs[0];
        assert!(unsupported.unsupported_placeholder().is_some());
        assert_eq!(unsupported.diagnostic_indices, [ordinal]);
        assert_eq!(
            unsupported
                .offered_mime_types
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["application/javascript"]
        );
        assert_eq!(
            record.diagnostics[ordinal].code,
            DiagnosticCode::UnsupportedCellOutput
        );
        assert_eq!(record.diagnostics[ordinal].span, Some(cell.span));
        let fallback = &cell.outputs[1];
        assert_eq!(fallback.selected_mime_type.as_deref(), Some("text/plain"));
        assert!(fallback.diagnostic_indices.is_empty());
        assert!(matches!(
            fresh.validated().representation(ordinal, fallback.slot, 0),
            Some(ValidatedRepresentationRef::Text("safe fallback"))
        ));
    }
    assert!(
        !serde_json::to_string(record)
            .unwrap()
            .contains("rejected payload")
    );
    assert!(fresh.staged_assets().is_empty());
    assert_reaped(root.path()).await;
    let (context, request) = super::request(root.path(), SOURCE);
    let cached = engine.execute_page(context, &request).await.unwrap();
    assert_eq!(cached.validated().record().cells, record.cells);
    assert_eq!(cached.validated().record().diagnostics, record.diagnostics);
    assert!(matches!(
        cached
            .validated()
            .record()
            .provenance
            .as_ref()
            .unwrap()
            .execution
            .activity,
        crate::ir::ProvenanceActivity::Execution {
            origin: crate::ir::ExecutionOrigin::Cache,
            ..
        }
    ));
    assert_eq!(
        std::fs::read_to_string(root.path().join("requests"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert!(cached.staged_assets().is_empty());
    assert_reaped(root.path()).await;
}

#[tokio::test]
async fn missing_kernel_fails_before_launch_or_cache_and_asset_writes() {
    let root = TempDir::new().unwrap();
    let (context, request) = request(root.path(), SOURCE);
    let failure = engine(root.path())
        .with_cache_root(root.path().join("cache"))
        .execute_page(context, &request)
        .await
        .unwrap_err();
    assert_eq!(failure.kind, ExecutionFailureKind::Startup);
    assert_eq!(failure.diagnostics.len(), 1);
    assert_eq!(
        failure.diagnostics[0].code,
        DiagnosticCode::ExecutionStartupFailed
    );
    assert!(failure.diagnostics[0].message.contains("not found"));
    assert!(failure.cleanup_diagnostics.is_empty());
    for path in ["observations.json", "requests", "assets", "cache"] {
        assert!(!root.path().join(path).exists(), "unexpected {path}");
    }
}
