use super::*;
use crate::configuration::ContentConfiguration;
use crate::documents::prepare_collection_document;
use crate::execution::ExecutionEngine;
use crate::execution::assets::PageAssetStore;
use crate::ir::*;
use crate::provenance::fingerprint_bytes;
use std::collections::BTreeSet;

const SOURCE: &str =
    "```{python}\n1\n```\n\n```{python}\n#| include: false\n#| output: false\n2\n```\n";
fn inputs() -> (PageExecutionRequest, AuthoredOutputContext) {
    inputs_from(SOURCE)
}
fn inputs_from(source: &str) -> (PageExecutionRequest, AuthoredOutputContext) {
    let config: ContentConfiguration = toml::from_str("id='guide'\nowner='project'\nrepository='repo'\npath='.'\nmount=''\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
    let prepared = prepare_collection_document(source, &config)
        .unwrap()
        .preparation
        .unwrap();
    let page = ExecutionPage {
        source: SourceLocation {
            repository: "repo".into(),
            path: "page.qmd".try_into().unwrap(),
            span: None,
        },
        collection: "guide".into(),
        working_directory: None,
        source_fingerprint: fingerprint_bytes(source.as_bytes()),
        format: crate::documents::AuthoredFormat::Qmd,
        mode: crate::configuration::ExecutionMode::Execute,
        page_veto: false,
        parser_version: crate::provenance::PANACHE_VERSION.into(),
        qmd_policy: "qmd-mvp-v1".into(),
    };
    let context = AuthoredOutputContext::new(
        page.source.clone(),
        page.collection.clone(),
        BTreeSet::new(),
    );
    (
        PageExecutionRequest {
            page,
            kernel: "python3".into(),
            defaults: prepared.defaults,
            cells: prepared.cells,
            declared_environment_inputs: vec![],
        },
        context,
    )
}
fn prepared() -> PreparedExecution {
    let (request, context) = inputs();
    PreparedExecution::checked(request, SOURCE.as_bytes().to_vec(), context).unwrap()
}
fn record(prepared: &PreparedExecution) -> PageExecutionRecord {
    let request = prepared.request();
    PageExecutionRecord {
        page: request.page.clone(),
        defaults: request.defaults.clone(),
        cells: request
            .cells
            .iter()
            .map(|c| CellExecutionResult {
                ordinal: c.ordinal,
                language: c.cell.language.clone(),
                span: c.cell.span,
                source_segments: c.cell.source_segments.clone(),
                submitted_source_fingerprint: Some(fingerprint_bytes(c.cell.source.as_bytes())),
                options: c.options.clone(),
                outcome: CellOutcome::Ok,
                outputs: vec![],
            })
            .collect(),
        diagnostics: vec![],
        assets: vec![],
        provenance: None,
    }
}
fn text(owner: usize, slot: usize, value: &str) -> ExecutionOutput {
    ExecutionOutput {
        owning_cell: owner,
        producing_cell: 0,
        updating_cell: Some(1),
        slot,
        output: CellOutput {
            kind: CellOutputKind::Display,
            representations: vec![OutputRepresentation::PlainText {
                media_type: "text/plain".into(),
                text: value.into(),
            }],
            provenance: vec![],
        },
        offered_mime_types: BTreeSet::from(["text/plain".into()]),
        selected_mime_type: Some("text/plain".into()),
        representations: vec![RepresentationEvidence {
            content_fingerprint: fingerprint_bytes(value.as_bytes()),
            producing_cell: 1,
            policy: None,
        }],
        diagnostic_indices: vec![],
    }
}
fn evidence(owner: usize, slot: usize, value: &str) -> SlotEvidence {
    SlotEvidence {
        owning_cell: owner,
        slot,
        origin: location(&prepared(), None).into(),
        representations: vec![OwnedRepresentation::Text(value.into())],
    }
}
fn store(root: &tempfile::TempDir, prepared: &PreparedExecution) -> PageAssetStore {
    PageAssetStore::new(
        prepared.request().page.clone(),
        root.path().into(),
        root.path().join("staging"),
    )
    .unwrap()
}
#[test]
fn prepared_rejects_source_request_and_context_mismatches() {
    let (request, context) = inputs();
    assert!(
        PreparedExecution::checked(request.clone(), b"different".to_vec(), context.clone())
            .is_err()
    );
    let mut changed = request.clone();
    changed.page.source_fingerprint = fingerprint_bytes(b"wrong");
    assert!(
        PreparedExecution::checked(changed, SOURCE.as_bytes().to_vec(), context.clone()).is_err()
    );
    let mut changed = request.clone();
    changed.cells[0].cell.source.push_str("mutated");
    assert!(PreparedExecution::checked(changed, SOURCE.as_bytes().to_vec(), context).is_err());
    let foreign = AuthoredOutputContext::new(
        request.page.source.clone(),
        "foreign".into(),
        BTreeSet::new(),
    );
    assert!(PreparedExecution::checked(request, SOURCE.as_bytes().to_vec(), foreign).is_err());
    let value = prepared();
    assert_eq!(value.source(), SOURCE.as_bytes());
    assert_eq!(value.context().collection(), "guide");
}
#[test]
fn final_owner_distinguishes_repeated_producers_and_clone_is_untrusted() {
    let prepared = prepared();
    let root = tempfile::tempdir().unwrap();
    let mut store = store(&root, &prepared);
    let mut record = record(&prepared);
    record.cells[0].outputs = vec![text(0, 3, "first")];
    record.cells[1].outputs = vec![text(1, 3, "second")];
    let page = ValidatedPage::checked(
        &prepared,
        record,
        vec![evidence(0, 3, "first"), evidence(1, 3, "second")],
        vec![],
        &mut store,
    )
    .unwrap();
    let mut portable = page.portable_record();
    portable.cells[0].outputs.clear();
    assert!(matches!(
        page.representation(0, 3, 0),
        Some(ValidatedRepresentationRef::Text("first"))
    ));
    assert!(matches!(
        page.representation(1, 3, 0),
        Some(ValidatedRepresentationRef::Text("second"))
    ));
    assert!(page.representation(0, 0, 0).is_none());
    assert_eq!(page.record().cells[0].outputs[0].slot, 3);
}
#[test]
fn missing_extra_wrong_slot_and_content_evidence_are_rejected() {
    let prepared = prepared();
    for evidence in [
        vec![],
        vec![evidence(0, 3, "first"), evidence(1, 3, "extra")],
        vec![evidence(0, 2, "first")],
        vec![evidence(0, 3, "changed")],
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![text(0, 3, "first")];
        assert!(ValidatedPage::checked(&prepared, record, evidence, vec![], &mut store).is_err());
    }
}

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="2" height="3"/></svg>"#;
const OTHER_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="2"/></svg>"#;
fn accepted<T: std::fmt::Debug>(value: Validation<T>) -> T {
    match value {
        Validation::Accepted { value, .. } => value,
        other => panic!("{other:?}"),
    }
}
fn location(prepared: &PreparedExecution, fragment: Option<FragmentIdentity>) -> OutputOrigin {
    OutputOrigin {
        cell: 1,
        slot: 0,
        cell_span: prepared.request().cells[1].cell.span,
        fragment,
    }
}
fn markdown(
    prepared: &PreparedExecution,
    store: &mut PageAssetStore,
    source: &str,
    ordinal: usize,
) -> OwnedRepresentation {
    let origin = location(
        prepared,
        Some(FragmentIdentity {
            ordinal,
            byte_length: source.len(),
        }),
    );
    let parsed = crate::documents::parse_markdown_fragment(
        source,
        crate::documents::MarkdownFragmentOrigin {
            collection: prepared.context().collection().into(),
            cell: origin.cell,
            output: origin.slot,
            source: SourceLocation {
                span: Some(origin.cell_span),
                ..prepared.request().page.source.clone()
            },
        },
    );
    OwnedRepresentation::Markdown {
        value: Box::new(accepted(
            validate_markdown_live(parsed, &origin, prepared.context(), store).unwrap(),
        )),
        origin,
    }
}
fn html(
    prepared: &PreparedExecution,
    store: &mut PageAssetStore,
    source: &str,
) -> OwnedRepresentation {
    let origin = location(prepared, None);
    OwnedRepresentation::Html {
        value: accepted(validate_html_live(source, &origin, prepared.context(), store).unwrap()),
    }
}
fn rich(
    owner: usize,
    slot: usize,
    values: Vec<OwnedRepresentation>,
) -> (ExecutionOutput, SlotEvidence) {
    let mut output = text(owner, slot, "unused");
    output.output.representations.clear();
    output.representations.clear();
    output.offered_mime_types.clear();
    for value in &values {
        let (portable, content, policy) = match value {
            OwnedRepresentation::Text(value) => (
                OutputRepresentation::PlainText {
                    media_type: "text/plain".into(),
                    text: value.clone(),
                },
                RepresentationContent::Text(value),
                None,
            ),
            OwnedRepresentation::Markdown { value, .. } => {
                output.output.provenance.push(value.provenance().clone());
                (
                    OutputRepresentation::MarkdownBlocks {
                        media_type: "text/markdown".into(),
                        blocks: value.blocks().to_vec(),
                    },
                    RepresentationContent::Markdown(value.canonical_content()),
                    Some("qmd-mvp-v1"),
                )
            }
            OwnedRepresentation::Html { value, .. } => (
                OutputRepresentation::HtmlCandidate {
                    media_type: "text/html".into(),
                    html: UnvalidatedHtml::new(value.canonical_content().markup.clone()),
                    policy: "html-mvp-v1".into(),
                    sanitizer: SanitizerProvenance {
                        name: "diplodocus-html-sanitizer".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                    },
                },
                RepresentationContent::Html(value.canonical_content()),
                Some("html-mvp-v1"),
            ),
            OwnedRepresentation::Asset(asset) => (
                OutputRepresentation::Asset {
                    media_type: asset.media_type.clone(),
                    asset: asset.reference.clone(),
                },
                RepresentationContent::Asset(AssetUse::from(asset)),
                Some(if asset.media_type == "image/svg+xml" {
                    "svg-mvp-v1"
                } else {
                    "mime-mvp-v1"
                }),
            ),
        };
        let mime = match &portable {
            OutputRepresentation::PlainText { media_type, .. }
            | OutputRepresentation::MarkdownBlocks { media_type, .. }
            | OutputRepresentation::HtmlCandidate { media_type, .. }
            | OutputRepresentation::Asset { media_type, .. } => media_type.clone(),
        };
        if output.output.representations.is_empty() {
            output.selected_mime_type = Some(mime.clone());
        }
        output.offered_mime_types.insert(mime);
        output.representations.push(RepresentationEvidence {
            producing_cell: 1,
            content_fingerprint: project_representation(content).unwrap().fingerprint,
            policy: policy.map(str::to_owned),
        });
        output.output.representations.push(portable);
    }
    (
        output,
        SlotEvidence {
            owning_cell: owner,
            slot,
            origin: location(&prepared(), None).into(),
            representations: values,
        },
    )
}
#[test]
fn nested_hidden_unselected_and_direct_assets_form_exact_union() {
    let prepared = prepared();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("nested.svg"), SVG).unwrap();
    std::fs::write(root.path().join("hidden.svg"), OTHER_SVG).unwrap();
    let mut store = store(&root, &prepared);
    let direct = store.stage_bytes("image/svg+xml", SVG).unwrap();
    let hidden = store.stage_bytes("image/svg+xml", OTHER_SVG).unwrap();
    let md = markdown(
        &prepared,
        &mut store,
        "- ![a](nested.svg)\n- ![b](nested.svg)\n",
        5,
    );
    let hidden_md = markdown(&prepared, &mut store, "![hidden](hidden.svg)\n", 6);
    let html = html(&prepared, &mut store, "<img src='nested.svg'>");
    let (first, first_evidence) = rich(
        0,
        3,
        vec![OwnedRepresentation::Asset(direct.clone()), md, html],
    );
    let (second, second_evidence) = rich(1, 3, vec![hidden_md]);
    let mut record = record(&prepared);
    record.cells[0].outputs = vec![first];
    record.cells[1].outputs = vec![second];
    record.assets = vec![direct, hidden];
    record.assets.sort_by(|a, b| {
        a.reference
            .fingerprint
            .value
            .cmp(&b.reference.fingerprint.value)
    });
    let page = ValidatedPage::checked(
        &prepared,
        record,
        vec![first_evidence, second_evidence],
        vec![],
        &mut store,
    )
    .unwrap();
    assert_eq!(page.referenced_assets().count(), 2);
    assert!(
        matches!(page.representation(0,3,1), Some(ValidatedRepresentationRef::Markdown(value)) if value.image_bindings().len() == 2)
    );
    assert_eq!(
        page.representation_origin(0, 3, 1)
            .unwrap()
            .fragment
            .unwrap()
            .ordinal,
        5
    );
    assert_eq!(
        page.representation_origin(1, 3, 0)
            .unwrap()
            .fragment
            .unwrap()
            .ordinal,
        6
    );
    let original = page.record().clone();
    let mut portable = page.portable_record();
    portable.assets[0].byte_size = 0;
    portable.cells[0].outputs[0].output.representations.clear();
    assert_eq!(page.record(), &original);
    let result = PageExecutionResult::retain(page, store).unwrap();
    assert_eq!(result.staged_assets().len(), 2);
    for (asset, staged) in result
        .validated()
        .referenced_assets()
        .zip(result.staged_assets())
    {
        assert_eq!(asset.reference, staged.reference);
        assert_eq!(
            fingerprint_bytes(&std::fs::read(&staged.path).unwrap()),
            asset.reference.fingerprint
        );
    }
}
#[test]
fn missing_extra_or_mismatched_assets_never_create_trust() {
    for case in 0..6 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let asset = store.stage_bytes("image/svg+xml", SVG).unwrap();
        let mut direct = asset.clone();
        if case == 2 {
            direct.byte_size += 1;
        }
        if case == 3 {
            direct.media_type = "image/png".into();
        }
        if case == 4 {
            direct.reference.fingerprint.value = "0".repeat(64);
        }
        let (output, evidence) = rich(0, 0, vec![OwnedRepresentation::Asset(direct.clone())]);
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        record.assets = vec![direct];
        if case == 0 {
            record.assets.clear();
        }
        if case == 1 {
            record
                .assets
                .push(store.stage_bytes("image/svg+xml", OTHER_SVG).unwrap());
        }
        if case == 5 {
            record.assets.push(asset);
        }
        assert!(
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn foreign_wrappers_fragment_bounds_and_wrong_portable_content_are_rejected() {
    for case in 0..5 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut value = markdown(&prepared, &mut store, "hello\n", 2);
        if let OwnedRepresentation::Markdown { origin, .. } = &mut value {
            if case == 0 {
                origin.cell = 0;
            }
            if case == 1 {
                origin.fragment.as_mut().unwrap().byte_length = 0;
            }
        }
        if case == 2 {
            let foreign = AuthoredOutputContext::new(
                prepared.request().page.source.clone(),
                "foreign".into(),
                BTreeSet::from(["foreign".into()]),
            );
            let origin = location(&prepared, None);
            value = OwnedRepresentation::Html {
                value: accepted(
                    validate_html_live("<a href='#foreign'>x</a>", &origin, &foreign, &mut store)
                        .unwrap(),
                ),
            };
        }
        let (mut output, evidence) = rich(0, 3, vec![value]);
        if case == 3 {
            output.output.representations[0] = OutputRepresentation::PlainText {
                media_type: "text/plain".into(),
                text: "changed".into(),
            };
        }
        if case == 4 {
            output.representations[0].content_fingerprint = fingerprint_bytes(b"wrong");
        }
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        assert!(
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn typed_diagnostics_are_retained_and_portable_prose_cannot_replace_them() {
    let prepared = prepared();
    let diagnostic = ExecutionDiagnostic::UnknownDisplayUpdate {
        attribution: DiagnosticAttribution::output(
            prepared.context(),
            &location(&prepared, None),
            None,
        ),
    };
    for altered in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut record = record(&prepared);
        record.diagnostics = vec![diagnostic.to_diagnostic("guide")];
        if altered {
            record.diagnostics[0].message.push_str("forged");
        }
        let page = ValidatedPage::checked(
            &prepared,
            record,
            vec![],
            vec![diagnostic.clone()],
            &mut store,
        );
        if altered {
            assert!(page.is_err());
        } else {
            assert_eq!(
                page.unwrap().diagnostics(),
                std::slice::from_ref(&diagnostic)
            );
        }
    }
}
#[test]
fn retention_rechecks_page_staging_bytes_and_does_not_accept_public_parts() {
    for case in 0..3 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let asset = store.stage_bytes("image/svg+xml", SVG).unwrap();
        let (output, evidence) = rich(0, 0, vec![OwnedRepresentation::Asset(asset.clone())]);
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        record.assets = vec![asset];
        let page =
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).unwrap();
        if case == 0 {
            let child = std::fs::read_dir(root.path().join("staging"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let file = std::fs::read_dir(child)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            std::fs::write(file, OTHER_SVG).unwrap();
        } else {
            let mut other = prepared.request().page.clone();
            if case == 1 {
                other.collection = "foreign".into();
            }
            store =
                PageAssetStore::new(other, root.path().into(), root.path().join("other")).unwrap();
        }
        assert!(PageExecutionResult::retain(page, store).is_err());
    }
}
#[tokio::test]
async fn trait_object_returns_a_checked_page_without_mutating_prepared_inputs() {
    struct Engine(std::sync::Mutex<Option<PageExecutionResult>>);
    impl ExecutionEngine for Engine {
        fn capabilities(&self) -> ExecutionCapabilities {
            ExecutionCapabilities {
                languages: BTreeSet::new(),
                media_types: BTreeSet::new(),
                features: BTreeSet::new(),
            }
        }
        fn requirements(&self) -> ExecutionRequirements {
            ExecutionRequirements {
                operating_systems: BTreeSet::new(),
                protocol: KernelProtocolRequirement {
                    name: "jupyter".into(),
                    major: 5,
                },
            }
        }
        fn execute_page<'a>(
            &'a self,
            mut context: ExecutionContext<'a>,
            _request: &'a PageExecutionRequest,
        ) -> ExecutionFuture<'a> {
            Box::pin(async move {
                tokio::select! { biased; ()=&mut context.cancellation => Err(ExecutionFailure {kind:ExecutionFailureKind::Cancelled,diagnostics:vec![],cleanup_diagnostics:vec![]}), result=std::future::ready(self.0.lock().unwrap().take().unwrap()) => Ok(result) }
            })
        }
    }
    let prepared = prepared();
    let original = prepared.request().clone();
    let root = tempfile::tempdir().unwrap();
    let mut store = store(&root, &prepared);
    let mut record = record(&prepared);
    record.cells[0].outputs = vec![text(0, 3, "hello")];
    let page = ValidatedPage::checked(
        &prepared,
        record,
        vec![evidence(0, 3, "hello")],
        vec![],
        &mut store,
    )
    .unwrap();
    let engine: Box<dyn ExecutionEngine> = Box::new(Engine(std::sync::Mutex::new(Some(
        PageExecutionResult::retain(page, store).unwrap(),
    ))));
    let result = engine
        .execute_page(
            ExecutionContext {
                repository_root: root.path().into(),
                page_path: root.path().join("page.qmd"),
                asset_staging_directory: root.path().join("staging"),
                deadlines: ExecutionDeadlines::default(),
                cancellation: Box::pin(std::future::pending()),
            },
            prepared.request(),
        )
        .await
        .unwrap();
    assert!(matches!(
        result.validated().representation(0, 3, 0),
        Some(ValidatedRepresentationRef::Text("hello"))
    ));
    assert_eq!(prepared.request(), &original);
}

#[test]
fn unsupported_display_keeps_specific_rejection_without_generic_warning() {
    let prepared = prepared();
    let root = tempfile::tempdir().unwrap();
    let mut store = store(&root, &prepared);
    let diagnostic = ExecutionDiagnostic::HtmlRejected {
        attribution: DiagnosticAttribution::output(
            prepared.context(),
            &location(&prepared, None),
            None,
        ),
        reason: HtmlRejectionReason::Element,
    };
    let mut record = record(&prepared);
    record.diagnostics = vec![diagnostic.to_diagnostic("guide")];
    let mut output = text(0, 3, "");
    output.output.representations.clear();
    output.representations.clear();
    output.offered_mime_types = BTreeSet::from(["text/html".into()]);
    output.selected_mime_type = None;
    output.diagnostic_indices = vec![0];
    record.cells[0].outputs = vec![output];
    let page = ValidatedPage::checked(
        &prepared,
        record,
        vec![SlotEvidence {
            owning_cell: 0,
            slot: 3,
            origin: location(&prepared, None).into(),
            representations: vec![],
        }],
        vec![diagnostic],
        &mut store,
    )
    .unwrap();
    assert_eq!(page.diagnostics().len(), 1);
    assert_eq!(
        page.canonical_representation(0, 3, 0)
            .unwrap()
            .unwrap()
            .kind,
        "unsupported"
    );
}
#[test]
fn conflicting_fragment_identity_is_rejected_but_copied_updates_are_allowed() {
    for conflict in [false, true] {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let first = markdown(&prepared, &mut store, "first\n", 0);
        let second = if conflict {
            markdown(&prepared, &mut store, "other\n", 0)
        } else {
            first.clone()
        };
        let (a, ea) = rich(0, 3, vec![first]);
        let (b, eb) = rich(1, 3, vec![second]);
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![a];
        record.cells[1].outputs = vec![b];
        let page = ValidatedPage::checked(&prepared, record, vec![ea, eb], vec![], &mut store);
        assert_eq!(page.is_err(), conflict);
    }
}
#[test]
fn staging_proof_checks_empty_pages_poisoning_and_superseded_assets() {
    let prepared = prepared();
    let root = tempfile::tempdir().unwrap();
    let mut store = store(&root, &prepared);
    let old = store.stage_bytes("image/svg+xml", OTHER_SVG).unwrap();
    assert!(store.verify_assets(&prepared.request().page, &[]).is_ok());
    let page =
        ValidatedPage::checked(&prepared, record(&prepared), vec![], vec![], &mut store).unwrap();
    let result = PageExecutionResult::retain(page, store).unwrap();
    assert!(result.staged_assets().is_empty());
    assert!(
        std::fs::read_dir(root.path().join("staging"))
            .unwrap()
            .next()
            .is_none()
    );
    let mut store = PageAssetStore::new(
        prepared.request().page.clone(),
        root.path().into(),
        root.path().join("other"),
    )
    .unwrap();
    let mut foreign = prepared.request().page.clone();
    foreign.collection = "foreign".into();
    assert!(store.verify_assets(&foreign, &[]).is_err());
    assert!(store.verify_assets(&prepared.request().page, &[]).is_err());
    assert!(store.stage_bytes("image/svg+xml", SVG).is_err());
    assert_ne!(old.byte_size, 0);
}

#[test]
fn producing_ledger_offset_excludes_current_build_diagnostics() {
    let prepared = prepared();
    for index in [0, 1, 2] {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let warning = ExecutionDiagnostic::HtmlRejected {
            reason: HtmlRejectionReason::Element,
            attribution: DiagnosticAttribution::output(
                prepared.context(),
                &location(&prepared, None),
                None,
            ),
        };
        let current = crate::diagnostics::Diagnostic::new(
            crate::diagnostics::DiagnosticCode::InvalidExecutionCache,
            crate::diagnostics::Severity::Warning,
            "current lookup warning",
        );
        let diagnostics = DiagnosticEvidence {
            before: vec![current.clone()],
            execution: vec![warning.clone()],
            after: vec![current.clone()],
        };
        let mut record = record(&prepared);
        record.diagnostics = vec![current.clone(), warning.to_diagnostic("guide"), current];
        let mut output = text(0, 3, "value");
        output.offered_mime_types.insert("text/html".into());
        output.diagnostic_indices = vec![index];
        record.cells[0].outputs = vec![output];
        let page = ValidatedPage::checked(
            &prepared,
            record,
            vec![evidence(0, 3, "value")],
            diagnostics,
            &mut store,
        );
        if index == 1 {
            let page = page.unwrap();
            assert_eq!(page.execution_diagnostic_offset(), 1);
            assert_eq!(page.diagnostics(), &[warning]);
            assert_eq!(page.record().diagnostics.len(), 3);
        } else {
            assert!(page.is_err());
        }
    }
}

#[test]
fn checked_construction_preserves_active_asset_failure_kind() {
    let prepared = prepared();
    let root = tempfile::tempdir().unwrap();
    let mut store = store(&root, &prepared);
    let asset = store.stage_bytes("image/svg+xml", SVG).unwrap();
    let (output, evidence) = rich(0, 0, vec![OwnedRepresentation::Asset(asset.clone())]);
    let directory = std::fs::read_dir(root.path().join("staging"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(
        directory.join(&asset.reference.fingerprint.value),
        OTHER_SVG,
    )
    .unwrap();
    let mut record = record(&prepared);
    record.assets = vec![asset];
    record.cells[0].outputs = vec![output];
    assert_eq!(
        ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).unwrap_err(),
        RecordValidationError::Asset(crate::execution::assets::AssetError::Collision)
    );
    assert!(store.verify_assets(&prepared.request().page, &[]).is_err());
    store.rollback().unwrap();
}

#[test]
fn fragment_diagnostic_slot_is_nullable_but_cell_and_source_rules_hold() {
    let prepared = prepared();
    for case in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut attribution = DiagnosticAttribution {
            source: None,
            cell: Some(1),
            slot: None,
            fragment: Some(FragmentIdentity {
                ordinal: 8,
                byte_length: 4,
            }),
            span: Some(SourceSpan { start: 0, end: 4 }),
            related_spans: vec![],
        };
        if case == 1 {
            attribution.cell = None;
        }
        if case == 2 {
            attribution.source = Some(crate::diagnostics::DiagnosticSource::Repository {
                repository: prepared.request().page.source.repository.clone(),
                path: prepared.request().page.source.path.clone(),
            });
        }
        let diagnostic = ExecutionDiagnostic::FragmentUnsupported {
            attribution,
            source_kind: "raw-html".into(),
        };
        let mut record = record(&prepared);
        record.diagnostics = vec![diagnostic.to_diagnostic("guide")];
        let result =
            ValidatedPage::checked(&prepared, record, vec![], vec![diagnostic], &mut store);
        assert_eq!(result.is_ok(), case == 0, "case {case}");
    }
}

#[test]
fn unsupported_output_rejects_unrelated_producer_and_session_warnings() {
    let prepared = prepared();
    for case in 0..2 {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut attribution =
            DiagnosticAttribution::output(prepared.context(), &location(&prepared, None), None);
        let warning = if case == 0 {
            attribution.cell = Some(0);
            attribution.span = Some(prepared.request().cells[0].cell.span);
            ExecutionDiagnostic::HtmlRejected {
                attribution,
                reason: HtmlRejectionReason::Element,
            }
        } else {
            attribution.cell = None;
            attribution.slot = None;
            attribution.span = None;
            ExecutionDiagnostic::KernelMessageIgnored { attribution }
        };
        let mut record = record(&prepared);
        record.diagnostics = vec![warning.to_diagnostic("guide")];
        let mut output = text(0, 3, "");
        output.output.representations.clear();
        output.representations.clear();
        output.offered_mime_types = BTreeSet::from(["text/html".into()]);
        output.selected_mime_type = None;
        output.diagnostic_indices = vec![0];
        record.cells[0].outputs = vec![output];
        assert!(
            ValidatedPage::checked(
                &prepared,
                record,
                vec![SlotEvidence {
                    owning_cell: 0,
                    slot: 3,
                    origin: location(&prepared, None).into(),
                    representations: vec![]
                }],
                vec![warning],
                &mut store
            )
            .is_err(),
            "case {case}"
        );
    }
}
#[test]
fn fragment_warnings_and_wrappers_share_consistent_identity() {
    let prepared = prepared();
    for case in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let value = markdown(&prepared, &mut store, "value\n", 4);
        let (output, evidence) = rich(0, 3, vec![value]);
        let mut attribution = DiagnosticAttribution::output(
            prepared.context(),
            &location(
                &prepared,
                Some(FragmentIdentity {
                    ordinal: 4,
                    byte_length: 6,
                }),
            ),
            Some(SourceSpan { start: 0, end: 5 }),
        );
        if case == 0 {
            attribution.fragment.as_mut().unwrap().byte_length = 10;
        }
        if case == 1 {
            attribution.slot = Some(8);
        }
        if case == 2 {
            attribution.slot = None;
        }
        if case >= 3 {
            attribution.fragment.as_mut().unwrap().ordinal = 9;
        }
        let warning = ExecutionDiagnostic::FragmentUnsupported {
            attribution: attribution.clone(),
            source_kind: "raw-html".into(),
        };
        let mut diagnostics = vec![warning];
        if case >= 3 {
            attribution.fragment.as_mut().unwrap().byte_length = 10;
            diagnostics.push(ExecutionDiagnostic::FragmentUnsupported {
                attribution,
                source_kind: "raw-html".into(),
            });
        }
        if case == 4 {
            diagnostics.pop();
        }
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        record.diagnostics = diagnostics
            .iter()
            .map(|d| d.to_diagnostic("guide"))
            .collect();
        let page =
            ValidatedPage::checked(&prepared, record, vec![evidence], diagnostics, &mut store);
        assert_eq!(page.is_ok(), case == 2 || case == 4, "case {case}");
    }
}

#[test]
fn display_mime_order_uniqueness_and_ascii_names_are_checked() {
    for case in 0..4 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let md = markdown(&prepared, &mut store, "value\n", 0);
        let h = html(&prepared, &mut store, "<b>value</b>");
        let values = match case {
            0 => vec![h, md],
            1 => vec![h.clone(), h],
            _ => vec![md, h],
        };
        let (mut output, evidence) = rich(0, 3, values);
        if case == 2 {
            output.offered_mime_types.insert("application/é".into());
        }
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        assert_eq!(
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).is_ok(),
            case == 3,
            "case {case}"
        );
    }
}
#[test]
fn generated_markdown_provenance_is_an_exact_projection() {
    for case in 0..5 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let md = markdown(&prepared, &mut store, "value\n", 0);
        let (mut output, evidence) = rich(0, 3, vec![md]);
        if case == 0 {
            output.output.provenance.clear();
        }
        if case == 1 {
            output
                .output
                .provenance
                .push(output.output.provenance[0].clone());
        }
        if case == 2 {
            let mut foreign = output.output.provenance[0].clone();
            foreign.activity = ProvenanceActivity::GeneratedMarkdown {
                collection: "foreign".into(),
                cell: 0,
                output: 99,
            };
            output.output.provenance.push(foreign);
        }
        if case == 3 {
            let mut extra = output.output.provenance[0].clone();
            extra.activity = ProvenanceActivity::Declaration;
            output.output.provenance.push(extra);
        }
        let mut record = record(&prepared);
        record.cells[0].outputs = vec![output];
        assert_eq!(
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).is_ok(),
            case == 4,
            "case {case}"
        );
    }
}
#[test]
fn stderr_and_ordinary_stdout_require_literal_unupdated_streams() {
    for case in 0..5 {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let values = if case < 2 {
            vec![markdown(&prepared, &mut store, "value\n", 0)]
        } else {
            vec![OwnedRepresentation::Text("literal".into())]
        };
        let (mut output, evidence) = rich(1, 3, values);
        output.producing_cell = 1;
        output.output.kind = CellOutputKind::Stream {
            stream: if case == 1 {
                StreamName::Stdout
            } else {
                StreamName::Stderr
            },
        };
        if case != 2 {
            output.updating_cell = None;
        }
        if case == 3 {
            output.output.provenance.push(Provenance {
                activity: ProvenanceActivity::Declaration,
                source: None,
                span: None,
                tools: Default::default(),
            });
        }
        let mut record = record(&prepared);
        record.cells[1].outputs = vec![output];
        assert_eq!(
            ValidatedPage::checked(&prepared, record, vec![evidence], vec![], &mut store).is_ok(),
            case == 4,
            "case {case}"
        );
    }
}

#[test]
fn restored_non_markdown_outputs_preserve_unknown_producer_slots() {
    for kind in 0..4 {
        let mut previous = None;
        for known in [Some(0), None] {
            let prepared = prepared();
            let root = tempfile::tempdir().unwrap();
            let mut store = store(&root, &prepared);
            let values = match kind {
                0 => vec![html(&prepared, &mut store, "<b>value</b>")],
                1 => vec![OwnedRepresentation::Text("value".into())],
                2 => vec![OwnedRepresentation::Asset(
                    store.stage_bytes("image/svg+xml", SVG).unwrap(),
                )],
                _ => vec![],
            };
            let (mut output, mut evidence) = rich(0, 3, values);
            evidence.origin.slot = known;
            let mut record = record(&prepared);
            for value in &evidence.representations {
                if let OwnedRepresentation::Asset(asset) = value {
                    record.assets.push(asset.clone());
                }
            }
            let diagnostics = if kind == 3 {
                let mut attribution = DiagnosticAttribution::output(
                    prepared.context(),
                    &location(&prepared, None),
                    None,
                );
                attribution.slot = None;
                output.selected_mime_type = None;
                output.offered_mime_types = BTreeSet::from(["text/html".into()]);
                output.diagnostic_indices = vec![0];
                vec![ExecutionDiagnostic::HtmlRejected {
                    attribution,
                    reason: HtmlRejectionReason::Element,
                }]
            } else {
                vec![]
            };
            record.diagnostics = diagnostics
                .iter()
                .map(|d| d.to_diagnostic("guide"))
                .collect();
            record.cells[0].outputs = vec![output];
            let page =
                ValidatedPage::checked(&prepared, record, vec![evidence], diagnostics, &mut store)
                    .unwrap();
            assert_eq!(page.output_origin(0, 3).unwrap().slot, known);
            assert!(page.representation_origin(0, 3, 0).is_none());
            let portable = (
                page.portable_record(),
                page.canonical_representation(0, 3, 0).unwrap(),
            );
            if let Some(previous) = previous {
                assert_eq!(previous, portable);
            }
            previous = Some(portable);
        }
    }
}
#[test]
fn copied_updates_share_warning_indices_and_known_slot_facts_must_agree() {
    for conflict in [false, true] {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let mut attribution =
            DiagnosticAttribution::output(prepared.context(), &location(&prepared, None), None);
        if conflict {
            attribution.slot = Some(8);
        }
        let warning = ExecutionDiagnostic::HtmlRejected {
            attribution,
            reason: HtmlRejectionReason::Element,
        };
        let mut record = record(&prepared);
        record.diagnostics = vec![warning.to_diagnostic("guide")];
        let mut all_evidence = vec![];
        for (owner, slot) in [(0, 3), (1, 7)] {
            let (mut output, evidence) = rich(
                owner,
                slot,
                vec![OwnedRepresentation::Text("fallback".into())],
            );
            output.offered_mime_types.insert("text/html".into());
            output.diagnostic_indices = vec![0];
            record.cells[owner].outputs = vec![output];
            all_evidence.push(evidence);
        }
        let page =
            ValidatedPage::checked(&prepared, record, all_evidence, vec![warning], &mut store);
        if conflict {
            assert!(page.is_err());
        } else {
            let page = page.unwrap();
            assert_eq!(page.diagnostics().len(), 1);
            assert_eq!(page.record().cells[0].outputs[0].diagnostic_indices, [0]);
            assert_eq!(page.record().cells[1].outputs[0].diagnostic_indices, [0]);
        }
    }
}
#[test]
fn asis_stdout_retains_markdown_or_a_literal_rejection_fallback() {
    let source = SOURCE.replace("#| output: false", "#| output: asis");
    let (request, context) = inputs_from(&source);
    let prepared = PreparedExecution::checked(request, source.into_bytes(), context).unwrap();
    for fallback in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let value = if fallback {
            OwnedRepresentation::Text("unsafe fragment".into())
        } else {
            markdown(&prepared, &mut store, "value\n", 0)
        };
        let (mut output, mut evidence) = rich(1, 3, vec![value]);
        evidence.origin = location(&prepared, None).into();
        output.output.kind = CellOutputKind::Stream {
            stream: StreamName::Stdout,
        };
        output.producing_cell = 1;
        output.updating_cell = None;
        let diagnostics = if fallback {
            output.offered_mime_types.insert("text/markdown".into());
            output.diagnostic_indices = vec![0];
            vec![ExecutionDiagnostic::MarkdownRejected {
                attribution: DiagnosticAttribution::output(
                    prepared.context(),
                    &location(
                        &prepared,
                        Some(FragmentIdentity {
                            ordinal: 0,
                            byte_length: 15,
                        }),
                    ),
                    None,
                ),
                reason: MarkdownRejectionReason::Url,
            }]
        } else {
            vec![]
        };
        let mut record = record(&prepared);
        record.cells[1].outputs = vec![output];
        record.diagnostics = diagnostics
            .iter()
            .map(|d| d.to_diagnostic("guide"))
            .collect();
        assert!(
            ValidatedPage::checked(&prepared, record, vec![evidence], diagnostics, &mut store)
                .is_ok()
        );
    }
}
#[test]
fn fragment_warning_unknown_slots_merge_but_known_slots_cannot_conflict() {
    for unknown in [false, true] {
        let prepared = prepared();
        let root = tempfile::tempdir().unwrap();
        let mut store = store(&root, &prepared);
        let base = DiagnosticAttribution {
            source: None,
            cell: Some(1),
            slot: if unknown { None } else { Some(0) },
            fragment: Some(FragmentIdentity {
                ordinal: 9,
                byte_length: 4,
            }),
            span: None,
            related_spans: vec![],
        };
        let mut other = base.clone();
        other.slot = Some(2);
        let diagnostics = vec![
            ExecutionDiagnostic::FragmentUnsupported {
                attribution: base,
                source_kind: "raw-html".into(),
            },
            ExecutionDiagnostic::FragmentUnsupported {
                attribution: other,
                source_kind: "raw-html".into(),
            },
        ];
        let mut record = record(&prepared);
        record.diagnostics = diagnostics
            .iter()
            .map(|d| d.to_diagnostic("guide"))
            .collect();
        assert_eq!(
            ValidatedPage::checked(&prepared, record, vec![], diagnostics, &mut store).is_ok(),
            unknown
        );
    }
}

#[test]
fn nullable_fragment_warning_connects_wrapper_and_output_slot_facts() {
    for wrapper_owner in 0..2 {
        for conflict in [false, true] {
            let prepared = prepared();
            let root = tempfile::tempdir().unwrap();
            let mut store = store(&root, &prepared);
            let value = markdown(&prepared, &mut store, "value\n", 4);
            let (md, md_evidence) = rich(wrapper_owner, 3, vec![value]);
            let owner = 1 - wrapper_owner;
            let (mut fallback, mut fallback_evidence) =
                rich(owner, 3, vec![OwnedRepresentation::Text("fallback".into())]);
            fallback_evidence.origin.slot = Some(if conflict { 7 } else { 0 });
            fallback.offered_mime_types.insert("text/markdown".into());
            fallback.diagnostic_indices = vec![0];
            let mut attribution = DiagnosticAttribution::output(
                prepared.context(),
                &location(
                    &prepared,
                    Some(FragmentIdentity {
                        ordinal: 4,
                        byte_length: 6,
                    }),
                ),
                None,
            );
            attribution.slot = None;
            let warning = ExecutionDiagnostic::MarkdownRejected {
                attribution,
                reason: MarkdownRejectionReason::Url,
            };
            let mut record = record(&prepared);
            record.cells[wrapper_owner].outputs = vec![md];
            record.cells[owner].outputs = vec![fallback];
            record.diagnostics = vec![warning.to_diagnostic("guide")];
            let result = ValidatedPage::checked(
                &prepared,
                record,
                vec![md_evidence, fallback_evidence],
                vec![warning],
                &mut store,
            );
            assert_eq!(
                result.is_ok(),
                !conflict,
                "owner {wrapper_owner}, conflict {conflict}"
            );
        }
    }
}
#[test]
fn slot_facts_propagate_through_warning_only_fragment_connections() {
    for reverse in [false, true] {
        for conflict in [false, true] {
            let prepared = prepared();
            let root = tempfile::tempdir().unwrap();
            let mut store = store(&root, &prepared);
            let diagnostics: Vec<_> = (4..6)
                .map(|ordinal| {
                    let mut attribution = DiagnosticAttribution::output(
                        prepared.context(),
                        &location(
                            &prepared,
                            Some(FragmentIdentity {
                                ordinal,
                                byte_length: 6,
                            }),
                        ),
                        None,
                    );
                    attribution.slot = None;
                    ExecutionDiagnostic::MarkdownRejected {
                        attribution,
                        reason: MarkdownRejectionReason::Url,
                    }
                })
                .collect();
            let mut record = record(&prepared);
            record.diagnostics = diagnostics
                .iter()
                .map(|d| d.to_diagnostic("guide"))
                .collect();
            let mut evidence = vec![];
            for index in 0..3 {
                let (mut output, mut proof) =
                    rich(0, index, vec![OwnedRepresentation::Text("fallback".into())]);
                output.offered_mime_types.insert("text/markdown".into());
                let node = if reverse { 2 - index } else { index };
                output.diagnostic_indices = match node {
                    0 => vec![0],
                    1 => vec![0, 1],
                    _ => vec![1],
                };
                proof.origin.slot = match node {
                    0 => Some(0),
                    1 => None,
                    _ => Some(if conflict { 7 } else { 0 }),
                };
                record.cells[0].outputs.push(output);
                evidence.push(proof);
            }
            let result =
                ValidatedPage::checked(&prepared, record, evidence, diagnostics, &mut store);
            assert_eq!(
                result.is_ok(),
                !conflict,
                "reverse {reverse}, conflict {conflict}"
            );
            if let Ok(page) = result {
                for slot in 0..3 {
                    assert_eq!(page.output_origin(0, slot).unwrap().slot, Some(0));
                }
            }
        }
    }
}
