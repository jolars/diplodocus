use super::*;
use crate::diagnostics::DiagnosticSource;
use crate::ir::SourceSpan;

fn require(condition: bool) -> Result<(), RecordValidationError> {
    condition
        .then_some(())
        .ok_or(RecordValidationError::Association)
}
impl ValidatedPage {
    /// Finalize output evidence while its page-scoped staging owner is still live.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "The production engine is supported only on Linux."
        )
    )]
    pub(crate) fn checked(
        prepared: &PreparedExecution,
        record: PageExecutionRecord,
        evidence: Vec<SlotEvidence>,
        diagnostics: impl Into<DiagnosticEvidence>,
        store: &mut PageAssetStore,
    ) -> Result<Self, RecordValidationError> {
        let DiagnosticEvidence {
            before,
            execution: diagnostics,
            after,
        } = diagnostics.into();
        let diagnostic_offset = before.len();
        let request = prepared.request();
        require(
            record.page == request.page
                && record.defaults == request.defaults
                && record.cells.len() == request.cells.len(),
        )?;
        check_diagnostics(prepared, &diagnostics)?;
        require(
            record.diagnostics
                == before
                    .into_iter()
                    .chain(
                        diagnostics
                            .iter()
                            .map(|d| d.to_diagnostic(&record.page.collection)),
                    )
                    .chain(after)
                    .collect::<Vec<_>>(),
        )?;
        let mut slots = BTreeMap::new();
        for slot in evidence {
            require(slots.insert((slot.owning_cell, slot.slot), slot).is_none())?;
        }
        let mut assets = BTreeMap::new();
        let mut fragments = BTreeMap::new();
        let mut fragment_claims = BTreeMap::new();
        for slot in slots.values() {
            for value in &slot.representations {
                match value {
                    OwnedRepresentation::Text(_) => {}
                    OwnedRepresentation::Markdown { value, origin } => {
                        let fragment = origin.fragment.ok_or(RecordValidationError::Association)?;
                        record_fragment(
                            &mut fragment_claims,
                            origin.cell,
                            Some(origin.slot),
                            fragment,
                        )?;
                        if let Some((old_origin, old_value)) =
                            fragments.insert((origin.cell, fragment.ordinal), (origin, value))
                        {
                            require(old_origin == origin && old_value == value)?;
                        }
                        for asset in value.referenced_assets() {
                            insert_asset(&mut assets, asset)?;
                        }
                    }
                    OwnedRepresentation::Html { value, .. } => {
                        for asset in value.referenced_assets() {
                            insert_asset(&mut assets, asset)?;
                        }
                    }
                    OwnedRepresentation::Asset(asset) => insert_asset(&mut assets, asset)?,
                }
            }
        }
        for diagnostic in &diagnostics {
            let attribution = diagnostic.attribution();
            if let Some(fragment) = attribution.fragment {
                record_fragment(
                    &mut fragment_claims,
                    attribution.cell.ok_or(RecordValidationError::Association)?,
                    attribution.slot,
                    fragment,
                )?;
            }
        }
        require(record.assets == assets.into_values().collect::<Vec<_>>())?;
        let verified = VerifiedAssets::from_store(store, &record.page, &record.assets)?;
        let mut slot_facts: BTreeMap<_, _> = fragment_claims
            .iter()
            .map(|(key, (_, slot))| (SlotFact::Fragment(key.0, key.1), *slot))
            .collect();
        let mut slot_edges = BTreeMap::new();
        let mut output_count = 0;
        for (cell, input) in record.cells.iter().zip(&request.cells) {
            require(
                cell.ordinal == input.ordinal
                    && cell.language == input.cell.language
                    && cell.span == input.cell.span
                    && cell.source_segments == input.cell.source_segments
                    && cell.options == input.options,
            )?;
            if matches!(cell.outcome, CellOutcome::Skipped { .. }) {
                require(cell.outputs.is_empty() && cell.submitted_source_fingerprint.is_none())?;
            } else {
                require(
                    cell.submitted_source_fingerprint
                        == Some(fingerprint_bytes(input.cell.source.as_bytes())),
                )?;
                require(
                    cell.outcome != CellOutcome::AllowedError || cell.options.execution.error.value,
                )?;
            }
            require(cell.outputs.windows(2).all(|w| w[0].slot < w[1].slot))?;
            for output in &cell.outputs {
                output_count += 1;
                require(
                    output.owning_cell == cell.ordinal
                        && request.cells.get(output.producing_cell).is_some()
                        && output
                            .updating_cell
                            .is_none_or(|c| request.cells.get(c).is_some()),
                )?;
                let slot = slots
                    .get_mut(&(cell.ordinal, output.slot))
                    .ok_or(RecordValidationError::Association)?;
                let current_cell = request
                    .cells
                    .get(slot.origin.cell)
                    .ok_or(RecordValidationError::Association)?;
                require(
                    slot.origin.cell == output.updating_cell.unwrap_or(output.producing_cell)
                        && slot.origin.cell_span == current_cell.cell.span,
                )?;
                let output_fact = SlotFact::Output(cell.ordinal, output.slot);
                let values = &slot.representations;
                require(
                    values.len() == output.output.representations.len()
                        && values.len() == output.representations.len(),
                )?;
                require(
                    output.diagnostic_indices.windows(2).all(|w| w[0] < w[1])
                        && output.diagnostic_indices.iter().all(|i| {
                            *i >= diagnostic_offset && *i - diagnostic_offset < diagnostics.len()
                        }),
                )?;
                let generated: Vec<_> = values
                    .iter()
                    .filter_map(|value| match value {
                        OwnedRepresentation::Markdown { value, .. } => {
                            Some(value.provenance().clone())
                        }
                        _ => None,
                    })
                    .collect();
                require(output.output.provenance == generated)?;
                check_output_shape(output, &cell.options)?;
                let mut known_slot = slot.origin.slot;
                for value in values {
                    if let OwnedRepresentation::Markdown { origin, .. } = value {
                        merge_slot(&mut known_slot, Some(origin.slot))?;
                        let fragment = origin.fragment.ok_or(RecordValidationError::Association)?;
                        connect_slots(
                            &mut slot_edges,
                            output_fact,
                            SlotFact::Fragment(origin.cell, fragment.ordinal),
                        );
                    }
                }
                let mut rejection = false;
                for index in &output.diagnostic_indices {
                    let diagnostic = &diagnostics[*index - diagnostic_offset];
                    let attribution = diagnostic.attribution();
                    require(attribution.cell == Some(slot.origin.cell))?;
                    merge_slot(&mut known_slot, attribution.slot)?;
                    if let Some(fragment) = attribution.fragment {
                        connect_slots(
                            &mut slot_edges,
                            output_fact,
                            SlotFact::Fragment(slot.origin.cell, fragment.ordinal),
                        );
                    }
                    let supports_rejection = check_output_diagnostic(output, diagnostic)?;
                    rejection |= supports_rejection;
                }
                if output.unsupported_placeholder().is_some() {
                    require(rejection)?;
                }
                slot.origin.slot = known_slot;
                slot_facts.insert(output_fact, known_slot);
                for ((value, portable), evidence) in values
                    .iter()
                    .zip(&output.output.representations)
                    .zip(&output.representations)
                {
                    require(
                        evidence.producing_cell
                            == output.updating_cell.unwrap_or(output.producing_cell),
                    )?;
                    if let OwnedRepresentation::Markdown { origin, .. } = value {
                        require(
                            origin.cell == slot.origin.cell
                                && Some(origin.slot) == slot.origin.slot
                                && origin.cell_span == slot.origin.cell_span,
                        )?;
                    }
                    let content =
                        check_representation(prepared, value, portable, evidence, &verified)?;
                    require(
                        project_representation(content)?.fingerprint
                            == evidence.content_fingerprint,
                    )?;
                }
            }
        }
        require(output_count == slots.len())?;
        resolve_slot_facts(&mut slots, &slot_facts, &slot_edges)?;
        Ok(Self {
            record,
            slots,
            diagnostics,
            diagnostic_offset,
        })
    }
}
fn insert_asset(
    assets: &mut BTreeMap<String, ExecutionAsset>,
    asset: &ExecutionAsset,
) -> Result<(), RecordValidationError> {
    if let Some(previous) = assets.insert(asset.reference.fingerprint.value.clone(), asset.clone())
    {
        require(previous == *asset)?;
    }
    Ok(())
}
fn origin(
    prepared: &PreparedExecution,
    evidence: &RepresentationEvidence,
    value: &OutputOrigin,
    markdown: bool,
) -> Result<(), RecordValidationError> {
    let cell = prepared
        .request()
        .cells
        .get(value.cell)
        .ok_or(RecordValidationError::Association)?;
    require(
        value.cell == evidence.producing_cell
            && value.cell_span == cell.cell.span
            && value.fragment.is_some() == markdown,
    )
}
fn check_representation<'a>(
    prepared: &PreparedExecution,
    value: &'a OwnedRepresentation,
    portable: &OutputRepresentation,
    evidence: &RepresentationEvidence,
    verified: &VerifiedAssets,
) -> Result<RepresentationContent<'a>, RecordValidationError> {
    match (value, portable) {
        (
            OwnedRepresentation::Text(text),
            OutputRepresentation::PlainText {
                media_type,
                text: other,
            },
        ) => {
            require(media_type == "text/plain" && text == other && evidence.policy.is_none())?;
            Ok(RepresentationContent::Text(text))
        }
        (
            OwnedRepresentation::Asset(asset),
            OutputRepresentation::Asset {
                media_type,
                asset: reference,
            },
        ) => {
            require(
                media_type == &asset.media_type
                    && reference == &asset.reference
                    && evidence.policy.as_deref()
                        == Some(if media_type == "image/svg+xml" {
                            "svg-mvp-v1"
                        } else {
                            "mime-mvp-v1"
                        }),
            )?;
            Ok(RepresentationContent::Asset(AssetUse::from(asset)))
        }
        (
            OwnedRepresentation::Html { value },
            OutputRepresentation::HtmlCandidate {
                media_type,
                html,
                policy,
                sanitizer,
            },
        ) => {
            require(
                media_type == "text/html"
                    && html.as_untrusted_str() == value.canonical_content().markup
                    && policy == "html-mvp-v1"
                    && evidence.policy.as_deref() == Some("html-mvp-v1")
                    && sanitizer.name == "diplodocus-html-sanitizer"
                    && sanitizer.version == env!("CARGO_PKG_VERSION"),
            )?;
            let checked = restore_html_content(
                value.canonical_content().clone(),
                prepared.context(),
                verified,
            )
            .map_err(|_| RecordValidationError::Association)?;
            require(checked == *value)?;
            Ok(RepresentationContent::Html(value.canonical_content()))
        }
        (
            OwnedRepresentation::Markdown {
                value,
                origin: location,
            },
            OutputRepresentation::MarkdownBlocks { media_type, blocks },
        ) => {
            origin(prepared, evidence, location, true)?;
            require(
                media_type == "text/markdown"
                    && blocks == value.blocks()
                    && evidence.policy.as_deref() == Some("qmd-mvp-v1"),
            )?;
            let checked = restore_markdown(
                value.canonical_content().clone(),
                location,
                prepared.context(),
                verified,
            )
            .map_err(|_| RecordValidationError::Association)?;
            require(checked == **value)?;
            Ok(RepresentationContent::Markdown(value.canonical_content()))
        }
        _ => Err(RecordValidationError::Association),
    }
}
fn media_type(representation: &OutputRepresentation) -> &str {
    match representation {
        OutputRepresentation::PlainText { media_type, .. }
        | OutputRepresentation::MarkdownBlocks { media_type, .. }
        | OutputRepresentation::Asset { media_type, .. }
        | OutputRepresentation::HtmlCandidate { media_type, .. } => media_type,
    }
}
fn check_output_shape(
    output: &ExecutionOutput,
    options: &EffectiveCellOptions,
) -> Result<(), RecordValidationError> {
    require(output.offered_mime_types.iter().all(|mime| mime.is_ascii()))?;
    let representations = &output.output.representations;
    match &output.output.kind {
        CellOutputKind::Error { .. } => {
            return require(
                representations.is_empty()
                    && output.representations.is_empty()
                    && output.selected_mime_type.is_none()
                    && output.offered_mime_types.is_empty()
                    && output.updating_cell.is_none()
                    && output.producing_cell == output.owning_cell,
            );
        }
        CellOutputKind::Display if representations.is_empty() => {
            return require(output.unsupported_placeholder().is_some());
        }
        CellOutputKind::Display => {
            let ranks: Vec<_> = representations
                .iter()
                .map(|r| match media_type(r) {
                    "image/svg+xml" => Ok(0),
                    "image/png" => Ok(1),
                    "image/jpeg" => Ok(2),
                    "text/markdown" => Ok(3),
                    "text/html" => Ok(4),
                    "text/plain" => Ok(5),
                    _ => Err(RecordValidationError::Association),
                })
                .collect::<Result<_, _>>()?;
            require(ranks.windows(2).all(|r| r[0] < r[1]))?;
        }
        CellOutputKind::Stream { stream } => {
            require(
                output.updating_cell.is_none()
                    && output.producing_cell == output.owning_cell
                    && representations.len() == 1,
            )?;
            let literal = matches!(representations[0], OutputRepresentation::PlainText { .. });
            let asis = *stream == crate::ir::StreamName::Stdout
                && options.execution.output.value == OutputVisibility::AsIs;
            require(
                literal
                    || (asis
                        && matches!(
                            representations[0],
                            OutputRepresentation::MarkdownBlocks { .. }
                        )),
            )?;
        }
    }
    let first = representations
        .first()
        .ok_or(RecordValidationError::Association)?;
    require(
        output.selected_mime_type.as_deref() == Some(media_type(first))
            && representations
                .iter()
                .all(|r| output.offered_mime_types.contains(media_type(r))),
    )
}
fn check_diagnostics(
    prepared: &PreparedExecution,
    diagnostics: &[ExecutionDiagnostic],
) -> Result<(), RecordValidationError> {
    let expected_source = DiagnosticSource::Repository {
        repository: prepared.request().page.source.repository.clone(),
        path: prepared.request().page.source.path.clone(),
    };
    for diagnostic in diagnostics {
        let a = diagnostic.attribution();
        require(
            a.cell
                .is_none_or(|cell| prepared.request().cells.get(cell).is_some())
                && (a.slot.is_none() || a.cell.is_some()),
        )?;
        let length = match a.fragment {
            Some(fragment) => {
                require(a.source.is_none() && a.cell.is_some())?;
                fragment.byte_length
            }
            None => {
                require(a.source.as_ref() == Some(&expected_source))?;
                prepared.source().len()
            }
        };
        let span_valid = |s: &SourceSpan| s.start <= s.end && s.end <= length;
        require(a.span.as_ref().is_none_or(span_valid) && a.related_spans.iter().all(span_valid))?;
        if let ExecutionDiagnostic::NoSupportedRepresentation { mime_types, .. } = diagnostic {
            require(
                mime_types.windows(2).all(|w| w[0] < w[1])
                    && mime_types.iter().all(|s| s.is_ascii()),
            )?;
        }
    }
    Ok(())
}

// A fragment may survive in several owning slots or only in the warning ledger.
// Missing slot attribution is unknown; known claims must still agree.
fn record_fragment(
    claims: &mut BTreeMap<(usize, usize), (usize, Option<usize>)>,
    cell: usize,
    slot: Option<usize>,
    fragment: FragmentIdentity,
) -> Result<(), RecordValidationError> {
    let claim = claims
        .entry((cell, fragment.ordinal))
        .or_insert((fragment.byte_length, slot));
    require(
        claim.0 == fragment.byte_length && (claim.1.is_none() || slot.is_none() || claim.1 == slot),
    )?;
    claim.1 = claim.1.or(slot);
    Ok(())
}

fn check_output_diagnostic(
    output: &ExecutionOutput,
    diagnostic: &ExecutionDiagnostic,
) -> Result<bool, RecordValidationError> {
    use ExecutionDiagnostic::*;
    let offered = &output.offered_mime_types;
    let (belongs, rejection) = match diagnostic {
        KernelMessageIgnored { .. } | UnknownDisplayUpdate { .. } => (false, false),
        NoSupportedRepresentation { mime_types, .. } => {
            (mime_types.iter().eq(offered.iter()), true)
        }
        UnsupportedMedia { media_type, .. } | InvalidTextPayload { media_type, .. } => {
            (offered.contains(media_type), true)
        }
        InvalidImage {
            attribution,
            media_type,
        } => (
            offered.contains(media_type)
                || (media_type == "image/*"
                    && if attribution.fragment.is_some() {
                        offered.contains("text/markdown")
                    } else {
                        offered.contains("text/html")
                    }),
            true,
        ),
        InvalidMimeBundle { .. } => (true, true),
        SvgRejected { attribution } => (
            offered.contains("image/svg+xml")
                || if attribution.fragment.is_some() {
                    offered.contains("text/markdown")
                } else {
                    offered.contains("text/html")
                },
            true,
        ),
        HtmlRejected { .. } => (offered.contains("text/html"), true),
        MarkdownRejected { .. } => (offered.contains("text/markdown"), true),
        FragmentUnsupported { .. } => (offered.contains("text/markdown"), false),
    };
    require(belongs)?;
    Ok(rejection)
}

fn merge_slot(
    known: &mut Option<usize>,
    other: Option<usize>,
) -> Result<(), RecordValidationError> {
    require(known.is_none() || other.is_none() || *known == other)?;
    *known = known.or(other);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SlotFact {
    Output(usize, usize),
    Fragment(usize, usize),
}
fn connect_slots(edges: &mut BTreeMap<SlotFact, Vec<SlotFact>>, left: SlotFact, right: SlotFact) {
    edges.entry(left).or_default().push(right);
    edges.entry(right).or_default().push(left);
}

// Nullable warnings connect fragment identity to current slot evidence. Resolve
// entire components so a fact learned from a later output cannot evade a check.
fn resolve_slot_facts(
    slots: &mut BTreeMap<(usize, usize), SlotEvidence>,
    facts: &BTreeMap<SlotFact, Option<usize>>,
    edges: &BTreeMap<SlotFact, Vec<SlotFact>>,
) -> Result<(), RecordValidationError> {
    let mut visited = std::collections::BTreeSet::new();
    for start in facts.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut pending = vec![*start];
        let mut component = Vec::new();
        let mut known = None;
        while let Some(node) = pending.pop() {
            if !visited.insert(node) {
                continue;
            }
            merge_slot(
                &mut known,
                *facts.get(&node).ok_or(RecordValidationError::Association)?,
            )?;
            component.push(node);
            if let Some(neighbors) = edges.get(&node) {
                pending.extend(neighbors);
            }
        }
        for node in component {
            if let SlotFact::Output(cell, slot) = node {
                slots
                    .get_mut(&(cell, slot))
                    .ok_or(RecordValidationError::Association)?
                    .origin
                    .slot = known;
            }
        }
    }
    Ok(())
}
