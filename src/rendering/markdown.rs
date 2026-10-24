//! Markdown serialization of the same snapshot-backed API presentation as HTML.

use std::collections::BTreeSet;
use std::fmt::Write;

use crate::execution::{
    ValidatedRepresentationRef,
    output_safety::{HtmlAttribute, HtmlNode},
};
use crate::ir::*;
use crate::site::{PageModel, Site, SiteError};
use crate::validation::{DocumentIdentity, ReferenceKind, ReferenceTarget, ResolvedDocument};

pub(super) fn page(
    site: &Site<'_>,
    route: &str,
    page: &PageModel<'_>,
) -> Result<String, SiteError> {
    let mut renderer = Markdown {
        site,
        route,
        page,
        resolved: page.resolved,
        identifiers: page.resolved.map(|d| d.anchors.clone()).unwrap_or_default(),
        headings: true,
        heading_offset: usize::from(
            page.item
                .is_some_and(|item| super::rust::data(item).is_some()),
        ),
        cell: 0,
    };
    renderer
        .identifiers
        .extend(["main", "search", "search-results"].map(str::to_owned));
    if let Some(class) = &page.python_class {
        renderer.identifiers.extend(
            class
                .members
                .iter()
                .filter(|member| !member.inherited)
                .map(|member| member.anchor.clone()),
        );
        if !class.constructors.is_empty() {
            renderer.identifiers.insert("constructor".into());
        }
    }
    if let Some(rust) = &page.rust_page {
        renderer
            .identifiers
            .extend(rust.sections.iter().map(|section| section.anchor.clone()));
        renderer
            .identifiers
            .extend(rust.members().map(|member| member.anchor.clone()));
    }
    let mut output = String::new();
    let package_overview = super::package_metadata::is_overview(site, route, page.owner.as_deref());
    let authored_title = page.item.is_none()
        && page.concept.is_none()
        && page.document.is_some_and(|d| {
            matches!(d.document.blocks.first(),
            Some(Block::Heading {level:1,inlines,..}) if super::html::plain(inlines)==page.title)
        });
    if !authored_title {
        writeln!(output, "# {}\n", escape(&page.title)).unwrap();
    } else if package_overview {
        output.push_str(
            &renderer.blocks(&page.document.expect("authored title").document.blocks[..1])?,
        );
    }
    if let Some(home) = site.home(route) {
        writeln!(
            output,
            "{}\n\n{}\n",
            escape(&home.tagline),
            escape(&home.description),
        )
        .unwrap();
        for action in &home.actions {
            writeln!(
                output,
                "- [{}]({})",
                escape(&action.label),
                link_url(&site.url(route, action.path.as_str())),
            )
            .unwrap();
        }
        output.push('\n');
        for feature in &home.features {
            writeln!(
                output,
                "## {}\n\n{}\n\n[Read more]({})\n",
                escape(&feature.title),
                escape(&feature.description),
                link_url(&site.url(route, feature.path.as_str())),
            )
            .unwrap();
        }
    }
    if let Some(owner) = &page.owner
        && package_overview
    {
        output.push_str(&super::package_metadata::markdown(site, route, owner));
    } else if let Some(owner) = &page.owner {
        let package = &site.workspace.packages[owner];
        writeln!(
            output,
            "{}{}\n",
            escape(&package.name),
            package
                .version
                .as_ref()
                .map(|v| format!(" · {}", escape(v)))
                .unwrap_or_default()
        )
        .unwrap();
    }
    if let Some(item) = page.item {
        output.push_str(&sources(site, item));
        let ecosystem = page
            .owner
            .as_ref()
            .map(|owner| site.workspace.packages[owner].ecosystem.as_str())
            .unwrap_or("");
        let name = if ecosystem == "python" {
            page.title.rsplit('.').next().unwrap_or(&item.name)
        } else {
            &item.name
        };
        let overloads = match &item.language_data {
            Some(ItemLanguageData::Python(data)) => match &data.declaration {
                PythonDeclaration::Callable {
                    role: PythonCallableRole::Family { overloads },
                    ..
                } => overloads.as_slice(),
                _ => &[],
            },
            _ => &[],
        };
        for (index, signature) in item.signatures.iter().enumerate() {
            if let Some(member) = overloads.get(index) {
                anchor(
                    &mut output,
                    &crate::site::python_overload_anchor(&member.item),
                );
            }
            output.push_str(&fence(
                &super::signatures::signature_for_site(site, name, &signature.signature, ecosystem),
                ecosystem,
            ));
        }
    }
    if let Some(class) = &page.python_class {
        if !class.bases.is_empty() {
            writeln!(
                output,
                "Bases: {}\n",
                class
                    .bases
                    .iter()
                    .map(|base| code(&super::signatures::expression_for_site(
                        site, base, "python"
                    )))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
        if !class.constructors.is_empty() {
            anchor(&mut output, "constructor");
        }
        for signature in &class.constructors {
            let name = page.title.rsplit('.').next().unwrap_or(&page.title);
            output.push_str(&fence(
                &super::signatures::signature_for_site(site, name, signature, "python"),
                "python",
            ));
        }
        if let Some(owner) = &class.constructor_owner
            && page.item.is_some_and(|item| {
                site.workspace.packages[&owner.package].items[&owner.item].qualified_name
                    != item.qualified_name
            })
        {
            let target = site.target(
                route,
                &ReferenceTarget::Item {
                    item: owner.clone(),
                },
            )?;
            writeln!(
                output,
                "Constructor inherited from [{}]({}).\n",
                code(&site.workspace.packages[&owner.package].items[&owner.item].name),
                link_url(&target)
            )
            .unwrap();
        }
    }
    if let Some(document) = page.document {
        let start = usize::from(package_overview && authored_title);
        output.push_str(&renderer.blocks(&document.document.blocks[start..])?);
    }
    if let Some(item) = page.item {
        let members = super::html::julia_members(item);
        if !members.is_empty() {
            anchor(&mut output, "methods");
            output.push_str("## Methods\n\n");
        }
        for member in members {
            let method_page = &site.embedded_pages[member];
            let method = method_page.item.expect("embedded Julia method");
            anchor(&mut output, &site.overload_anchors[member]);
            for signature in &method.signatures {
                writeln!(
                    output,
                    "### {}\n",
                    code(&super::signatures::signature_for_site(
                        site,
                        &method.name,
                        &signature.signature,
                        "julia"
                    ))
                )
                .unwrap();
            }
            output.push_str(&sources(site, method));
            if let Some(document) = method_page.document {
                let mut method_renderer = Markdown {
                    site,
                    route,
                    page: method_page,
                    resolved: method_page.resolved,
                    identifiers: BTreeSet::new(),
                    headings: true,
                    heading_offset: 2,
                    cell: 0,
                };
                output.push_str(&method_renderer.blocks(&document.document.blocks)?);
            }
        }
    }
    if let Some(class) = &page.python_class {
        for member in &class.members {
            if member.inherited {
                let destination = site.target(
                    route,
                    &ReferenceTarget::Item {
                        item: member.reference.clone(),
                    },
                )?;
                writeln!(
                    output,
                    "- [{}]({}) (inherited)\n",
                    code(&member.item.name),
                    link_url(&destination)
                )
                .unwrap();
                continue;
            }
            anchor(&mut output, &member.anchor);
            writeln!(output, "## {}\n", code(&member.item.name)).unwrap();
            let overloads = match &member.item.language_data {
                Some(ItemLanguageData::Python(data)) => match &data.declaration {
                    PythonDeclaration::Callable {
                        role: PythonCallableRole::Family { overloads },
                        ..
                    } => overloads.as_slice(),
                    _ => &[],
                },
                _ => &[],
            };
            for (index, signature) in super::python::member_signatures(member.item)
                .iter()
                .enumerate()
            {
                if let Some(overload) = overloads.get(index) {
                    anchor(
                        &mut output,
                        &crate::site::python_overload_anchor(&overload.item),
                    );
                }
                output.push_str(&fence(
                    &super::signatures::signature_for_site(
                        site,
                        &member.item.name,
                        signature,
                        "python",
                    ),
                    "python",
                ));
            }
            output.push_str(&sources(site, member.item));
            if let Some(document) = &member.item.documentation {
                let mut member_renderer = Markdown {
                    site,
                    route,
                    page,
                    resolved: member.resolved,
                    identifiers: BTreeSet::new(),
                    headings: false,
                    heading_offset: 1,
                    cell: 0,
                };
                output.push_str(&member_renderer.blocks(&document.document.blocks)?);
            }
        }
    }
    if let Some(rust) = &page.rust_page {
        for section in &rust.sections {
            anchor(&mut output, &section.anchor);
            let title = if let Some(target) = section
                .implementation
                .and_then(|implementation| implementation.trait_target.as_ref())
            {
                format!(
                    "[{}]({})",
                    escape(&section.title),
                    link_url(&site.target(
                        route,
                        &ReferenceTarget::Item {
                            item: target.clone()
                        }
                    )?)
                )
            } else {
                escape(&section.title)
            };
            writeln!(output, "## {title}\n").unwrap();
            if let Some(implementation) = section.implementation {
                output.push_str(&source_location(
                    site,
                    implementation.source_location.as_ref(),
                ));
                output.push_str(&fence(
                    &super::signatures::expression_for_site(
                        site,
                        &implementation.signature,
                        "rust",
                    ),
                    "rust",
                ));
            }
            if let Some(documentation) = &section.documentation {
                output.push_str(&rust_document(site, route, documentation, 2)?);
            }
            for member in &section.members {
                output.push_str(&rust_member(site, route, member, 3)?);
            }
        }
    }
    if let Some(concept) = page.concept {
        output.push_str(&concept_links(site, route, concept)?);
    }
    if let Some(identity) = site.routes.iter().find_map(|(identity, path)| {
        let overload = matches!(identity, DocumentIdentity::Item { item }
            if site.overload_anchors.contains_key(item));
        (path == route && !overload).then_some(identity)
    }) && let DocumentIdentity::Item { item } = identity
    {
        for concept in site
            .workspace
            .concepts
            .values()
            .filter(|concept| concept.members.contains(item))
        {
            output.push_str(&concept_links(site, route, concept)?);
        }
    }
    if page.document.is_none()
        && page.item.is_none()
        && page.concept.is_none()
        && site.home(route).is_none()
    {
        for (path, candidate) in &site.pages {
            if candidate.visible
                && path != route
                && (page.owner.is_none() || candidate.owner == page.owner)
            {
                writeln!(
                    output,
                    "- [{}]({})",
                    escape(&candidate.title),
                    link_url(&site.url(route, path))
                )
                .unwrap();
            }
        }
    }
    Ok(format!("{}\n", output.trim_end()))
}

fn rust_member(
    site: &Site<'_>,
    route: &str,
    member: &super::rust::RustMember<'_>,
    level: usize,
) -> Result<String, SiteError> {
    let mut output = String::new();
    anchor(&mut output, &member.anchor);
    writeln!(
        output,
        "{} {}\n",
        "#".repeat(level),
        code(&member.item.name)
    )
    .unwrap();
    for signature in &member.item.signatures {
        output.push_str(&fence(
            &super::signatures::signature_for_site(
                site,
                &member.item.name,
                &signature.signature,
                "rust",
            ),
            "rust",
        ));
    }
    output.push_str(&sources(site, member.item));
    output.push_str(&rust_document(site, route, member, level)?);
    for child in &member.children {
        output.push_str(&rust_member(site, route, child, level + 1)?);
    }
    Ok(output)
}

fn rust_document(
    site: &Site<'_>,
    route: &str,
    member: &super::rust::RustMember<'_>,
    level: usize,
) -> Result<String, SiteError> {
    let mut output = String::new();
    if let Some(document) = &member.item.documentation {
        let page = &site.embedded_pages[&member.reference];
        let mut renderer = Markdown {
            site,
            route,
            page,
            resolved: member.resolved,
            identifiers: BTreeSet::new(),
            headings: true,
            heading_offset: level,
            cell: 0,
        };
        output.push_str(&renderer.blocks(&document.document.blocks)?);
    }
    Ok(output)
}

fn concept_links(site: &Site<'_>, route: &str, concept: &Concept) -> Result<String, SiteError> {
    let label = if concept.kind == ConceptKind::Equivalent {
        "Same API in"
    } else {
        "Related API"
    };
    let mut output = format!("## {label}\n\n");
    for item in &concept.members {
        let target = site
            .routes
            .get(&DocumentIdentity::Item { item: item.clone() })
            .ok_or(SiteError::Evidence)?;
        if target == route {
            continue;
        }
        let package = &site.workspace.packages[&item.package];
        let name = if package.ecosystem == "python" {
            &site.pages[target].title
        } else {
            &package.items[&item.item].qualified_name
        };
        writeln!(
            output,
            "- [{}: {}]({})",
            escape(&package.name),
            code(name),
            link_url(&site.url(route, target))
        )
        .unwrap();
    }
    output.push('\n');
    Ok(output)
}

struct Markdown<'a, 'b> {
    site: &'a Site<'b>,
    route: &'a str,
    page: &'a PageModel<'b>,
    resolved: Option<&'a ResolvedDocument>,
    identifiers: BTreeSet<String>,
    headings: bool,
    heading_offset: usize,
    cell: usize,
}

impl Markdown<'_, '_> {
    fn reference(&self, kind: ReferenceKind, spelling: &str) -> Result<String, SiteError> {
        let target = &self
            .resolved
            .and_then(|d| {
                d.references
                    .iter()
                    .find(|r| r.kind == kind && r.spelling == spelling)
            })
            .ok_or(SiteError::Evidence)?
            .target;
        if let ReferenceTarget::Anchor { fragment } = target
            && let Some(prefix) = &self.page.anchor_prefix
        {
            return Ok(format!(
                "#{}",
                crate::site::encode(&format!("{prefix}-{fragment}"))
            ));
        }
        self.site.target(self.route, target)
    }

    fn inlines(&self, nodes: &[Inline]) -> Result<String, SiteError> {
        let mut output = String::new();
        for node in nodes {
            match node {
                Inline::Text { value, .. } => output.push_str(&escape(value)),
                Inline::Space { .. } | Inline::SoftBreak { .. } => output.push(' '),
                Inline::HardBreak { .. } => output.push_str("  \n"),
                Inline::NonbreakingSpace { .. } => output.push('\u{a0}'),
                Inline::Code { value, .. } => output.push_str(&code(value)),
                Inline::Math { latex, .. } => write!(output, "${latex}$").unwrap(),
                Inline::Emphasis { inlines, .. } => {
                    write!(output, "*{}*", self.inlines(inlines)?).unwrap()
                }
                Inline::Strong { inlines, .. } => {
                    write!(output, "**{}**", self.inlines(inlines)?).unwrap()
                }
                Inline::Strikeout { inlines, .. } => {
                    write!(output, "~~{}~~", self.inlines(inlines)?).unwrap()
                }
                Inline::Styled { inlines, .. } => output.push_str(&self.inlines(inlines)?),
                Inline::Citation { raw, inlines, .. } => {
                    if inlines.is_empty() {
                        output.push_str(&escape(raw));
                    } else {
                        output.push_str(&self.inlines(inlines)?);
                    }
                }
                Inline::Link {
                    inlines, target, ..
                } => write!(
                    output,
                    "[{}]({})",
                    self.inlines(inlines)?,
                    link_url(&self.reference(ReferenceKind::Link, target)?)
                )
                .unwrap(),
                Inline::Image { alt, target, .. } => write!(
                    output,
                    "![{}]({})",
                    self.inlines(alt)?,
                    link_url(&self.reference(ReferenceKind::Image, target)?)
                )
                .unwrap(),
                Inline::AutoLink { target, .. } => write!(
                    output,
                    "[{}]({})",
                    escape(target),
                    link_url(&self.reference(ReferenceKind::Link, target)?)
                )
                .unwrap(),
                Inline::SemanticReference { target, .. } => write!(
                    output,
                    "[{}]({})",
                    code(target),
                    link_url(&self.reference(ReferenceKind::Semantic, target)?)
                )
                .unwrap(),
                Inline::Unsupported { raw, .. } => output.push_str(&code(raw)),
            }
        }
        Ok(output)
    }

    fn blocks(&mut self, nodes: &[Block]) -> Result<String, SiteError> {
        let mut output = String::new();
        for node in nodes {
            match node {
                Block::Tabset {
                    attributes, blocks, ..
                } => {
                    if let Some(identifier) = &attributes.identifier {
                        anchor(&mut output, &identifier.value);
                    }
                    output.push_str(&self.blocks(blocks)?);
                }
                Block::Container {
                    kind: DocumentContainerKind::GeneratedOutput,
                    blocks,
                    ..
                } => {
                    for block in blocks {
                        if let Block::CodeBlock { source, .. } = block {
                            output.push_str(&fence(source, "text"));
                        } else {
                            output.push_str(&self.blocks(std::slice::from_ref(block))?);
                        }
                    }
                }
                Block::Container {
                    identifier, blocks, ..
                } => {
                    if let Some(identifier) = identifier {
                        anchor(&mut output, identifier);
                    }
                    output.push_str(&self.blocks(blocks)?);
                }
                Block::DisplayMath { latex, .. } => write!(output, "$$\n{latex}\n$$\n\n").unwrap(),
                Block::Paragraph { inlines, .. } => {
                    write!(output, "{}\n\n", self.inlines(inlines)?).unwrap()
                }
                Block::Heading {
                    level,
                    attributes,
                    inlines,
                    ..
                } => {
                    if self.headings && *level + self.heading_offset > 1 {
                        let identifier = if let Some(id) = &attributes.identifier {
                            id.value.clone()
                        } else {
                            let label = super::html::plain(inlines);
                            let slug = label
                                .to_lowercase()
                                .split(|ch: char| !ch.is_alphanumeric())
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>()
                                .join("-");
                            let base = if slug.is_empty() { "section" } else { &slug };
                            let mut id = base.to_owned();
                            let mut suffix = 2;
                            while !self.identifiers.insert(id.clone()) {
                                id = format!("{base}-{suffix}");
                                suffix += 1;
                            }
                            id
                        };
                        let identifier = self
                            .page
                            .anchor_prefix
                            .as_ref()
                            .map(|prefix| format!("{prefix}-{identifier}"))
                            .unwrap_or(identifier);
                        anchor(&mut output, &identifier);
                    }
                    writeln!(
                        output,
                        "{} {}\n",
                        "#".repeat((*level + self.heading_offset).clamp(1, 6)),
                        self.inlines(inlines)?
                    )
                    .unwrap();
                }
                Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => {
                    for line in self.blocks(blocks)?.trim_end().lines() {
                        writeln!(output, "> {line}").unwrap();
                    }
                    output.push('\n');
                }
                Block::List { ordered, items, .. } => {
                    for (index, item) in items.iter().enumerate() {
                        let marker = if *ordered {
                            format!("{}. ", index + 1)
                        } else {
                            "- ".into()
                        };
                        let checked = item
                            .checked
                            .map(|checked| if checked { "[x] " } else { "[ ] " })
                            .unwrap_or("");
                        let content = self.blocks(&item.blocks)?;
                        let mut lines = content.trim_end().lines();
                        writeln!(output, "{marker}{checked}{}", lines.next().unwrap_or(""))
                            .unwrap();
                        let indent = " ".repeat(marker.len());
                        for line in lines {
                            writeln!(output, "{indent}{line}").unwrap();
                        }
                    }
                    output.push('\n');
                }
                Block::ThematicBreak { .. } => output.push_str("---\n\n"),
                Block::CodeBlock {
                    source, language, ..
                } => output.push_str(&fence(source, language.as_deref().unwrap_or(""))),
                Block::CodeCell(cell) => output.push_str(&self.cell(cell)?),
                Block::Table { caption, rows, .. } => {
                    if !caption.is_empty() {
                        writeln!(output, "{}\n", self.inlines(caption)?).unwrap();
                    }
                    for (index, row) in rows.iter().enumerate() {
                        let mut cells = Vec::new();
                        for cell in &row.cells {
                            cells.push(
                                self.blocks(&cell.blocks)?
                                    .trim()
                                    .replace('\n', "<br>")
                                    .replace('|', "\\|"),
                            );
                        }
                        writeln!(output, "| {} |", cells.join(" | ")).unwrap();
                        if index == 0 {
                            writeln!(output, "| {} |", vec!["---"; row.cells.len()].join(" | "))
                                .unwrap();
                        }
                    }
                    output.push('\n');
                }
                Block::Unsupported { raw, .. } => output.push_str(&fence(raw, "text")),
            }
        }
        Ok(output)
    }

    fn cell(&mut self, cell: &CodeCell) -> Result<String, SiteError> {
        let ordinal = self.cell;
        self.cell += 1;
        let (view, options) = if let Some(executed) = self.page.executed {
            let record = executed
                .record()
                .cells
                .get(ordinal)
                .ok_or(SiteError::Evidence)?;
            (
                super::present_cell(&executed.record().page, record)
                    .map_err(|_| SiteError::Evidence)?,
                Some(&record.options),
            )
        } else if let Some(prepared) = self.page.cells.get(ordinal) {
            (
                super::present_prepared_cell(self.page.mode, prepared),
                Some(&prepared.options),
            )
        } else {
            return Ok(fence(&cell.source, cell.language.as_deref().unwrap_or("")));
        };
        let mut output = String::new();
        if let Some(source) = view.source {
            output.push_str(&fence(
                &source.iter().map(|s| s.text.as_str()).collect::<String>(),
                cell.language.as_deref().unwrap_or(""),
            ));
        }
        for record in view.outputs {
            let executed = self.page.executed.ok_or(SiteError::Evidence)?;
            if let Some(representation) = executed.representation(ordinal, record.slot, 0) {
                match representation {
                    ValidatedRepresentationRef::Text(text) => output.push_str(&fence(text, "text")),
                    ValidatedRepresentationRef::Markdown(value) => {
                        output.push_str(&self.blocks(value.blocks())?)
                    }
                    ValidatedRepresentationRef::Html(value) => {
                        output.push_str(&self.html_nodes(value.nodes())?);
                        output.push_str("\n\n");
                    }
                    ValidatedRepresentationRef::Asset(asset) => {
                        let url = self.site.url(
                            self.route,
                            &self.site.asset_route(&asset.reference.fingerprint.value)?,
                        );
                        let alt = options
                            .and_then(|o| o.fig_alt.value.as_deref())
                            .unwrap_or("");
                        writeln!(output, "![{}]({})\n", escape(alt), link_url(&url)).unwrap();
                    }
                }
            } else if let CellOutputKind::Error {
                name,
                message,
                traceback,
            } = &record.output.kind
            {
                output.push_str(&fence(
                    &format!("{name}: {message}\n{}", traceback.join("\n")),
                    "text",
                ));
            } else {
                output.push_str("Unsupported cell output\n\n");
            }
        }
        Ok(output)
    }

    fn html_nodes(&self, nodes: &[HtmlNode]) -> Result<String, SiteError> {
        let mut output = String::new();
        for node in nodes {
            match node {
                HtmlNode::Text(value) => output.push_str(&super::html::escape(value)),
                HtmlNode::Element {
                    name,
                    attributes,
                    children,
                } => {
                    write!(output, "<{name}").unwrap();
                    for (key, value) in attributes {
                        let value = match value {
                            HtmlAttribute::Text(value) => value.clone(),
                            HtmlAttribute::Asset(asset) => self.site.url(
                                self.route,
                                &self.site.asset_route(&asset.reference.fingerprint.value)?,
                            ),
                        };
                        write!(output, " {key}=\"{}\"", super::html::escape(&value)).unwrap();
                    }
                    output.push('>');
                    output.push_str(&self.html_nodes(children)?);
                    if !matches!(name.as_str(), "br" | "hr" | "img") {
                        write!(output, "</{name}>").unwrap();
                    }
                }
            }
        }
        Ok(output)
    }
}

fn sources(site: &Site<'_>, item: &Item) -> String {
    source_location(site, item.source_location.as_ref())
}

fn source_location(site: &Site<'_>, source: Option<&SourceLocation>) -> String {
    let mut output = String::new();
    if let Some(source) = source {
        let target = ReferenceTarget::Repository {
            repository: source.repository.clone(),
            path: source.path.clone(),
            directory: false,
            fragment: None,
        };
        if let Some(url) = crate::validation::repository_target_url(site.workspace, &target) {
            writeln!(output, "[Source]({})\n", link_url(&url)).unwrap();
        }
    }
    output
}

fn anchor(output: &mut String, id: &str) {
    writeln!(output, "<a id=\"{}\"></a>\n", super::html::escape(id)).unwrap();
}
pub(super) fn escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if matches!(
                ch,
                '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|'
            ) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}
pub(super) fn link_url(value: &str) -> String {
    format!("<{}>", value.replace('<', "%3C").replace('>', "%3E"))
}
fn code(value: &str) -> String {
    let ticks = "`".repeat(value.split(|ch| ch != '`').map(str::len).max().unwrap_or(0) + 1);
    let pad = if value.starts_with('`')
        || value.ends_with('`')
        || value.starts_with(' ')
        || value.ends_with(' ')
    {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{value}{pad}{ticks}")
}
fn fence(source: &str, language: &str) -> String {
    let ticks = "`".repeat(
        (source
            .split(|ch| ch != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            + 1)
        .max(3),
    );
    format!(
        "{ticks}{language}\n{}{ticks}\n\n",
        if source.ends_with('\n') {
            source.to_owned()
        } else {
            format!("{source}\n")
        }
    )
}
