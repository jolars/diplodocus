//! Validate explicit navigation against the final generated routes.

use crate::configuration::{SidebarConfiguration, SidebarItem};
use crate::diagnostics::DiagnosticPath;

use super::{Site, SiteError};

impl Site<'_> {
    pub(super) fn validate_sidebar(&self) -> Result<(), SiteError> {
        let Some(sidebar) = &self.presentation.sidebar else {
            return Ok(());
        };
        match sidebar {
            SidebarConfiguration::Single(items) => {
                self.validate_sidebar_items(items, "presentation.sidebar", 1)
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
                    self.validate_sidebar_items(
                        items,
                        &format!("presentation.sidebar[{prefix}]"),
                        1,
                    )?;
                }
                Ok(())
            }
        }
    }

    fn validate_sidebar_items(
        &self,
        items: &[SidebarItem],
        field: &str,
        depth: usize,
    ) -> Result<(), SiteError> {
        if depth > 6 && !items.is_empty() {
            return Err(SiteError::Presentation(format!(
                "sidebar items may nest at most six levels: {field}"
            )));
        }
        for (index, item) in items.iter().enumerate() {
            let field = format!("{field}[{index}]");
            if item.text.trim().is_empty()
                || (item.link.is_none() && item.items.is_empty())
                || (item.collapsed.is_some() && item.items.is_empty())
            {
                return Err(SiteError::Presentation(format!(
                    "sidebar items need text and a link or children; collapsed requires children: {field}"
                )));
            }
            if let Some(link) = &item.link
                && !self
                    .pages
                    .get(link.as_str())
                    .is_some_and(|page| page.visible)
            {
                return Err(SiteError::Presentation(format!(
                    "sidebar link must name a visible generated page: {field}.link: {}",
                    link.as_str()
                )));
            }
            self.validate_sidebar_items(&item.items, &format!("{field}.items"), depth + 1)?;
        }
        Ok(())
    }
}
