//! Canonical heading identities and snapshot-backed legacy fragment aliases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::html::{escape, plain};
use crate::configuration::FragmentAliasTarget;
use crate::ir::*;
use crate::site::{PageModel, Site, SiteError};
use crate::validation::{DocumentIdentity, document_anchors};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Title,
    Anchor(String),
    Heading(String),
}

#[derive(Default)]
pub(crate) struct PageAliases(BTreeMap<Target, Vec<String>>);

impl PageAliases {
    fn markers(&self, target: Target, markdown: bool) -> String {
        let mut output = String::new();
        for alias in self.0.get(&target).into_iter().flatten() {
            if markdown {
                writeln!(output, "<a id=\"{}\"></a>\n", escape(alias)).unwrap();
            } else {
                write!(
                    output,
                    "<span id=\"{}\" class=\"fragment-alias\"></span>",
                    escape(alias)
                )
                .unwrap();
            }
        }
        output
    }
}

impl Site<'_> {
    pub(crate) fn heading_aliases(
        &self,
        route: &str,
        anchor: Option<&str>,
        heading: &str,
        markdown: bool,
    ) -> String {
        self.fragment_aliases
            .get(route)
            .map_or_else(String::new, |aliases| {
                aliases.markers(target(anchor, heading), markdown)
            })
    }

    pub(crate) fn title_aliases(&self, route: &str, markdown: bool) -> String {
        self.fragment_aliases
            .get(route)
            .map_or_else(String::new, |aliases| {
                aliases.markers(Target::Title, markdown)
            })
    }
}

fn normalized_heading(heading: &str) -> String {
    heading.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn target(anchor: Option<&str>, heading: &str) -> Target {
    anchor.map_or_else(
        || Target::Heading(normalized_heading(heading)),
        |anchor| Target::Anchor(anchor.into()),
    )
}

/// Allocate canonical IDs without letting compatibility declarations change slugs.
pub(super) fn heading_id(
    identifiers: &mut BTreeSet<String>,
    prefix: Option<&str>,
    attributes: &Attributes,
    label: &str,
) -> String {
    let prefix = prefix
        .map(|prefix| format!("{prefix}-"))
        .unwrap_or_default();
    if let Some(identifier) = &attributes.identifier {
        return format!("{prefix}{}", identifier.value);
    }
    let slug = label
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if slug.is_empty() { "section" } else { &slug };
    let base = format!("{prefix}{base}");
    let mut identifier = base.clone();
    let mut suffix = 2;
    while !identifiers.insert(identifier.clone()) {
        identifier = format!("{base}-{suffix}");
        suffix += 1;
    }
    identifier
}

pub(crate) fn resolve(site: &Site<'_>) -> Result<BTreeMap<String, PageAliases>, SiteError> {
    let mut pages = BTreeMap::new();
    for (route, declarations) in &site.presentation.fragment_aliases {
        let invalid = |message: String| {
            SiteError::Presentation(format!(
                "presentation.fragment-aliases[{route:?}]: {message}"
            ))
        };
        let page = site
            .pages
            .get(route)
            .ok_or_else(|| invalid("key must name a generated site-relative HTML path".into()))?;
        let inventory = Inventory::page(site, route, page)?;
        let mut aliases = PageAliases::default();
        for (alias, selector) in declarations {
            if alias.is_empty() || alias.chars().any(char::is_control) {
                return Err(invalid(format!(
                    "alias {alias:?} must be nonempty and contain no control characters"
                )));
            }
            let matches: Vec<_> = inventory
                .targets
                .iter()
                .filter(|entry| match selector {
                    FragmentAliasTarget::Heading(label) => {
                        entry.heading.as_deref() == Some(normalized_heading(label).as_str())
                    }
                    FragmentAliasTarget::Anchor(anchor) => entry.anchor.as_ref() == Some(anchor),
                })
                .collect();
            if matches.len() != 1 {
                return Err(invalid(format!(
                    "alias {alias:?} has {} target {selector:?}; {}",
                    if matches.is_empty() {
                        "a missing"
                    } else {
                        "an ambiguous"
                    },
                    if matches.is_empty() {
                        "select an existing heading or API anchor"
                    } else {
                        "select a unique canonical anchor"
                    },
                )));
            }
            let entry = matches[0];
            if let Some((id, _)) = inventory.occupied.iter().find(|(_, count)| **count > 1) {
                return Err(invalid(format!(
                    "duplicate ID {id:?} on the destination page"
                )));
            }
            if inventory.occupied.contains_key(alias) {
                if entry.anchor.as_ref() == Some(alias) {
                    continue;
                }
                return Err(invalid(format!(
                    "alias {alias:?} conflicts with an existing ID on the destination page"
                )));
            }
            aliases
                .0
                .entry(entry.target.clone())
                .or_default()
                .push(alias.clone());
        }
        pages.insert(route.clone(), aliases);
    }
    Ok(pages)
}

pub(super) fn validate_html(site: &Site<'_>, route: &str, html: &str) -> Result<(), SiteError> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{NodeData, RcDom};

    let Some(declarations) = site.presentation.fragment_aliases.get(route) else {
        return Ok(());
    };
    if declarations.is_empty() {
        return Ok(());
    }
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut nodes = vec![dom.document.clone()];
    let mut ids = BTreeMap::<String, usize>::new();
    while let Some(node) = nodes.pop() {
        if let NodeData::Element { attrs, .. } = &node.data {
            for attribute in attrs
                .borrow()
                .iter()
                .filter(|a| a.name.local.as_ref() == "id")
            {
                *ids.entry(attribute.value.to_string()).or_default() += 1;
            }
        }
        nodes.extend(node.children.borrow().iter().cloned());
    }
    for alias in declarations.keys() {
        if ids.get(alias) != Some(&1) {
            return Err(SiteError::Presentation(format!(
                "presentation.fragment-aliases[{route:?}]: alias {alias:?} must occur exactly once in rendered HTML (found {})",
                ids.get(alias).copied().unwrap_or_default()
            )));
        }
    }
    Ok(())
}

struct Entry {
    target: Target,
    anchor: Option<String>,
    heading: Option<String>,
}

#[derive(Default)]
struct Inventory {
    targets: Vec<Entry>,
    occupied: BTreeMap<String, usize>,
}

struct DocumentHeadings<'a> {
    page: &'a PageModel<'a>,
    identifiers: BTreeSet<String>,
    navigation: bool,
    offset: usize,
}

impl Inventory {
    fn id(&mut self, id: &str) {
        *self.occupied.entry(id.into()).or_default() += 1;
    }

    fn heading(&mut self, anchor: Option<String>, heading: &str) {
        if let Some(anchor) = &anchor {
            self.id(anchor);
        }
        self.targets.push(Entry {
            target: target(anchor.as_deref(), heading),
            anchor,
            heading: Some(normalized_heading(heading)),
        });
    }

    fn api(&mut self, anchor: &str) {
        self.id(anchor);
        self.targets.push(Entry {
            target: Target::Anchor(anchor.into()),
            anchor: Some(anchor.into()),
            heading: None,
        });
    }

    fn page(site: &Site<'_>, route: &str, page: &PageModel<'_>) -> Result<Self, SiteError> {
        let mut inventory = Self::default();
        for id in [
            "main",
            "search",
            "search-results",
            "desktop-sections",
            "mobile-sections",
        ] {
            inventory.id(id);
        }
        let mut headings = DocumentHeadings {
            page,
            identifiers: page.resolved.map(|d| d.anchors.clone()).unwrap_or_default(),
            navigation: true,
            offset: usize::from(page.item.is_some_and(|item| {
                super::rust::data(item).is_some() || super::cpp::data(item).is_some()
            })),
        };
        headings
            .identifiers
            .extend(["main", "search", "search-results"].map(str::to_owned));
        if let Some(class) = &page.python_class {
            headings.identifiers.extend(
                class
                    .members
                    .iter()
                    .filter(|m| !m.inherited)
                    .map(|m| m.anchor.clone()),
            );
            if !class.constructors.is_empty() {
                headings.identifiers.insert("constructor".into());
            }
        }
        if let Some(api) = &page.api_page {
            headings
                .identifiers
                .extend(api.sections.iter().map(|s| s.anchor.clone()));
            headings
                .identifiers
                .extend(api.members().map(|m| m.anchor.clone()));
        }
        let home = site.home(route);
        let authored_title = page.item.is_none()
            && page.concept.is_none()
            && page.document.is_some_and(|document| {
                matches!(document.document.blocks.first(), Some(Block::Heading {level:1,inlines,..})
                    if home.is_some() || plain(inlines) == page.title)
            });
        if home.is_some() || !authored_title {
            let anchor = home.and(page.document).and_then(|document| {
                match document.document.blocks.first() {
                    Some(Block::Heading {
                        level: 1,
                        attributes,
                        ..
                    }) => attributes.identifier.as_ref().map(|id| id.value.clone()),
                    _ => None,
                }
            });
            if let Some(anchor) = &anchor {
                inventory.id(anchor);
            }
            let label = if let Some(home) = home {
                home.tagline.clone()
            } else if let Some(inlines) = page.item.and_then(super::r_topics::title) {
                plain(inlines)
            } else if let Some(item) = page.item.filter(|item| {
                item.kind == ItemKind::Method
                    && matches!(&item.language_data, Some(ItemLanguageData::Julia(_)))
            }) {
                item.qualified_name.clone()
            } else {
                page.title.clone()
            };
            inventory.targets.push(Entry {
                target: Target::Title,
                anchor,
                heading: Some(normalized_heading(&label)),
            });
        }
        if let Some(home) = home {
            for feature in &home.features {
                inventory.heading(None, &feature.title);
            }
        }
        if authored_title && home.is_none() {
            let document = page.document.expect("authored title");
            inventory.blocks(site, &document.document.blocks[..1], &mut headings)?;
        }
        if let Some(item) = page.item {
            inventory.overloads(item, &mut headings);
        }
        if let Some(class) = &page.python_class
            && !class.constructors.is_empty()
        {
            inventory.api("constructor");
        }
        if let Some(document) = page.document {
            let start =
                usize::from(authored_title || page.item.and_then(super::r_topics::title).is_some());
            inventory.blocks(site, &document.document.blocks[start..], &mut headings)?;
        }
        for section in &page.julia_sections {
            inventory.heading(Some(section.anchor.into()), section.title);
            for member in &section.members {
                let model = &site.embedded_pages[member];
                let item = model.item.ok_or(SiteError::Evidence)?;
                inventory.heading(
                    Some(site.overload_anchors[member].clone()),
                    &super::julia::member_title(item),
                );
                inventory.document(site, model, 2, true)?;
            }
        }
        if let Some(class) = &page.python_class {
            for member in class.members.iter().filter(|m| !m.inherited) {
                inventory.heading(Some(member.anchor.clone()), &member.item.name);
                inventory.overloads(member.item, &mut headings);
                if let Some(document) = &member.item.documentation {
                    inventory.blocks(
                        site,
                        &document.document.blocks,
                        &mut DocumentHeadings {
                            page,
                            identifiers: BTreeSet::new(),
                            navigation: false,
                            offset: 1,
                        },
                    )?;
                }
            }
        }
        if let Some(api) = &page.api_page {
            for section in &api.sections {
                inventory.heading(Some(section.anchor.clone()), &section.title);
                if let Some(documentation) = &section.documentation {
                    inventory.document(
                        site,
                        &site.embedded_pages[&documentation.reference],
                        2,
                        true,
                    )?;
                }
                for member in &section.members {
                    inventory.member(site, member, 3)?;
                }
            }
        }
        if let Some(concept) = page.concept {
            inventory.concept(concept);
        }
        if let Some(DocumentIdentity::Item {item}) = site.routes.iter().find_map(|(identity,path)| {
            let overload = matches!(identity, DocumentIdentity::Item {item} if site.overload_anchors.contains_key(item));
            (path == route && !overload).then_some(identity)
        }) {
            for concept in site.workspace.concepts.values().filter(|c| c.members.contains(item)) {
                inventory.concept(concept);
            }
        }
        Ok(inventory)
    }

    fn concept(&mut self, concept: &Concept) {
        self.heading(
            None,
            if concept.kind == crate::configuration::ConceptKind::Equivalent {
                "Same API in"
            } else {
                "Related API"
            },
        );
    }

    fn overloads(&mut self, item: &Item, headings: &mut DocumentHeadings<'_>) {
        if let Some(ItemLanguageData::Python(data)) = &item.language_data
            && let PythonDeclaration::Callable {
                role: PythonCallableRole::Family { overloads },
                ..
            } = &data.declaration
        {
            for overload in overloads.iter().take(item.signatures.len()) {
                let anchor = crate::site::python_overload_anchor(&overload.item);
                headings.identifiers.insert(anchor.clone());
                self.api(&anchor);
            }
        }
    }

    fn member(
        &mut self,
        site: &Site<'_>,
        member: &super::members::ApiMember<'_>,
        level: usize,
    ) -> Result<(), SiteError> {
        self.heading(Some(member.anchor.clone()), &member.item.name);
        self.document(site, &site.embedded_pages[&member.reference], level, true)?;
        for child in &member.children {
            self.member(site, child, level + 1)?;
        }
        Ok(())
    }

    fn document(
        &mut self,
        site: &Site<'_>,
        page: &PageModel<'_>,
        offset: usize,
        navigation: bool,
    ) -> Result<(), SiteError> {
        if let Some(document) = page.document {
            self.blocks(
                site,
                &document.document.blocks,
                &mut DocumentHeadings {
                    page,
                    identifiers: BTreeSet::new(),
                    navigation,
                    offset,
                },
            )?;
        }
        Ok(())
    }

    fn blocks(
        &mut self,
        site: &Site<'_>,
        blocks: &[Block],
        headings: &mut DocumentHeadings<'_>,
    ) -> Result<(), SiteError> {
        for block in blocks {
            match block {
                Block::Heading {
                    level,
                    attributes,
                    inlines,
                    ..
                } => {
                    if headings.navigation {
                        let label = plain(inlines);
                        let anchor = if *level + headings.offset > 1 {
                            Some(heading_id(
                                &mut headings.identifiers,
                                headings.page.anchor_prefix.as_deref(),
                                attributes,
                                &label,
                            ))
                        } else {
                            attributes.identifier.as_ref().map(|id| id.value.clone())
                        };
                        self.heading(anchor, &label);
                    }
                    self.inlines(inlines);
                }
                Block::ApiDocumentation {
                    item,
                    selection,
                    identifier,
                    ..
                } => {
                    self.heading(
                        Some(identifier.clone()),
                        &site.workspace.packages[&item.package].items[&item.item].qualified_name,
                    );
                    for item in selection {
                        self.document(site, site.api_page(item)?.1, 0, false)?;
                    }
                }
                Block::Component {
                    attributes, blocks, ..
                }
                | Block::Callout {
                    attributes, blocks, ..
                } => {
                    self.attributes(attributes);
                    self.blocks(site, blocks, headings)?;
                }
                Block::Tabset {
                    attributes, blocks, ..
                } => {
                    self.attributes(attributes);
                    let level = blocks
                        .iter()
                        .filter_map(|block| match block {
                            Block::Heading { level, .. } => Some(*level),
                            _ => None,
                        })
                        .min();
                    let mut start = 0;
                    for (index, block) in blocks.iter().enumerate() {
                        if let Block::Heading {
                            level: heading_level,
                            attributes,
                            inlines,
                            ..
                        } = block
                            && Some(*heading_level) == level
                        {
                            self.blocks(site, &blocks[start..index], headings)?;
                            let label = plain(inlines);
                            let anchor = heading_id(
                                &mut headings.identifiers,
                                headings.page.anchor_prefix.as_deref(),
                                attributes,
                                &label,
                            );
                            self.heading(Some(anchor), &label);
                            self.inlines(inlines);
                            start = index + 1;
                        }
                    }
                    self.blocks(site, &blocks[start..], headings)?;
                }
                Block::Container {
                    kind,
                    identifier,
                    blocks,
                    ..
                } => {
                    if let Some(id) = identifier {
                        self.id(id);
                    }
                    let navigation = headings.navigation;
                    if matches!(
                        kind,
                        DocumentContainerKind::GeneratedChunk
                            | DocumentContainerKind::GeneratedOutput
                    ) {
                        headings.navigation = false;
                    }
                    self.blocks(site, blocks, headings)?;
                    headings.navigation = navigation;
                }
                Block::BlockQuote { blocks, .. } => self.blocks(site, blocks, headings)?,
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(site, &item.blocks, headings)?;
                    }
                }
                Block::Table { caption, rows, .. } => {
                    self.inlines(caption);
                    for row in rows {
                        for cell in &row.cells {
                            self.blocks(site, &cell.blocks, headings)?;
                        }
                    }
                }
                Block::Paragraph { inlines, .. } => self.inlines(inlines),
                Block::CodeCell(_) => {
                    for anchor in document_anchors(std::slice::from_ref(block)) {
                        self.id(&anchor);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn attributes(&mut self, attributes: &Attributes) {
        if let Some(id) = &attributes.identifier {
            self.id(&id.value);
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Link {
                    attributes,
                    inlines,
                    ..
                } => {
                    self.attributes(attributes);
                    self.inlines(inlines);
                }
                Inline::Image {
                    attributes, alt, ..
                } => {
                    self.attributes(attributes);
                    self.inlines(alt);
                }
                Inline::Emphasis { inlines, .. }
                | Inline::Strong { inlines, .. }
                | Inline::Styled { inlines, .. }
                | Inline::Citation { inlines, .. }
                | Inline::Strikeout { inlines, .. } => self.inlines(inlines),
                _ => {}
            }
        }
    }
}
