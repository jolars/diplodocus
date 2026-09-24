//! Explicit artifact projection. Re-encoding the checked result closes every
//! record, including nullable fields and nested fragment evidence.
use super::*;
use crate::execution::output_safety::*;
use crate::execution::validated::{
    DiagnosticEvidence, OutputProducer, OwnedRepresentation, SlotEvidence,
};
use crate::ir::*;
use serde_json::{Value, json};

mod diagnostics;
mod restore;
pub(super) use restore::restore;

pub(super) const SCHEMA: &str = "page-execution-artifact-v1";
const RESULT_DOMAIN: &str = "diplodocus/page-execution-result-v1";

pub(super) fn envelope(bytes: &[u8], key: &CanonicalValue) -> Result<Value, CanonicalError> {
    let manifest = CanonicalValue::decode(bytes)?;
    let fields = manifest.fields(&["schema", "key", "key_input", "result_digest", "result"])?;
    identity::validate_key_input(&fields["key_input"])?;
    require(fields["key_input"] == *key)?;
    require(fields["schema"] == CanonicalValue::String(SCHEMA.into()))?;
    require(
        fields["key"]
            == CanonicalValue::String(identity::domain_digest(
                "diplodocus/page-execution-key-v1",
                key,
            )?),
    )?;
    require(
        fields["result_digest"]
            == CanonicalValue::String(identity::domain_digest(RESULT_DOMAIN, &fields["result"])?),
    )?;
    json_value(&manifest)
}

pub(super) fn require(ok: bool) -> Result<(), CanonicalError> {
    ok.then_some(()).ok_or(CanonicalError)
}
pub(super) fn json_value(value: &CanonicalValue) -> Result<Value, CanonicalError> {
    serde_json::from_slice(&value.encode()?).map_err(|_| CanonicalError)
}
pub(super) fn from_value<T: serde::de::DeserializeOwned>(
    value: &Value,
) -> Result<T, CanonicalError> {
    serde_json::from_value(value.clone()).map_err(|_| CanonicalError)
}
fn string(value: &Value) -> Result<&str, CanonicalError> {
    value.as_str().ok_or(CanonicalError)
}
fn list(value: &Value) -> Result<&[Value], CanonicalError> {
    value.as_array().map(Vec::as_slice).ok_or(CanonicalError)
}
fn index(value: &Value) -> Result<usize, CanonicalError> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(CanonicalError)
}
pub(super) fn fingerprint(value: &Value) -> Result<Fingerprint, CanonicalError> {
    let value = string(value)?
        .strip_prefix("sha256:")
        .ok_or(CanonicalError)?;
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    )?;
    Ok(Fingerprint {
        algorithm: "sha256".into(),
        value: value.into(),
    })
}
fn digest(value: &Fingerprint) -> String {
    format!("{}:{}", value.algorithm, value.value)
}
fn asset(value: &ExecutionAsset) -> Value {
    json!({"digest":digest(&value.reference.fingerprint),"media_type":value.media_type,"byte_size":value.byte_size})
}
fn default_origins(options: &ExecutionDefaults) -> Value {
    json!({"eval":options.eval.origin,"echo":options.echo.origin,"output":options.output.origin,"include":options.include.origin,"error":options.error.origin})
}
fn option_origins(options: &EffectiveCellOptions) -> Value {
    let mut value = default_origins(&options.execution);
    let fields = value.as_object_mut().expect("default origins object");
    for (key, origin) in [
        ("label", options.label.origin),
        ("fig-alt", options.fig_alt.origin),
        ("fig-cap", options.fig_cap.origin),
        ("fig-subcap", options.fig_subcap.origin),
    ] {
        fields.insert(key.into(), json!(origin));
    }
    value
}
fn preparation(prepared: &PreparedExecution, key: &Value) -> Result<Value, CanonicalError> {
    let source = std::str::from_utf8(prepared.source()).map_err(|_| CanonicalError)?;
    let mut declarations = Vec::new();
    let mut add = |cell: Option<usize>,
                   kind: &str,
                   key: Option<&str>,
                   span: SourceSpan|
     -> Result<(), CanonicalError> {
        let raw = source.get(span.start..span.end).ok_or(CanonicalError)?;
        declarations.push(json!({"cell":cell,"kind":kind,"key":key,"raw":raw,"span":span}));
        Ok(())
    };
    let parsed =
        crate::documents::parse_authored_document(source, crate::documents::AuthoredFormat::Qmd);
    if let Some(MetadataValue::Mapping { entries, .. }) = &parsed.document.frontmatter {
        for entry in entries {
            if entry.key.value == "execute" {
                if let MetadataValue::Mapping { entries, .. } = &entry.value {
                    for entry in entries {
                        add(None, "document", Some(&entry.key.value), entry.span)?;
                    }
                } else {
                    add(None, "document", Some("execute"), entry.span)?;
                }
            }
        }
    }
    for cell in &prepared.request().cells {
        if let Some(id) = &cell.cell.identifier {
            add(Some(cell.ordinal), "fence-identifier", None, id.span)?;
        }
        for declaration in &cell.cell.options {
            add(
                Some(cell.ordinal),
                match declaration.source {
                    CellOptionSource::InlineInfo => "inline",
                    CellOptionSource::HashpipeYaml => "hashpipe",
                },
                declaration.canonical_key.as_deref(),
                declaration.span,
            )?;
        }
    }
    declarations.sort_by_key(|d| (d["span"]["start"].as_u64(), d["span"]["end"].as_u64()));
    Ok(
        json!({"parser_version":prepared.request().page.parser_version,"defaults":key["options"]["defaults"],"default_origins":default_origins(&prepared.request().defaults),"declarations":declarations}),
    )
}

pub(super) fn encode(
    prepared: &PreparedExecution,
    key: &CanonicalValue,
    page: &ValidatedPage,
) -> Result<Vec<u8>, CanonicalError> {
    let key = json_value(key)?;
    let mut provenance = json!({"origin":"executed","preparation":preparation(prepared, &key)?});
    for name in [
        "page",
        "engine",
        "policies",
        "components",
        "kernel",
        "platform",
        "deadlines_ms",
        "environment_inputs",
    ] {
        provenance[name] = key[name].clone();
    }
    let mut expected = producing_provenance(&key, prepared)?;
    if let Some(actual) = &page.record().provenance {
        if let (
            ProvenanceActivity::Execution { origin, .. },
            ProvenanceActivity::Execution {
                origin: actual_origin,
                ..
            },
        ) = (&mut expected.execution.activity, &actual.execution.activity)
        {
            *origin = *actual_origin;
        }
        // Both fresh and restored carriers project the original producing origin.
        require(matches!(
            actual.execution.activity,
            ProvenanceActivity::Execution { .. }
        ))?;
        require(*actual == expected)?;
    } else {
        return Err(CanonicalError);
    }
    let mut cells = Vec::new();
    for cell in &page.record().cells {
        let mut outputs = Vec::new();
        for output in &cell.outputs {
            let mut representations = Vec::new();
            for i in 0..output.representations.len().max(1) {
                let projection = page
                    .canonical_representation(cell.ordinal, output.slot, i)?
                    .ok_or(CanonicalError)?;
                let (media, policy, producer) = match page.representation(
                    cell.ordinal,
                    output.slot,
                    i,
                ) {
                    Some(ValidatedRepresentationRef::Text(_)) => (Some("text/plain"), None, None),
                    Some(ValidatedRepresentationRef::Markdown(_)) => (
                        Some("text/markdown"),
                        Some("qmd-mvp-v1"),
                        Some(
                            json!({"name":"panache-parser","version":prepared.request().page.parser_version}),
                        ),
                    ),
                    Some(ValidatedRepresentationRef::Html(_)) => (
                        Some("text/html"),
                        Some("html-mvp-v1"),
                        Some(
                            json!({"name":"diplodocus-html-sanitizer","version":env!("CARGO_PKG_VERSION")}),
                        ),
                    ),
                    Some(ValidatedRepresentationRef::Asset(a)) => (
                        Some(a.media_type.as_str()),
                        Some(if a.media_type == "image/svg+xml" {
                            "svg-mvp-v1"
                        } else {
                            "mime-mvp-v1"
                        }),
                        Some(
                            json!({"name":if a.media_type == "image/svg+xml" { "diplodocus-svg-validator" } else { "diplodocus-raster-validator" },"version":env!("CARGO_PKG_VERSION")}),
                        ),
                    ),
                    None if matches!(output.output.kind, CellOutputKind::Error { .. }) => {
                        (Some("text/plain"), None, None)
                    }
                    None => (None, Some("mime-mvp-v1"), None),
                };
                let fragment = page.representation_origin(cell.ordinal, output.slot, i).and_then(|origin| origin.fragment.map(|fragment| json!({"ordinal":fragment.ordinal,"slot":origin.slot,"byte_length":fragment.byte_length})));
                representations.push(json!({"kind":projection.kind,"media_type":media,"content":json_value(&projection.content)?,"content_digest":digest(&projection.fingerprint),"producing_cell":output.updating_cell.unwrap_or(output.producing_cell),"policy":policy,"producer":producer,"fragment":fragment}));
            }
            let (kind, stream) = match output.output.kind {
                CellOutputKind::Stream { stream } => ("stream", Some(stream)),
                CellOutputKind::Display => ("display", None),
                CellOutputKind::Error { .. } => ("error", None),
            };
            let indices = output
                .diagnostic_indices
                .iter()
                .map(|i| {
                    i.checked_sub(page.execution_diagnostic_offset())
                        .ok_or(CanonicalError)
                })
                .collect::<Result<Vec<_>, _>>()?;
            outputs.push(json!({"kind":kind,"stream":stream,"owning_cell":output.owning_cell,"producing_cell":output.producing_cell,"updating_cell":output.updating_cell,"slot":output.slot,"offered_mime_types":output.offered_mime_types,"selected_mime_type":output.selected_mime_type,"representations":representations,"diagnostic_indices":indices}));
        }
        let (outcome, skip_reason) = match cell.outcome {
            CellOutcome::Ok => ("ok", None),
            CellOutcome::AllowedError => ("allowed-error", None),
            CellOutcome::Skipped { reason } => ("skipped", Some(reason)),
        };
        cells.push(json!({"ordinal":cell.ordinal,"label":cell.options.label.value,"span":cell.span,"source_segments":cell.source_segments,"submitted_source_digest":cell.submitted_source_fingerprint.as_ref().map(digest),"effective":key["options"]["cells"][cell.ordinal]["effective"],"option_origins":option_origins(&cell.options),"outcome":outcome,"skip_reason":skip_reason,"outputs":outputs}));
    }
    let diagnostics = page
        .diagnostics()
        .iter()
        .map(diagnostics::encode)
        .collect::<Result<Vec<_>, _>>()?;
    let result = CanonicalValue::from_json(
        json!({"ir_schema":"execution-result-v1","provenance":provenance,"cells":cells,"diagnostics":diagnostics,"assets":page.record().assets.iter().map(asset).collect::<Vec<_>>()}),
    )?;
    let key_input = CanonicalValue::from_json(key.clone())?;
    CanonicalValue::from_json(json!({"schema":SCHEMA,"key":identity::domain_digest("diplodocus/page-execution-key-v1", &key_input)?,"key_input":key,"result_digest":identity::domain_digest(RESULT_DOMAIN, &result)?,"result":json_value(&result)?}))?.encode()
}

pub(super) fn producing_provenance(
    key: &Value,
    prepared: &PreparedExecution,
) -> Result<PageExecutionProvenance, CanonicalError> {
    let kernel = &key["kernel"];
    let runtime = &kernel["runtime"];
    let version = string(&key["engine"]["version"])?;
    let mut components = BTreeMap::new();
    for c in list(&key["components"])? {
        components.insert(
            string(&c["role"])?.into(),
            ExecutionComponent {
                name: string(&c["name"])?.into(),
                version: string(&c["version"])?.into(),
            },
        );
    }
    let mut inputs = Vec::new();
    for input in list(&key["environment_inputs"])? {
        inputs.push(InputFingerprint {
            source: SourceLocation {
                repository: string(&input["repository"])?.into(),
                path: string(&input["path"])?
                    .try_into()
                    .map_err(|_| CanonicalError)?,
                span: None,
            },
            fingerprint: fingerprint(&input["digest"])?,
        });
    }
    let page = &prepared.request().page;
    Ok(PageExecutionProvenance {
        execution: crate::provenance::ExecutionObservation {
            engine: crate::configuration::ExecutionEngine::Jupyter,
            kernel: KernelProvenance {
                name: string(&kernel["name"])?.into(),
                language: Some(string(&runtime["language"])?.into()),
                language_version: Some(string(&runtime["language_version"])?.into()),
                version: Some(string(&runtime["implementation_version"])?.into()),
            },
            origin: ExecutionOrigin::Executed,
            tools: BTreeMap::from([
                ("diplodocus".into(), version.into()),
                ("jupyter".into(), version.into()),
            ]),
            declared_environment_inputs: inputs,
        }
        .into_provenance(
            Some(DiagnosticSource::Repository {
                repository: page.source.repository.clone(),
                path: page.source.path.clone(),
            }),
            None,
        ),
        engine_build_fingerprint: fingerprint(&key["engine"]["build_digest"])?,
        components,
        policies: ExecutionPolicies {
            execution: string(&key["policies"]["execution"])?.into(),
            mime: string(&key["policies"]["mime"])?.into(),
            html: string(&key["policies"]["html"])?.into(),
            svg: string(&key["policies"]["svg"])?.into(),
        },
        kernel: KernelExecutionProvenance {
            spec_fingerprint: fingerprint(&kernel["spec_digest"])?,
            launch_fingerprint: fingerprint(&kernel["launch_digest"])?,
            search: from_value(&kernel["search"])?,
            interrupt_mode: from_value(&kernel["interrupt_mode"])?,
            implementation: string(&runtime["implementation"])?.into(),
            protocol_version: string(&runtime["protocol_version"])?.into(),
        },
        platform: from_value(&key["platform"])?,
        deadlines_ms: from_value(&key["deadlines_ms"])?,
    })
}
