use super::{CanonicalUrlStyle, Site, SiteError, clean_route, encode_url_path};

impl Site<'_> {
    pub(super) fn assign_redirects(&mut self) -> Result<(), SiteError> {
        if self.presentation.redirects.is_empty() {
            return Ok(());
        }
        if self.presentation.redirects.len() > 2_000 {
            return Err(invalid(
                "at most 2,000 redirects can be emitted in _redirects",
            ));
        }
        if let Some(route) = self.pages.keys().find(|route| {
            route.starts_with("_redirects/")
                || (self.presentation.canonical_url_style == CanonicalUrlStyle::Clean
                    && clean_route(route).trim_end_matches('/') == "_redirects")
        }) {
            return Err(invalid(format!(
                "generated page {route:?} conflicts with the _redirects hosting file"
            )));
        }
        let occupied: std::collections::BTreeSet<_> = self
            .pages
            .keys()
            .flat_map(|route| {
                let stem = route.strip_suffix(".html").unwrap();
                [
                    route.clone(),
                    stem.into(),
                    clean_route(route).trim_end_matches('/').into(),
                    format!("{stem}.md"),
                ]
            })
            .collect();
        let mut decoded_sources = std::collections::BTreeSet::new();
        for (source, target) in &self.presentation.redirects {
            let decoded = decode_source(source)
                .map_err(|message| invalid(format!("redirect source {source:?}: {message}")))?;
            if !decoded_sources.insert(decoded.clone()) {
                return Err(invalid(format!(
                    "redirect source {source:?} is declared more than once"
                )));
            }
            let path = decoded.trim_start_matches('/').trim_end_matches('/');
            if matches!(
                path.split('/').next().unwrap_or(""),
                "assets" | "__diplodocus" | ".diplodocus-site"
            ) || matches!(
                path,
                "_redirects" | "sitemap.xml" | "robots.txt" | "documents.json"
            ) || occupied.contains(path)
            {
                return Err(invalid(format!(
                    "redirect source {source:?} conflicts with a generated page or reserved output"
                )));
            }
            if !self
                .pages
                .get(target.as_str())
                .is_some_and(|page| page.visible)
            {
                return Err(invalid(format!(
                    "redirect {source:?} must target a visible generated HTML page: {}",
                    target.as_str()
                )));
            }
            let destination = match self.presentation.canonical_url_style {
                CanonicalUrlStyle::File => target.as_str(),
                CanonicalUrlStyle::Clean => clean_route(target.as_str()),
            };
            // Static hosts match existing percent escapes literally, including their case.
            let mut source_url = url::Url::parse("https://diplodocus.invalid/").unwrap();
            source_url.set_path(source);
            let source = source_url.path().to_owned();
            let destination = format!("/{}", encode_url_path(destination));
            self.redirects.insert(source.clone(), destination.clone());
            if self.hosted_redirect(&source, &destination).len() > 1_000 {
                return Err(invalid(format!(
                    "redirect {source:?} exceeds the 1,000-character _redirects rule limit"
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn redirect_rules(&self) -> String {
        self.redirects
            .iter()
            .map(|(source, destination)| format!("{}\n", self.hosted_redirect(source, destination)))
            .collect()
    }

    fn hosted_redirect(&self, source: &str, destination: &str) -> String {
        let prefix = self
            .base_url
            .as_ref()
            .map_or("", |base| base.path().trim_end_matches('/'))
            .replace(':', "%3A")
            .replace('*', "%2A");
        format!("{prefix}{source} {prefix}{destination} 301")
    }
}

fn invalid(message: impl Into<String>) -> SiteError {
    SiteError::Presentation(message.into())
}

fn decode_source(source: &str) -> Result<String, &'static str> {
    let relative = source
        .strip_prefix('/')
        .ok_or("use a path starting with /, without the hosting prefix")?;
    if source.chars().any(|c| c.is_whitespace() || c.is_control())
        || source.as_bytes().iter().enumerate().any(|(index, byte)| {
            *byte == b'%'
                && !source
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
        })
    {
        return Err("URL paths must encode whitespace and use valid percent escapes");
    }
    if relative.is_empty() {
        return Ok("/".into());
    }
    let mut decoded = Vec::new();
    for segment in relative.strip_suffix('/').unwrap_or(relative).split('/') {
        let segment = percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| "URL paths must contain valid UTF-8")?;
        if matches!(segment.as_ref(), "" | "." | "..")
            || segment.chars().any(|c| c.is_control())
            || segment.contains(['/', '\\', ':', '*', '?', '#'])
        {
            return Err(
                "use an exact path without empty or dot segments, wildcards, query parameters, or fragments",
            );
        }
        decoded.push(segment.into_owned());
    }
    Ok(format!(
        "/{}{}",
        decoded.join("/"),
        if relative.ends_with('/') { "/" } else { "" }
    ))
}
