//! HTML tokens are checked before tree construction can discard forbidden tags.
use super::*;
use html5ever::tendril::TendrilSink;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer};
use html5ever::{QualName, local_name, ns, parse_fragment};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::cell::Cell;
use std::collections::BTreeMap;

/// Untrusted canonical HTML payload read by a cache adapter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodedHtml {
    /// Markup that must pass active validation and an exact canonical comparison.
    pub markup: String,
}

/// Owned HTML structure. Constructing nodes does not construct a validated wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlNode {
    /// Decoded text, escaped by the publisher.
    Text(String),
    /// Allowlisted element with typed image attributes.
    Element {
        /// Lowercase HTML name.
        name: String,
        /// Attributes in ASCII name order.
        attributes: BTreeMap<String, HtmlAttribute>,
        /// Ordered children, without a retained parser DOM.
        children: Vec<HtmlNode>,
    },
}

/// An inert attribute value or a verified image binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlAttribute {
    /// Text escaped as an attribute value by the publisher.
    Text(String),
    /// Image source resolved through the current asset publisher.
    Asset(ExecutionAsset),
}

/// Immutable allowlisted HTML and typed image bindings, with no Serde trust path.
///
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedHtml;
/// let _: ValidatedHtml = serde_json::from_str("{}").unwrap();
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedHtml;
/// fn require_serializable<T: serde::Serialize>() {}
/// require_serializable::<ValidatedHtml>();
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::ValidatedHtml;
/// fn mutate(value: &mut ValidatedHtml) { value.canonical_content().markup.clear(); }
/// ```
/// ```compile_fail
/// use diplodocus::execution::output_safety::{ValidatedHtml, DecodedHtml};
/// let value = ValidatedHtml { content: DecodedHtml { markup: String::new() }, nodes: vec![], assets: vec![] };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedHtml {
    content: DecodedHtml,
    nodes: Vec<HtmlNode>,
    assets: Vec<ExecutionAsset>,
}
impl ValidatedHtml {
    /// Canonical accepted content for identity and cache projection.
    pub fn canonical_content(&self) -> &DecodedHtml {
        &self.content
    }
    /// Owned immutable tree; publishers must resolve typed assets themselves.
    pub fn nodes(&self) -> &[HtmlNode] {
        &self.nodes
    }
    /// Every image occurrence, including repeated references to the same bytes.
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset> {
        self.assets.iter()
    }
}

fn allowed(name: &str) -> bool {
    matches!(
        name,
        "p" | "br"
            | "hr"
            | "div"
            | "span"
            | "strong"
            | "em"
            | "b"
            | "i"
            | "s"
            | "sub"
            | "sup"
            | "code"
            | "pre"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "caption"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "th"
            | "td"
            | "a"
            | "img"
    )
}
fn discard(name: &str) -> bool {
    matches!(name, "class" | "id") || name.starts_with("data-")
}
fn attribute(
    element: &str,
    name: &str,
    value: &str,
) -> Result<Option<String>, HtmlRejectionReason> {
    use HtmlRejectionReason::Attribute;
    if discard(name) {
        return Ok(None);
    }
    let value = match (element, name) {
        (_, "title") | ("a", "href") | ("img", "src" | "alt") => value.into(),
        ("img", "width" | "height") | ("th" | "td", "colspan" | "rowspan") => {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Attribute);
            }
            let number: u64 = value.parse().map_err(|_| Attribute)?;
            if number == 0 {
                return Err(Attribute);
            }
            number.to_string()
        }
        ("ol", "start") => {
            let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Attribute);
            }
            value.parse::<i64>().map_err(|_| Attribute)?.to_string()
        }
        ("th" | "td", "scope") if matches!(value, "row" | "col" | "rowgroup" | "colgroup") => {
            value.into()
        }
        _ => return Err(Attribute),
    };
    Ok(Some(value))
}
struct Preflight(Cell<Option<HtmlRejectionReason>>);
impl TokenSink for Preflight {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let error = match token {
            Token::TagToken(tag) => {
                if !allowed(&tag.name) {
                    Some(HtmlRejectionReason::Element)
                } else {
                    tag.attrs
                        .iter()
                        .find_map(|a| attribute(&tag.name, &a.name.local, &a.value).err())
                }
            }
            Token::DoctypeToken(_) | Token::NullCharacterToken | Token::ParseError(_) => {
                Some(HtmlRejectionReason::Structure)
            }
            _ => None,
        };
        if self.0.get().is_none() {
            self.0.set(error);
        }
        TokenSinkResult::Continue
    }
}
fn preflight(markup: &str) -> Result<(), HtmlRejectionReason> {
    let input = BufferQueue::default();
    input.push_back(markup.into());
    let tokenizer = Tokenizer::new(Preflight(Cell::new(None)), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.get().map_or(Ok(()), Err)
}

#[derive(Debug)]
enum Error {
    Policy(HtmlRejectionReason),
    Asset(AssetError),
    Restore(RestoreRejection),
}
impl From<HtmlRejectionReason> for Error {
    fn from(value: HtmlRejectionReason) -> Self {
        Self::Policy(value)
    }
}
impl From<AssetError> for Error {
    fn from(value: AssetError) -> Self {
        Self::Asset(value)
    }
}
impl From<RestoreRejection> for Error {
    fn from(value: RestoreRejection) -> Self {
        Self::Restore(value)
    }
}

enum Images<'a> {
    Live(&'a mut PageAssetStore),
    Restore(&'a VerifiedAssets),
}
impl Images<'_> {
    fn resolve(
        &mut self,
        target: &str,
        context: &AuthoredOutputContext,
    ) -> Result<ExecutionAsset, Error> {
        match self {
            Self::Live(store) => {
                let target = urls::image(target).map_err(|error| match error {
                    urls::UrlError::RemoteImage => HtmlRejectionReason::RemoteImage,
                    urls::UrlError::Url => HtmlRejectionReason::Url,
                })?;
                let asset = store.stage_local(&target)?;
                if !context.owns_asset(&asset) {
                    return Err(AssetError::Collision.into());
                }
                Ok(asset)
            }
            Self::Restore(assets) => {
                let digest = target
                    .strip_prefix("diplodocus-asset:sha256:")
                    .ok_or(RestoreRejection::Url)?;
                let fingerprint = Fingerprint {
                    algorithm: "sha256".into(),
                    value: digest.into(),
                };
                if !valid_digest(&fingerprint) {
                    return Err(RestoreRejection::Url.into());
                }
                let asset = assets
                    .assets
                    .get(digest)
                    .ok_or(RestoreRejection::UnboundAsset)?;
                Ok(assets.resolve(&AssetUse::from(asset), context)?)
            }
        }
    }
}

fn children(
    handle: &Handle,
    context: &AuthoredOutputContext,
    images: &mut Images<'_>,
    assets: &mut Vec<ExecutionAsset>,
    depth: usize,
) -> Result<Vec<HtmlNode>, Error> {
    if depth > 128 {
        return Err(HtmlRejectionReason::Structure.into());
    }
    let mut output = Vec::new();
    for node in handle.children.borrow().iter() {
        match &node.data {
            NodeData::Text { contents } => {
                output.push(HtmlNode::Text(contents.borrow().to_string()))
            }
            NodeData::Comment { .. } => {}
            NodeData::Element { name, attrs, .. } => {
                if name.ns != ns!(html) || !allowed(&name.local) {
                    return Err(HtmlRejectionReason::Element.into());
                }
                let mut attributes = BTreeMap::new();
                for attr in attrs.borrow().iter() {
                    if !attr.name.ns.is_empty() || attr.name.prefix.is_some() {
                        return Err(HtmlRejectionReason::Attribute.into());
                    }
                    let key = attr.name.local.to_string();
                    if let Some(value) = attribute(&name.local, &key, &attr.value)? {
                        let value = if name.local.as_ref() == "img" && key == "src" {
                            let asset = images.resolve(&value, context)?;
                            assets.push(asset.clone());
                            HtmlAttribute::Asset(asset)
                        } else {
                            if key == "href" {
                                urls::link(&value, context)
                                    .map_err(|_| HtmlRejectionReason::Url)?;
                            }
                            HtmlAttribute::Text(value)
                        };
                        attributes.insert(key, value);
                    }
                }
                if name.local.as_ref() == "img" && !attributes.contains_key("src") {
                    return Err(HtmlRejectionReason::Structure.into());
                }
                output.push(HtmlNode::Element {
                    name: name.local.to_string(),
                    attributes,
                    children: children(node, context, images, assets, depth + 1)?,
                });
            }
            _ => return Err(HtmlRejectionReason::Structure.into()),
        }
    }
    Ok(output)
}
fn escape(value: &str, attr: bool, output: &mut String) {
    for c in value.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attr => output.push_str("&quot;"),
            _ => output.push(c),
        }
    }
}
fn serialize(nodes: &[HtmlNode], output: &mut String) {
    for node in nodes {
        match node {
            HtmlNode::Text(text) => escape(text, false, output),
            HtmlNode::Element {
                name,
                attributes,
                children,
            } => {
                output.push('<');
                output.push_str(name);
                for (key, value) in attributes {
                    output.push(' ');
                    output.push_str(key);
                    output.push_str("=\"");
                    match value {
                        HtmlAttribute::Text(text) => escape(text, true, output),
                        HtmlAttribute::Asset(asset) => {
                            output.push_str("diplodocus-asset:sha256:");
                            output.push_str(&asset.reference.fingerprint.value);
                        }
                    }
                    output.push('"');
                }
                output.push('>');
                serialize(children, output);
                if !matches!(name.as_str(), "br" | "hr" | "img") {
                    output.push_str("</");
                    output.push_str(name);
                    output.push('>');
                }
            }
        }
    }
}
fn validate(
    markup: &str,
    context: &AuthoredOutputContext,
    mut images: Images<'_>,
) -> Result<ValidatedHtml, Error> {
    preflight(markup)?;
    let dom = parse_fragment(
        RcDom::default(),
        Default::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        false,
    )
    .one(markup);
    let root = dom
        .document
        .children
        .borrow()
        .first()
        .cloned()
        .ok_or(HtmlRejectionReason::Structure)?;
    let mut assets = Vec::new();
    let nodes = children(&root, context, &mut images, &mut assets, 0)?;
    let mut markup = String::new();
    serialize(&nodes, &mut markup);
    Ok(ValidatedHtml {
        content: DecodedHtml { markup },
        nodes,
        assets,
    })
}

/// Validate live HTML and stage every local image before later cells can alter it.
pub fn validate_html_live(
    markup: &str,
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &mut PageAssetStore,
) -> Result<Validation<ValidatedHtml>, ExecutionFailure> {
    let attribution = DiagnosticAttribution::output(context, origin, None);
    if origin.fragment.is_some() || origin.cell_span.start > origin.cell_span.end {
        return Ok(Validation::Rejected {
            diagnostics: vec![ExecutionDiagnostic::HtmlRejected {
                attribution,
                reason: HtmlRejectionReason::Structure,
            }],
        });
    }
    let diagnostic = match validate(markup, context, Images::Live(assets)) {
        Ok(value) => {
            return Ok(Validation::Accepted {
                value,
                diagnostics: Vec::new(),
            });
        }
        Err(Error::Asset(error)) => {
            if let Some(kind) = error.failure_kind() {
                return Err(failure(kind, context, origin));
            }
            if error == AssetError::UnsafeSvg {
                ExecutionDiagnostic::SvgRejected { attribution }
            } else {
                ExecutionDiagnostic::InvalidImage {
                    attribution,
                    media_type: "image/*".into(),
                }
            }
        }
        Err(Error::Policy(reason)) => ExecutionDiagnostic::HtmlRejected {
            attribution,
            reason,
        },
        Err(Error::Restore(_)) => unreachable!("live HTML cannot invoke restore"),
    };
    Ok(Validation::Rejected {
        diagnostics: vec![diagnostic],
    })
}

/// Revalidate exact canonical HTML using only verified assets, with no source lookup.
pub fn restore_html(
    decoded: DecodedHtml,
    origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &VerifiedAssets,
) -> Result<ValidatedHtml, RestoreRejection> {
    if origin.fragment.is_some() || origin.cell_span.start > origin.cell_span.end {
        return Err(RestoreRejection::Structure);
    }
    restore_html_content(decoded, context, assets)
}

/// Revalidate content whose caller separately checks current producer attribution.
pub(crate) fn restore_html_content(
    decoded: DecodedHtml,
    context: &AuthoredOutputContext,
    assets: &VerifiedAssets,
) -> Result<ValidatedHtml, RestoreRejection> {
    let value =
        validate(&decoded.markup, context, Images::Restore(assets)).map_err(
            |error| match error {
                Error::Restore(error) => error,
                Error::Policy(HtmlRejectionReason::Url | HtmlRejectionReason::RemoteImage) => {
                    RestoreRejection::Url
                }
                Error::Policy(_) => RestoreRejection::Structure,
                Error::Asset(_) => RestoreRejection::AssetMismatch,
            },
        )?;
    if value.canonical_content() != &decoded {
        return Err(RestoreRejection::NonCanonical);
    }
    Ok(value)
}
