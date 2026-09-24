use super::*;

/// No source-image paths are resolved and no staging files are written here.
pub(in crate::execution::cache) fn restore(
    bytes: &[u8],
    key: &CanonicalValue,
    prepared: &PreparedExecution,
    asset_bytes: &BTreeMap<String, Vec<u8>>,
) -> Result<ValidatedPage, CanonicalError> {
    let value = envelope(bytes, key)?;
    let result = &value["result"];
    let key_json = &value["key_input"];
    let mut verified = VerifiedAssets::new();
    let mut assets = Vec::new();
    for entry in list(&result["assets"])? {
        let asset = decode_asset(entry, prepared)?;
        let bytes = asset_bytes
            .get(&asset.reference.fingerprint.value)
            .ok_or(CanonicalError)?;
        verified
            .verify_bytes(prepared.context(), &asset, bytes)
            .map_err(|_| CanonicalError)?;
        assets.push(asset);
    }
    require(asset_bytes.len() == assets.len())?;
    let diagnostics = list(&result["diagnostics"])?
        .iter()
        .map(diagnostics::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let mut cells = Vec::new();
    let mut slots = Vec::new();
    require(list(&result["cells"])?.len() == prepared.request().cells.len())?;
    for (cell, input) in list(&result["cells"])?
        .iter()
        .zip(&prepared.request().cells)
    {
        let key_cell = &key_json["options"]["cells"][input.ordinal];
        let eligible = key_cell["eligible"].as_bool().ok_or(CanonicalError)?;
        let outcome = match string(&cell["outcome"])? {
            "ok" if eligible => CellOutcome::Ok,
            "allowed-error" if eligible && input.options.execution.error.value => {
                CellOutcome::AllowedError
            }
            "skipped" if !eligible => CellOutcome::Skipped {
                reason: if key_cell["language"] != key_json["kernel"]["runtime"]["language"] {
                    CellSkipReason::LanguageMismatch
                } else {
                    CellSkipReason::EvalFalse
                },
            },
            _ => return Err(CanonicalError),
        };
        let mut outputs = Vec::new();
        for output in list(&cell["outputs"])? {
            let producing_cell = index(&output["producing_cell"])?;
            let updating_cell: Option<usize> = from_value(&output["updating_cell"])?;
            let current = updating_cell.unwrap_or(producing_cell);
            let current_input = prepared
                .request()
                .cells
                .get(current)
                .ok_or(CanonicalError)?;
            require(
                producing_cell == input.ordinal
                    && current >= producing_cell
                    && key_json["options"]["cells"][producing_cell]["eligible"] == true
                    && key_json["options"]["cells"][current]["eligible"] == true,
            )?;
            let slot = index(&output["slot"])?;
            let reps = list(&output["representations"])?;
            let kind = match string(&output["kind"])? {
                "stream" => CellOutputKind::Stream {
                    stream: from_value(&output["stream"])?,
                },
                "display" => CellOutputKind::Display,
                "error" => {
                    require(reps.len() == 1 && cell["outcome"] == "allowed-error")?;
                    let content = &reps[0]["content"];
                    CellOutputKind::Error {
                        name: string(&content["name"])?.into(),
                        message: string(&content["value"])?.into(),
                        traceback: from_value(&content["traceback"])?,
                    }
                }
                _ => return Err(CanonicalError),
            };
            let mut evidence = Vec::new();
            let mut owned = Vec::new();
            let mut portable = Vec::new();
            let mut provenance = Vec::new();
            for rep in reps {
                let content = &rep["content"];
                let (value, representation) = match string(&rep["kind"])? {
                    "text" if content["type"] == "literal" => {
                        let text = string(&content["text"])?;
                        (
                            OwnedRepresentation::Text(text.into()),
                            OutputRepresentation::PlainText {
                                media_type: "text/plain".into(),
                                text: text.into(),
                            },
                        )
                    }
                    "text"
                        if content["type"] == "error"
                            && matches!(kind, CellOutputKind::Error { .. }) =>
                    {
                        continue;
                    }
                    "unsupported" if matches!(kind, CellOutputKind::Display) && reps.len() == 1 => {
                        continue;
                    }
                    "markdown" => {
                        let f = &rep["fragment"];
                        let origin = OutputOrigin {
                            cell: current,
                            slot: index(&f["slot"])?,
                            cell_span: current_input.cell.span,
                            fragment: Some(FragmentIdentity {
                                ordinal: index(&f["ordinal"])?,
                                byte_length: index(&f["byte_length"])?,
                            }),
                        };
                        let mut content = content.clone();
                        decode_image_uses(&mut content)?;
                        let value = restore_markdown(
                            from_value(&content)?,
                            &origin,
                            prepared.context(),
                            &verified,
                        )
                        .map_err(|_| CanonicalError)?;
                        let representation = OutputRepresentation::MarkdownBlocks {
                            media_type: "text/markdown".into(),
                            blocks: value.blocks().to_vec(),
                        };
                        provenance.push(value.provenance().clone());
                        (
                            OwnedRepresentation::Markdown {
                                value: Box::new(value),
                                origin,
                            },
                            representation,
                        )
                    }
                    "html-candidate" => {
                        let value = restore_html_content(
                            from_value(content)?,
                            prepared.context(),
                            &verified,
                        )
                        .map_err(|_| CanonicalError)?;
                        let representation = OutputRepresentation::HtmlCandidate {
                            media_type: "text/html".into(),
                            html: UnvalidatedHtml::new(value.canonical_content().markup.clone()),
                            policy: "html-mvp-v1".into(),
                            sanitizer: SanitizerProvenance {
                                name: "diplodocus-html-sanitizer".into(),
                                version: env!("CARGO_PKG_VERSION").into(),
                            },
                        };
                        (OwnedRepresentation::Html { value }, representation)
                    }
                    "asset" => {
                        let asset = decode_asset(&content["asset"], prepared)?;
                        let representation = OutputRepresentation::Asset {
                            media_type: asset.media_type.clone(),
                            asset: asset.reference.clone(),
                        };
                        (OwnedRepresentation::Asset(asset), representation)
                    }
                    _ => return Err(CanonicalError),
                };
                evidence.push(RepresentationEvidence {
                    content_fingerprint: fingerprint(&rep["content_digest"])?,
                    producing_cell: index(&rep["producing_cell"])?,
                    policy: from_value(&rep["policy"])?,
                });
                owned.push(value);
                portable.push(representation);
            }
            slots.push(SlotEvidence {
                owning_cell: input.ordinal,
                slot,
                origin: OutputProducer {
                    cell: current,
                    slot: None,
                    cell_span: current_input.cell.span,
                },
                representations: owned,
            });
            outputs.push(ExecutionOutput {
                owning_cell: index(&output["owning_cell"])?,
                producing_cell,
                updating_cell,
                slot,
                output: CellOutput {
                    kind,
                    representations: portable,
                    provenance,
                },
                offered_mime_types: from_value(&output["offered_mime_types"])?,
                selected_mime_type: from_value(&output["selected_mime_type"])?,
                representations: evidence,
                diagnostic_indices: from_value(&output["diagnostic_indices"])?,
            });
        }
        cells.push(CellExecutionResult {
            ordinal: input.ordinal,
            language: input.cell.language.clone(),
            span: input.cell.span,
            source_segments: input.cell.source_segments.clone(),
            submitted_source_fingerprint: eligible
                .then(|| crate::provenance::fingerprint_bytes(input.cell.source.as_bytes())),
            options: input.options.clone(),
            outcome,
            outputs,
        });
    }
    let record = PageExecutionRecord {
        page: prepared.request().page.clone(),
        defaults: prepared.request().defaults.clone(),
        cells,
        diagnostics: diagnostics
            .iter()
            .map(|d| d.to_diagnostic(prepared.context().collection()))
            .collect(),
        assets,
        provenance: Some(producing_provenance(key_json, prepared)?),
    };
    let page = ValidatedPage::restored(
        prepared,
        record,
        slots,
        DiagnosticEvidence::from(diagnostics),
        verified,
    )
    .map_err(|_| CanonicalError)?;
    // This exact comparison also rejects omitted nulls, unknown fields, forged
    // source evidence, wrong producer metadata, digests, and reordered sets.
    require(encode(prepared, key, &page)? == bytes)?;
    crate::execution::validate_figure_options(&page.record().page, &page.record().cells)
        .map_err(|_| CanonicalError)?;
    Ok(page)
}

pub(super) fn decode_asset(
    value: &Value,
    prepared: &PreparedExecution,
) -> Result<ExecutionAsset, CanonicalError> {
    let fingerprint = fingerprint(&value["digest"])?;
    let page = &prepared.request().page;
    let mut identity = b"diplodocus/execution-assets-v1\0".to_vec();
    for field in [
        page.source.repository.as_str(),
        page.collection.as_str(),
        page.source.path.as_str(),
    ] {
        identity.extend_from_slice(&(field.len() as u64).to_be_bytes());
        identity.extend_from_slice(field.as_bytes());
    }
    let namespace = crate::provenance::fingerprint_bytes(&identity).value;
    Ok(ExecutionAsset {
        reference: AssetReference {
            path: format!("execution-assets/{namespace}/sha256/{}", fingerprint.value)
                .try_into()
                .map_err(|_| CanonicalError)?,
            fingerprint,
        },
        media_type: string(&value["media_type"])?.into(),
        byte_size: value["byte_size"].as_u64().ok_or(CanonicalError)?,
    })
}

fn decode_image_uses(value: &mut Value) -> Result<(), CanonicalError> {
    match value {
        Value::Object(fields) => {
            if fields.get("type") == Some(&json!("image")) {
                let asset = fields.get_mut("asset").ok_or(CanonicalError)?;
                let digest = fingerprint(&asset["digest"])?;
                asset["digest"] = json!(digest);
            }
            for child in fields.values_mut() {
                decode_image_uses(child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                decode_image_uses(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}
