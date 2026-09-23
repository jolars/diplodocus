//! Decode before classifying URLs; normalization must not erase traversal evidence.
use super::AuthoredOutputContext;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer};
use percent_encoding::percent_decode_str;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy)]
pub(super) enum UrlError {
    Url,
    RemoteImage,
}

struct EntityDecoder(RefCell<Option<String>>);
impl TokenSink for EntityDecoder {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        if let Token::TagToken(tag) = token {
            for attribute in tag.attrs {
                if attribute.name.local.as_ref() == "href" {
                    self.0.replace(Some(attribute.value.to_string()));
                }
            }
        }
        TokenSinkResult::Continue
    }
}

fn entities(value: &str) -> String {
    let input = BufferQueue::default();
    input.push_back(format!("<a href=\"{}\">", value.replace('"', "&#34;")).into());
    let tokenizer = Tokenizer::new(EntityDecoder(RefCell::new(None)), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().unwrap_or_default()
}

fn decoded(value: &str) -> Result<String, UrlError> {
    // Repeated escapes must not conceal a forbidden scheme or a path separator.
    // Bound the work and reject deeper encodings rather than interpreting them.
    let mut value = value.to_owned();
    for _ in 0..8 {
        let next = entities(&value);
        let next = percent_decode_str(&next)
            .decode_utf8()
            .map_err(|_| UrlError::Url)?
            .into_owned();
        if next == value {
            if next.is_empty()
                || next.trim() != next
                || next.chars().any(char::is_control)
                || next.contains('\\')
            {
                return Err(UrlError::Url);
            }
            return Ok(next);
        }
        value = next;
    }
    Err(UrlError::Url)
}

pub(super) fn link(value: &str, context: &AuthoredOutputContext) -> Result<(), UrlError> {
    let decoded = decoded(value)?;
    if decoded.starts_with('/') {
        return Err(UrlError::Url);
    }
    if let Some(fragment) = decoded.strip_prefix('#') {
        return context
            .anchors()
            .contains(fragment)
            .then_some(())
            .ok_or(UrlError::Url);
    }
    let path = decoded.split(['?', '#']).next().unwrap_or_default();
    if path.contains(':') {
        let url = url::Url::parse(&decoded).map_err(|_| UrlError::Url)?;
        return match url.scheme() {
            "https" | "http" if url.host_str().is_some() => Ok(()),
            "mailto" if !url.path().is_empty() => Ok(()),
            _ => Err(UrlError::Url),
        };
    }
    let mut depth = context.source().path.as_str().split('/').count() - 1;
    for part in path.split('/') {
        match part {
            ".." => {
                depth = depth.checked_sub(1).ok_or(UrlError::Url)?;
            }
            "." | "" => {}
            _ => depth += 1,
        }
    }
    Ok(())
}

pub(super) fn image(value: &str) -> Result<String, UrlError> {
    let decoded = decoded(value)?;
    if decoded.starts_with("//") {
        return Err(UrlError::RemoteImage);
    }
    if decoded.contains(':') {
        return Err(
            if decoded.to_ascii_lowercase().starts_with("https:")
                || decoded.to_ascii_lowercase().starts_with("http:")
            {
                UrlError::RemoteImage
            } else {
                UrlError::Url
            },
        );
    }
    if decoded.contains(['?', '#']) {
        return Err(UrlError::Url);
    }
    // PageAssetStore performs the fatal path and filesystem boundary checks.
    // Protect literal percent characters from its single URL-decoding pass.
    Ok(decoded.replace('%', "%25"))
}
