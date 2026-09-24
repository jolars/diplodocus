use super::*;

pub(super) fn encode(diagnostic: &ExecutionDiagnostic) -> Result<Value, CanonicalError> {
    use ExecutionDiagnostic::*;
    let arguments = match diagnostic {
        KernelMessageIgnored { .. } => json!({"kind":"kernel-message-ignored"}),
        UnknownDisplayUpdate { .. } => json!({"kind":"unknown-display-update"}),
        NoSupportedRepresentation { mime_types, .. } => {
            json!({"kind":"no-supported-representation","mime_types":mime_types})
        }
        UnsupportedMedia { media_type, .. } => {
            json!({"kind":"unsupported-media","media_type":media_type})
        }
        InvalidMimeBundle { .. } => json!({"kind":"invalid-mime-bundle"}),
        InvalidTextPayload { media_type, .. } => {
            json!({"kind":"invalid-text-payload","media_type":media_type})
        }
        InvalidImage { media_type, .. } => json!({"kind":"invalid-image","media_type":media_type}),
        SvgRejected { .. } => json!({"kind":"svg-rejected"}),
        HtmlRejected { reason, .. } => {
            json!({"kind":"html-rejected","reason":match reason { HtmlRejectionReason::Element => "element", HtmlRejectionReason::Attribute => "attribute", HtmlRejectionReason::Url => "url", HtmlRejectionReason::RemoteImage => "remote-image", HtmlRejectionReason::Structure => "structure" }})
        }
        MarkdownRejected { reason, .. } => {
            json!({"kind":"markdown-rejected","reason":match reason { MarkdownRejectionReason::Url => "url", MarkdownRejectionReason::RemoteImage => "remote-image", MarkdownRejectionReason::Structure => "structure" }})
        }
        FragmentUnsupported { source_kind, .. } => {
            json!({"kind":"fragment-unsupported","source_kind":source_kind})
        }
    };
    let a = diagnostic.attribution();
    let source = match &a.source {
        None => Value::Null,
        Some(DiagnosticSource::Repository { repository, path }) => {
            json!({"repository":repository,"path":path})
        }
        _ => return Err(CanonicalError),
    };
    Ok(
        json!({"code":diagnostic.code().as_str(),"severity":"warning","arguments":arguments,"source":source,"cell":a.cell,"slot":a.slot,"fragment":a.fragment,"span":a.span,"related_spans":a.related_spans}),
    )
}

pub(super) fn decode(value: &Value) -> Result<ExecutionDiagnostic, CanonicalError> {
    use ExecutionDiagnostic::*;
    let source = &value["source"];
    let attribution = DiagnosticAttribution {
        source: if source.is_null() {
            None
        } else {
            Some(DiagnosticSource::Repository {
                repository: string(&source["repository"])?.into(),
                path: string(&source["path"])?
                    .try_into()
                    .map_err(|_| CanonicalError)?,
            })
        },
        cell: from_value(&value["cell"])?,
        slot: from_value(&value["slot"])?,
        fragment: from_value(&value["fragment"])?,
        span: from_value(&value["span"])?,
        related_spans: from_value(&value["related_spans"])?,
    };
    let a = &value["arguments"];
    let result = match string(&a["kind"])? {
        "kernel-message-ignored" => KernelMessageIgnored { attribution },
        "unknown-display-update" => UnknownDisplayUpdate { attribution },
        "no-supported-representation" => NoSupportedRepresentation {
            attribution,
            mime_types: from_value(&a["mime_types"])?,
        },
        "unsupported-media" => UnsupportedMedia {
            attribution,
            media_type: string(&a["media_type"])?.into(),
        },
        "invalid-mime-bundle" => InvalidMimeBundle { attribution },
        "invalid-text-payload" => InvalidTextPayload {
            attribution,
            media_type: string(&a["media_type"])?.into(),
        },
        "invalid-image" => InvalidImage {
            attribution,
            media_type: string(&a["media_type"])?.into(),
        },
        "svg-rejected" => SvgRejected { attribution },
        "html-rejected" => HtmlRejected {
            attribution,
            reason: match string(&a["reason"])? {
                "element" => HtmlRejectionReason::Element,
                "attribute" => HtmlRejectionReason::Attribute,
                "url" => HtmlRejectionReason::Url,
                "remote-image" => HtmlRejectionReason::RemoteImage,
                "structure" => HtmlRejectionReason::Structure,
                _ => return Err(CanonicalError),
            },
        },
        "markdown-rejected" => MarkdownRejected {
            attribution,
            reason: match string(&a["reason"])? {
                "url" => MarkdownRejectionReason::Url,
                "remote-image" => MarkdownRejectionReason::RemoteImage,
                "structure" => MarkdownRejectionReason::Structure,
                _ => return Err(CanonicalError),
            },
        },
        "fragment-unsupported" => FragmentUnsupported {
            attribution,
            source_kind: string(&a["source_kind"])?.into(),
        },
        _ => return Err(CanonicalError),
    };
    require(encode(&result)? == *value)?;
    Ok(result)
}
