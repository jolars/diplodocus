use super::images::{ImageRole, Images, logo_variants};
use crate::execution::output_safety::{HtmlAttribute, HtmlNode};
use crate::execution::{ValidatedPage, ValidatedRepresentationRef};
use crate::ir::{Block, Inline};
use crate::site::{Site, SiteError};
use crate::validation::{ReferenceKind, ReferenceTarget, ResolvedDocument};

pub(super) fn prepare(site: &Site<'_>, images: &Images) -> Result<(), SiteError> {
    for (route, page) in &site.pages {
        let home = site.home(route).is_some();
        if home && site.logo.is_some() {
            for (image, _) in logo_variants(site.logo, site.logo_dark) {
                images.prepare(site, &image.fingerprint.value, ImageRole::Hero)?;
            }
        }
        let mut visitor = Visitor {
            site,
            images,
            resolved: page.resolved,
            executed: page.executed,
            cell: 0,
        };
        if let Some(document) = page.document {
            let mut blocks = document.document.blocks.as_slice();
            if home
                && page.item.is_none()
                && page.concept.is_none()
                && matches!(blocks.first(), Some(Block::Heading { level: 1, .. }))
            {
                blocks = &blocks[1..];
            }
            visitor.blocks(blocks)?;
        }
        for member in page
            .julia_sections
            .iter()
            .flat_map(|section| &section.members)
        {
            let embedded = &site.embedded_pages[member];
            if let Some(document) = embedded.document {
                visitor.document(
                    &document.document.blocks,
                    embedded.resolved,
                    embedded.executed,
                )?;
            }
        }
        if let Some(class) = &page.python_class {
            for member in class.members.iter().filter(|member| !member.inherited) {
                if let Some(document) = &member.item.documentation {
                    visitor.document(&document.document.blocks, member.resolved, page.executed)?;
                }
            }
        }
        if let Some(api) = &page.api_page {
            for member in api
                .sections
                .iter()
                .flat_map(|section| section.documentation.iter().chain(&section.members))
            {
                visitor.member(member)?;
            }
        }
    }
    Ok(())
}

// Walk the image-bearing parts of fragment bodies without constructing markup.
// Each embedded document starts with its own reference context and cell ordinal.
struct Visitor<'a, 'site> {
    site: &'a Site<'site>,
    images: &'a Images,
    resolved: Option<&'a ResolvedDocument>,
    executed: Option<&'a ValidatedPage>,
    cell: usize,
}

impl Visitor<'_, '_> {
    fn member(&self, member: &super::members::ApiMember<'_>) -> Result<(), SiteError> {
        if let Some(document) = &member.item.documentation {
            let page = &self.site.embedded_pages[&member.reference];
            self.document(&document.document.blocks, member.resolved, page.executed)?;
        }
        for child in &member.children {
            self.member(child)?;
        }
        Ok(())
    }

    fn document(
        &self,
        blocks: &[Block],
        resolved: Option<&ResolvedDocument>,
        executed: Option<&ValidatedPage>,
    ) -> Result<(), SiteError> {
        Visitor {
            site: self.site,
            images: self.images,
            resolved,
            executed,
            cell: 0,
        }
        .blocks(blocks)
    }

    fn image(&self, digest: &str) -> Result<(), SiteError> {
        self.images.prepare(self.site, digest, ImageRole::Content)
    }

    fn blocks(&mut self, blocks: &[Block]) -> Result<(), SiteError> {
        for block in blocks {
            match block {
                Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. } => {
                    self.inlines(inlines)?
                }
                Block::BlockQuote { blocks, .. }
                | Block::Container { blocks, .. }
                | Block::Component { blocks, .. }
                | Block::Tabset { blocks, .. }
                | Block::Callout { blocks, .. } => self.blocks(blocks)?,
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&item.blocks)?;
                    }
                }
                Block::Table { caption, rows, .. } => {
                    self.inlines(caption)?;
                    for cell in rows.iter().flat_map(|row| &row.cells) {
                        self.blocks(&cell.blocks)?;
                    }
                }
                Block::CodeCell(_) => self.code_cell()?,
                Block::ApiDocumentation { selection, .. } => {
                    for item in selection {
                        let (_, page) = self.site.api_page(item)?;
                        if let Some(document) = page.document {
                            self.document(&document.document.blocks, page.resolved, page.executed)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn inlines(&self, inlines: &[Inline]) -> Result<(), SiteError> {
        for inline in inlines {
            match inline {
                Inline::Image { target, .. } => {
                    let reference = self
                        .resolved
                        .and_then(|document| {
                            document.references.iter().find(|reference| {
                                reference.kind == ReferenceKind::Image
                                    && reference.spelling == *target
                            })
                        })
                        .ok_or(SiteError::Evidence)?;
                    if let ReferenceTarget::Asset { asset, .. } = &reference.target {
                        self.image(&asset.fingerprint.value)?;
                    }
                }
                Inline::Emphasis { inlines, .. }
                | Inline::Strong { inlines, .. }
                | Inline::Styled { inlines, .. }
                | Inline::Citation { inlines, .. }
                | Inline::Strikeout { inlines, .. }
                | Inline::Link { inlines, .. } => self.inlines(inlines)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn code_cell(&mut self) -> Result<(), SiteError> {
        let ordinal = self.cell;
        self.cell += 1;
        let Some(page) = self.executed else {
            return Ok(());
        };
        let record = page.record();
        let cell = record.cells.get(ordinal).ok_or(SiteError::Evidence)?;
        let view = super::present_cell(&record.page, cell).map_err(|_| SiteError::Evidence)?;
        for output in view.outputs {
            match page.representation(ordinal, output.slot, 0) {
                Some(ValidatedRepresentationRef::Asset(asset)) => {
                    self.image(&asset.reference.fingerprint.value)?
                }
                Some(ValidatedRepresentationRef::Html(html)) => self.html_nodes(html.nodes())?,
                Some(ValidatedRepresentationRef::Markdown(markdown)) => {
                    self.blocks(markdown.blocks())?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn html_nodes(&self, nodes: &[HtmlNode]) -> Result<(), SiteError> {
        for node in nodes {
            if let HtmlNode::Element {
                name,
                attributes,
                children,
            } = node
            {
                if name == "img"
                    && let Some(HtmlAttribute::Asset(asset)) = attributes.get("src")
                {
                    self.image(&asset.reference.fingerprint.value)?;
                }
                self.html_nodes(children)?;
            }
        }
        Ok(())
    }
}
