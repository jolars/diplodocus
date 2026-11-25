//! Resolve explicit navigation to the final generated routes.

use crate::configuration::{SidebarConfiguration, SidebarItem};
use crate::diagnostics::DiagnosticPath;
use crate::documents::document_title;

use super::{Site, SiteError};

impl Site<'_> {
    pub(super) fn resolve_sidebar(&self) -> Result<Option<SidebarConfiguration>, SiteError> {
        let Some(mut sidebar) = self.presentation.sidebar.clone() else {
            return Ok(None);
        };
        match &mut sidebar {
            SidebarConfiguration::Single(items) => {
                self.resolve_sidebar_items(items, "presentation.sidebar", 1)?;
            }
            SidebarConfiguration::Multiple(sidebars) => {
                for (prefix, items) in sidebars {
                    if prefix != "/"
                        && (!prefix.starts_with('/')
                            || !prefix.ends_with('/')
                            || DiagnosticPath::try_from(&prefix[1..prefix.len() - 1]).is_err())
                    {
                        return Err(SiteError::Presentation(format!(
                            "sidebar prefixes must be site-root directories such as /guide/: {prefix}"
                        )));
                    }
                    self.resolve_sidebar_items(
                        items,
                        &format!("presentation.sidebar[{prefix}]"),
                        1,
                    )?;
                }
            }
        }
        Ok(Some(sidebar))
    }

    fn resolve_sidebar_items(
        &self,
        items: &mut [SidebarItem],
        field: &str,
        depth: usize,
    ) -> Result<(), SiteError> {
        if depth > 6 && !items.is_empty() {
            return Err(SiteError::Presentation(format!(
                "sidebar items may nest at most six levels: {field}"
            )));
        }
        for (index, item) in items.iter_mut().enumerate() {
            let field = format!("{field}[{index}]");
            if item
                .text
                .as_deref()
                .is_some_and(|text| text.trim().is_empty())
                || (item.link.is_none() && item.items.is_empty())
                || (item.collapsed.is_some() && item.items.is_empty())
            {
                return Err(SiteError::Presentation(format!(
                    "sidebar items need text and a link or children; collapsed requires children: {field}"
                )));
            }
            if let Some(link) = &mut item.link {
                let destination = self.sidebar_destination(link, &field)?;
                let Some((route, page)) = destination
                    .and_then(|route| self.pages.get_key_value(route))
                    .filter(|(_, page)| page.visible)
                else {
                    return Err(SiteError::Presentation(format!(
                        "sidebar link must name a visible generated page or its source file: {field}.link: {}",
                        link.as_str()
                    )));
                };
                if item.text.is_none()
                    && (page.title.trim().is_empty()
                        || (page
                            .resolved
                            .is_some_and(|document| document.collection_path.is_some())
                            && page
                                .document
                                .and_then(|document| document_title(&document.document))
                                .is_none_or(|title| title.trim().is_empty())))
                {
                    return Err(SiteError::Presentation(format!(
                        "sidebar items need text when the linked page has no title: {field}"
                    )));
                }
                *link =
                    DiagnosticPath::try_from(route.as_str()).map_err(|_| SiteError::Evidence)?;
            } else if item.text.is_none() {
                return Err(SiteError::Presentation(format!(
                    "sidebar groups without a link need text: {field}"
                )));
            }
            self.resolve_sidebar_items(&mut item.items, &format!("{field}.items"), depth + 1)?;
        }
        Ok(())
    }

    fn sidebar_destination<'a>(
        &'a self,
        link: &'a DiagnosticPath,
        field: &str,
    ) -> Result<Option<&'a str>, SiteError> {
        let path = link.as_str();
        if !matches!(path.rsplit_once('.'), Some((_, "md" | "qmd"))) {
            return Ok(Some(path));
        }

        let mut routes = self.pages.iter().filter(|(_, page)| {
            page.visible
                && page.item.is_none()
                && page.concept.is_none()
                && page
                    .document
                    .and_then(|document| document.source_location.as_ref())
                    .is_some_and(|source| source.path == *link)
        });
        let route = routes.next().map(|(route, _)| route.as_str());
        if routes.next().is_some() {
            return Err(SiteError::Presentation(format!(
                "sidebar source path matches multiple pages: {field}.link: {path}; use a generated HTML route"
            )));
        }
        Ok(route)
    }
}
