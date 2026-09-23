//! The single explicit live/cache content projection. No workspace Serde is used.
use super::*;
use crate::ir::{Attributes, CalloutKind, SourceSpan, SpannedString, TableAlignment};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Storage-independent content input. Decoded content is still untrusted.
#[derive(Debug)]
pub enum RepresentationContent<'a> {
    /// Exact literal UTF-8 text.
    Text(&'a str),
    /// Structured exception text.
    Error {
        /// Exception name.
        name: &'a str,
        /// Exception message.
        value: &'a str,
        /// Ordered traceback frames.
        traceback: &'a [String],
    },
    /// Typed inert Markdown content, including image metadata.
    Markdown(&'a DecodedMarkdown),
    /// Canonical allowlisted markup.
    Html(&'a DecodedHtml),
    /// Image digest, MIME type, and size.
    Asset(AssetUse),
    /// Offered MIME names for an unsupported display.
    Unsupported(&'a BTreeSet<String>),
}
/// Canonical content and its policy-defined fingerprint; no rendering authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepresentationProjection {
    /// Cache representation kind.
    pub kind: &'static str,
    /// Exact restricted content value.
    pub content: CanonicalValue,
    /// Digest excluding selection, attribution, and producer policy evidence.
    pub fingerprint: Fingerprint,
}
/// Project all representation kinds with one agreed live/cache digest mapping.
///
/// This operation encodes content only. It does not validate decoded content or
/// grant trust. Trusted consumers obtain inputs from [`ValidatedPage`].
pub fn project_representation(
    input: RepresentationContent<'_>,
) -> Result<RepresentationProjection, CanonicalError> {
    let (kind, content, raw_digest) = match input {
        RepresentationContent::Text(text) => (
            "text",
            json!({"type":"literal", "text":text}),
            Some(fingerprint_bytes(text.as_bytes())),
        ),
        RepresentationContent::Error {
            name,
            value,
            traceback,
        } => (
            "error",
            json!({"type":"error", "name":name, "value":value, "traceback":traceback}),
            None,
        ),
        RepresentationContent::Markdown(value) => {
            ("markdown", json!({"blocks":blocks(&value.blocks)}), None)
        }
        RepresentationContent::Html(value) => (
            "html-candidate",
            json!({"markup":value.markup}),
            Some(fingerprint_bytes(value.markup.as_bytes())),
        ),
        RepresentationContent::Asset(value) => {
            ("asset", json!({"asset":asset(&value)}), Some(value.digest))
        }
        RepresentationContent::Unsupported(mime_types) => {
            ("unsupported", json!({"mime_types":mime_types}), None)
        }
    };
    let fingerprint = match raw_digest {
        Some(digest) => digest,
        None => {
            let value = CanonicalValue::from_json(json!({"kind":kind, "content":content}))?;
            let digest = identity::domain_digest("diplodocus/execution-representation-v1", &value)?;
            Fingerprint {
                algorithm: "sha256".into(),
                value: digest.strip_prefix("sha256:").ok_or(CanonicalError)?.into(),
            }
        }
    };
    // Error uses the cache's text tag but the error-specific domain input.
    Ok(RepresentationProjection {
        kind: if kind == "error" { "text" } else { kind },
        content: CanonicalValue::from_json(content)?,
        fingerprint,
    })
}
fn span(s: &SourceSpan) -> Value {
    json!({"start":s.start,"end":s.end})
}
fn spanned(s: &SpannedString) -> Value {
    json!({"value":s.value,"span":span(&s.span)})
}
fn attributes(a: &Attributes) -> Value {
    json!({"identifier":a.identifier.as_ref().map(spanned),"classes":a.classes.iter().map(spanned).collect::<Vec<_>>(),"key_values":a.key_values.iter().map(|v|json!({"key":spanned(&v.key),"value":spanned(&v.value)})).collect::<Vec<_>>()})
}
fn asset(a: &AssetUse) -> Value {
    json!({"digest":format!("{}:{}",a.digest.algorithm,a.digest.value),"media_type":a.media_type,"byte_size":a.byte_size})
}
fn blocks(nodes: &[FragmentBlock]) -> Vec<Value> {
    nodes.iter().map(block).collect()
}
fn inlines(nodes: &[FragmentInline]) -> Vec<Value> {
    nodes.iter().map(inline).collect()
}
fn block(node: &FragmentBlock) -> Value {
    use FragmentBlock::*;
    match node {
        Paragraph {
            inlines: nodes,
            span: s,
        } => json!({"type":"paragraph","inlines":inlines(nodes),"span":span(s)}),
        Heading {
            level,
            attributes: a,
            inlines: nodes,
            span: s,
        } => {
            json!({"type":"heading","level":level,"attributes":attributes(a),"inlines":inlines(nodes),"span":span(s)})
        }
        BlockQuote {
            blocks: nodes,
            span: s,
        } => json!({"type":"block-quote","blocks":blocks(nodes),"span":span(s)}),
        List {
            ordered,
            items,
            span: s,
        } => {
            json!({"type":"list","ordered":ordered,"items":items.iter().map(|item|json!({"checked":item.checked,"blocks":blocks(&item.blocks),"span":span(&item.span)})).collect::<Vec<_>>(),"span":span(s)})
        }
        ThematicBreak { span: s } => json!({"type":"thematic-break","span":span(s)}),
        CodeBlock {
            language,
            source,
            source_segments,
            span: s,
        } => {
            json!({"type":"code-block","language":language,"source":source,"source_segments":source_segments.iter().map(|s|json!({"text":s.text,"span":span(&s.span)})).collect::<Vec<_>>(),"span":span(s)})
        }
        Table {
            caption,
            alignments,
            rows,
            span: s,
        } => {
            json!({"type":"table","caption":inlines(caption),"alignments":alignments.iter().map(|a|match a { TableAlignment::Default=>"default", TableAlignment::Left=>"left", TableAlignment::Center=>"center", TableAlignment::Right=>"right" }).collect::<Vec<_>>(),"rows":rows.iter().map(|r|json!({"header":r.header,"cells":r.cells.iter().map(|c|json!({"blocks":blocks(&c.blocks),"span":span(&c.span)})).collect::<Vec<_>>(),"span":span(&r.span)})).collect::<Vec<_>>(),"span":span(s)})
        }
        Callout {
            kind,
            attributes: a,
            blocks: nodes,
            span: s,
        } => {
            json!({"type":"callout","kind":match kind { CalloutKind::Note=>"note", CalloutKind::Tip=>"tip", CalloutKind::Important=>"important", CalloutKind::Warning=>"warning", CalloutKind::Caution=>"caution" },"attributes":attributes(a),"blocks":blocks(nodes),"span":span(s)})
        }
        Unsupported {
            source_kind,
            raw,
            span: s,
        } => json!({"type":"unsupported","source_kind":source_kind,"raw":raw,"span":span(s)}),
    }
}
fn inline(node: &FragmentInline) -> Value {
    use FragmentInline::*;
    match node {
        Text { value, span: s } => json!({"type":"text","value":value,"span":span(s)}),
        Space { span: s } => json!({"type":"space","span":span(s)}),
        SoftBreak { span: s } => json!({"type":"soft-break","span":span(s)}),
        HardBreak { span: s } => json!({"type":"hard-break","span":span(s)}),
        NonbreakingSpace { span: s } => json!({"type":"nonbreaking-space","span":span(s)}),
        Emphasis {
            inlines: nodes,
            span: s,
        } => json!({"type":"emphasis","inlines":inlines(nodes),"span":span(s)}),
        Strong {
            inlines: nodes,
            span: s,
        } => json!({"type":"strong","inlines":inlines(nodes),"span":span(s)}),
        Strikeout {
            inlines: nodes,
            span: s,
        } => json!({"type":"strikeout","inlines":inlines(nodes),"span":span(s)}),
        Code { value, span: s } => json!({"type":"code","value":value,"span":span(s)}),
        Link {
            inlines: nodes,
            target,
            title,
            attributes: a,
            span: s,
        } => {
            json!({"type":"link","inlines":inlines(nodes),"target":target,"title":title,"attributes":attributes(a),"span":span(s)})
        }
        Image {
            alt,
            asset: a,
            title,
            attributes: attrs,
            span: s,
        } => {
            json!({"type":"image","alt":inlines(alt),"asset":asset(a),"title":title,"attributes":attributes(attrs),"span":span(s)})
        }
        AutoLink { target, span: s } => json!({"type":"auto-link","target":target,"span":span(s)}),
        SemanticReference {
            target,
            target_span,
            span: s,
        } => {
            json!({"type":"semantic-reference","target":target,"target_span":span(target_span),"span":span(s)})
        }
        Unsupported {
            source_kind,
            raw,
            span: s,
        } => json!({"type":"unsupported","source_kind":source_kind,"raw":raw,"span":span(s)}),
    }
}
