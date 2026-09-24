use std::collections::BTreeMap;
use std::fmt::Write;

use super::{
    RenderedFile, RenderedSite, present_cell, present_prepared_cell, render_preformatted_text,
};
use crate::configuration::ConceptKind;
use crate::execution::ValidatedRepresentationRef;
use crate::execution::output_safety::{HtmlAttribute, HtmlNode};
use crate::ir::*;
use crate::site::{PageModel, Site, SiteError, relative_url};
use crate::validation::{DocumentIdentity, ReferenceKind};

/// Render one complete site from its prepared model, with no source or database I/O.
pub fn render_site(site: &Site<'_>) -> Result<RenderedSite, SiteError> {
    let mut files = BTreeMap::new();
    let mut search = Vec::new();
    for (route, page) in &site.pages {
        let mut renderer = Renderer {
            site,
            page,
            route,
            cell: 0,
        };
        let mut html = String::from(
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        );
        write!(html, "<title>{} · {}</title><link rel=\"stylesheet\" href=\"{}\"><script defer src=\"{}\"></script></head><body><a class=\"skip-link\" href=\"#main\">Skip to content</a>", escape(&page.title), escape(&site.workspace.name), escape(&relative_url(route, "assets/site.css")), escape(&relative_url(route, "assets/search.js"))).unwrap();
        write!(html, "<header><a class=\"brand\" href=\"{}\">{}</a><form role=\"search\"><label for=\"search\">Search documentation</label><input id=\"search\" type=\"search\" autocomplete=\"off\"><ul id=\"search-results\" aria-live=\"polite\"></ul></form></header><div class=\"layout\"><nav aria-label=\"Documentation\"><ul>", escape(&relative_url(route, "index.html")), escape(&site.workspace.name)).unwrap();
        for (destination, candidate) in &site.pages {
            if !candidate.visible {
                continue;
            }
            // Item lists stay with their package instead of overwhelming project navigation.
            if candidate.item.is_some() && candidate.owner != page.owner {
                continue;
            }
            let active = if destination == route {
                " aria-current=\"page\""
            } else {
                ""
            };
            write!(
                html,
                "<li><a href=\"{}\"{active}>{}</a></li>",
                escape(&relative_url(route, destination)),
                escape(&candidate.title)
            )
            .unwrap();
        }
        html.push_str("</ul></nav><main id=\"main\">");
        if let Some(owner) = &page.owner {
            let package = &site.workspace.packages[owner];
            write!(
                html,
                "<p class=\"package\">{} · {}{}</p>",
                escape(&package.name),
                escape(&package.ecosystem),
                package
                    .version
                    .as_ref()
                    .map(|v| format!(" · {}", escape(v)))
                    .unwrap_or_default()
            )
            .unwrap();
        }
        write!(html, "<h1>{}</h1>", escape(&page.title)).unwrap();
        if let Some(item) = page.item {
            let ecosystem = page
                .owner
                .as_ref()
                .map(|p| site.workspace.packages[p].ecosystem.as_str())
                .unwrap_or("");
            for signature in &item.signatures {
                html.push_str(&render_preformatted_text(&super::signatures::signature(
                    &item.name,
                    &signature.signature,
                    ecosystem,
                )));
            }
        }
        if let Some(document) = page.document {
            html.push_str(&renderer.blocks(&document.document.blocks)?);
        }
        if let Some(concept) = page.concept {
            html.push_str(&concept_links(site, route, concept)?);
        }
        if let Some(identity) = site
            .routes
            .iter()
            .find_map(|(id, path)| (path == route).then_some(id))
            && let DocumentIdentity::Item { item } = identity
        {
            for concept in site
                .workspace
                .concepts
                .values()
                .filter(|c| c.members.contains(item))
            {
                html.push_str(&concept_links(site, route, concept)?);
            }
        }
        if page.document.is_none() && page.item.is_none() && page.concept.is_none() {
            html.push_str("<ul>");
            for (path, candidate) in &site.pages {
                if candidate.visible
                    && path != route
                    && (page.owner.is_none() || candidate.owner == page.owner)
                {
                    write!(
                        html,
                        "<li><a href=\"{}\">{}</a></li>",
                        escape(&relative_url(route, path)),
                        escape(&candidate.title)
                    )
                    .unwrap();
                }
            }
            html.push_str("</ul>");
        }
        html.push_str("</main></div></body></html>\n");
        if page.visible {
            let package = page.owner.as_ref().map(|id| &site.workspace.packages[id]);
            search.push(serde_json::json!({"title": page.title, "path": route.split('/').map(crate::site::encode).collect::<Vec<_>>().join("/"), "package": package.map(|p| &p.name), "ecosystem": package.map(|p| &p.ecosystem), "text": page.document.and_then(|d| d.raw_source.as_deref()).unwrap_or("")}));
        }
        files.insert(
            route.clone(),
            file(html.into_bytes(), "text/html; charset=utf-8"),
        );
    }
    for (digest, asset) in site.assets {
        files.insert(
            site.asset_route(digest)?,
            file(asset.bytes.clone(), &asset.media_type),
        );
    }
    files.insert(
        "assets/site.css".into(),
        file(STYLE.as_bytes().to_vec(), "text/css; charset=utf-8"),
    );
    files.insert(
        "assets/search.js".into(),
        file(SEARCH.as_bytes().to_vec(), "text/javascript; charset=utf-8"),
    );
    files.insert(
        "assets/search.json".into(),
        file(
            serde_json::to_vec(&search).map_err(|_| SiteError::Evidence)?,
            "application/json",
        ),
    );
    Ok(RenderedSite { files })
}
fn file(bytes: Vec<u8>, media_type: &str) -> RenderedFile {
    RenderedFile {
        bytes,
        media_type: media_type.into(),
    }
}
pub(super) fn escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            _ => c.to_string(),
        })
        .collect()
}
fn id(attributes: &Attributes) -> String {
    attributes
        .identifier
        .as_ref()
        .map(|s| format!(" id=\"{}\"", escape(&s.value)))
        .unwrap_or_default()
}
fn concept_links(site: &Site<'_>, route: &str, concept: &Concept) -> Result<String, SiteError> {
    let label = if concept.kind == ConceptKind::Equivalent {
        "Same API in"
    } else {
        "Related API"
    };
    let mut html = format!("<section><h2>{label}</h2><ul>");
    for item in &concept.members {
        let target = site
            .routes
            .get(&DocumentIdentity::Item { item: item.clone() })
            .ok_or(SiteError::Evidence)?;
        let package = &site.workspace.packages[&item.package];
        write!(
            html,
            "<li><a href=\"{}\">{}: {}</a></li>",
            escape(&relative_url(route, target)),
            escape(&package.name),
            escape(&package.items[&item.item].qualified_name)
        )
        .unwrap();
    }
    html.push_str("</ul></section>");
    Ok(html)
}
struct Renderer<'a, 'b> {
    site: &'a Site<'b>,
    page: &'a PageModel<'b>,
    route: &'a str,
    cell: usize,
}
impl Renderer<'_, '_> {
    fn reference(&self, kind: ReferenceKind, spelling: &str) -> Result<String, SiteError> {
        let reference = self
            .page
            .resolved
            .and_then(|d| {
                d.references
                    .iter()
                    .find(|r| r.kind == kind && r.spelling == spelling)
            })
            .ok_or(SiteError::Evidence)?;
        self.site.target(self.route, &reference.target)
    }
    fn inlines(&self, inlines: &[Inline]) -> Result<String, SiteError> {
        let mut html = String::new();
        for inline in inlines {
            match inline {
                Inline::Text { value, .. } => html.push_str(&escape(value)),
                Inline::Space { .. } | Inline::SoftBreak { .. } => html.push(' '),
                Inline::HardBreak { .. } => html.push_str("<br>"),
                Inline::NonbreakingSpace { .. } => html.push_str("&nbsp;"),
                Inline::Emphasis { inlines, .. } => {
                    write!(html, "<em>{}</em>", self.inlines(inlines)?).unwrap()
                }
                Inline::Strong { inlines, .. } => {
                    write!(html, "<strong>{}</strong>", self.inlines(inlines)?).unwrap()
                }
                Inline::Strikeout { inlines, .. } => {
                    write!(html, "<del>{}</del>", self.inlines(inlines)?).unwrap()
                }
                Inline::Code { value, .. } => {
                    write!(html, "<code>{}</code>", escape(value)).unwrap()
                }
                Inline::Link {
                    inlines,
                    target,
                    attributes,
                    title,
                    ..
                } => write!(
                    html,
                    "<a href=\"{}\"{}{}>{}</a>",
                    escape(&self.reference(ReferenceKind::Link, target)?),
                    id(attributes),
                    title
                        .as_ref()
                        .map(|t| format!(" title=\"{}\"", escape(t)))
                        .unwrap_or_default(),
                    self.inlines(inlines)?
                )
                .unwrap(),
                Inline::Image {
                    alt,
                    target,
                    attributes,
                    title,
                    ..
                } => write!(
                    html,
                    "<img src=\"{}\" alt=\"{}\"{}{}>",
                    escape(&self.reference(ReferenceKind::Image, target)?),
                    escape(&plain(alt)),
                    id(attributes),
                    title
                        .as_ref()
                        .map(|t| format!(" title=\"{}\"", escape(t)))
                        .unwrap_or_default()
                )
                .unwrap(),
                Inline::SemanticReference { target, .. } => write!(
                    html,
                    "<a href=\"{}\"><code>{}</code></a>",
                    escape(&self.reference(ReferenceKind::Semantic, target)?),
                    escape(target)
                )
                .unwrap(),
                Inline::AutoLink { target, .. } => write!(
                    html,
                    "<a href=\"{}\">{}</a>",
                    escape(&self.reference(ReferenceKind::Link, target)?),
                    escape(target)
                )
                .unwrap(),
                Inline::Unsupported { raw, .. } => {
                    write!(html, "<span class=\"unsupported\">{}</span>", escape(raw)).unwrap()
                }
            }
        }
        Ok(html)
    }
    fn blocks(&mut self, blocks: &[Block]) -> Result<String, SiteError> {
        let mut html = String::new();
        for block in blocks {
            match block {
                Block::Paragraph { inlines, .. } => {
                    write!(html, "<p>{}</p>", self.inlines(inlines)?).unwrap()
                }
                Block::Heading {
                    level,
                    attributes,
                    inlines,
                    ..
                } => {
                    let level = (*level).clamp(1, 6);
                    write!(
                        html,
                        "<h{level}{}>{}</h{level}>",
                        id(attributes),
                        self.inlines(inlines)?
                    )
                    .unwrap();
                }
                Block::BlockQuote { blocks, .. } => {
                    write!(html, "<blockquote>{}</blockquote>", self.blocks(blocks)?).unwrap()
                }
                Block::List { ordered, items, .. } => {
                    let tag = if *ordered { "ol" } else { "ul" };
                    write!(html, "<{tag}>").unwrap();
                    for item in items {
                        html.push_str("<li>");
                        if let Some(checked) = item.checked {
                            write!(
                                html,
                                "<input type=\"checkbox\" disabled{} aria-label=\"Task\">",
                                if checked { " checked" } else { "" }
                            )
                            .unwrap();
                        }
                        html.push_str(&self.blocks(&item.blocks)?);
                        html.push_str("</li>");
                    }
                    write!(html, "</{tag}>").unwrap();
                }
                Block::ThematicBreak { .. } => html.push_str("<hr>"),
                Block::CodeBlock { source, .. } => html.push_str(&render_preformatted_text(source)),
                Block::CodeCell(cell) => html.push_str(&self.code_cell(cell)?),
                Block::Table { caption, rows, .. } => {
                    html.push_str("<table>");
                    if !caption.is_empty() {
                        write!(html, "<caption>{}</caption>", self.inlines(caption)?).unwrap();
                    }
                    for row in rows {
                        html.push_str("<tr>");
                        let tag = if row.header { "th" } else { "td" };
                        for cell in &row.cells {
                            write!(html, "<{tag}>{}</{tag}>", self.blocks(&cell.blocks)?).unwrap();
                        }
                        html.push_str("</tr>");
                    }
                    html.push_str("</table>");
                }
                Block::Callout {
                    kind,
                    attributes,
                    blocks,
                    ..
                } => write!(
                    html,
                    "<aside class=\"callout\"{}><p><strong>{:?}</strong></p>{}</aside>",
                    id(attributes),
                    kind,
                    self.blocks(blocks)?
                )
                .unwrap(),
                Block::Unsupported { raw, .. } => {
                    html.push_str("<div class=\"unsupported\"><p>Unsupported content</p>");
                    html.push_str(&render_preformatted_text(raw));
                    html.push_str("</div>");
                }
            }
        }
        Ok(html)
    }
    fn code_cell(&mut self, cell: &CodeCell) -> Result<String, SiteError> {
        let ordinal = self.cell;
        self.cell += 1;
        let mut html = String::new();
        let executed = self.page.executed;
        let (view, options) = if let Some(page) = executed {
            let record = page
                .record()
                .cells
                .get(ordinal)
                .ok_or(SiteError::Evidence)?;
            (
                present_cell(&page.record().page, record).map_err(|_| SiteError::Evidence)?,
                Some(&record.options),
            )
        } else if let Some(prepared) = self.page.cells.get(ordinal) {
            (
                present_prepared_cell(self.page.mode, prepared),
                Some(&prepared.options),
            )
        } else {
            html.push_str(&render_preformatted_text(&cell.source));
            return Ok(html);
        };
        let label = options
            .and_then(|o| o.label.value.as_ref())
            .or_else(|| cell.identifier.as_ref().map(|s| &s.value));
        write!(
            html,
            "<div class=\"code-cell\"{}>",
            label
                .map(|s| format!(" id=\"{}\"", escape(s)))
                .unwrap_or_default()
        )
        .unwrap();
        if let Some(source) = view.source {
            html.push_str(&render_preformatted_text(
                &source.iter().map(|s| s.text.as_str()).collect::<String>(),
            ));
        }
        let mut figure = 0;
        for output in view.outputs {
            let page = executed.ok_or(SiteError::Evidence)?;
            if let Some(representation) = page.representation(ordinal, output.slot, 0) {
                match representation {
                    ValidatedRepresentationRef::Text(text) => {
                        html.push_str(&render_preformatted_text(text))
                    }
                    ValidatedRepresentationRef::Markdown(value) => {
                        html.push_str(&self.blocks(value.blocks())?)
                    }
                    ValidatedRepresentationRef::Html(value) => {
                        html.push_str(&self.html_nodes(value.nodes())?)
                    }
                    ValidatedRepresentationRef::Asset(asset) => {
                        let options = options.ok_or(SiteError::Evidence)?;
                        write!(
                            html,
                            "<figure><img src=\"{}\" alt=\"{}\">",
                            escape(&relative_url(
                                self.route,
                                &self.site.asset_route(&asset.reference.fingerprint.value)?
                            )),
                            escape(options.fig_alt.value.as_deref().unwrap_or(""))
                        )
                        .unwrap();
                        if let Some(caption) = options.fig_subcap.value.get(figure) {
                            write!(html, "<figcaption>{}</figcaption>", escape(caption)).unwrap();
                        }
                        figure += 1;
                        html.push_str("</figure>");
                    }
                }
            } else if let CellOutputKind::Error {
                name,
                message,
                traceback,
            } = &output.output.kind
            {
                html.push_str(&render_preformatted_text(&format!(
                    "{name}: {message}\n{}",
                    traceback.join("\n")
                )));
            } else {
                html.push_str("<p class=\"unsupported\">Unsupported cell output</p>");
            }
        }
        if !view.outputs.is_empty()
            && let Some(caption) = options.and_then(|o| o.fig_cap.value.as_ref())
        {
            write!(html, "<p class=\"caption\">{}</p>", escape(caption)).unwrap();
        }
        html.push_str("</div>");
        Ok(html)
    }
    fn html_nodes(&self, nodes: &[HtmlNode]) -> Result<String, SiteError> {
        let mut html = String::new();
        for node in nodes {
            match node {
                HtmlNode::Text(text) => html.push_str(&escape(text)),
                HtmlNode::Element {
                    name,
                    attributes,
                    children,
                } => {
                    write!(html, "<{name}").unwrap();
                    for (key, value) in attributes {
                        let value = match value {
                            HtmlAttribute::Text(value) => value.clone(),
                            HtmlAttribute::Asset(asset) => relative_url(
                                self.route,
                                &self.site.asset_route(&asset.reference.fingerprint.value)?,
                            ),
                        };
                        write!(html, " {key}=\"{}\"", escape(&value)).unwrap();
                    }
                    html.push('>');
                    html.push_str(&self.html_nodes(children)?);
                    if !matches!(name.as_str(), "br" | "hr" | "img") {
                        write!(html, "</{name}>").unwrap();
                    }
                }
            }
        }
        Ok(html)
    }
}
fn plain(nodes: &[Inline]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Inline::Text { value, .. } | Inline::Code { value, .. } => value.clone(),
            Inline::SemanticReference { target, .. } => target.clone(),
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Strikeout { inlines, .. }
            | Inline::Link { inlines, .. } => plain(inlines),
            Inline::Image { alt, .. } => plain(alt),
            Inline::Unsupported { raw, .. } => raw.clone(),
            _ => " ".into(),
        })
        .collect()
}
const STYLE: &str = "body{margin:0;color:#202c38;background:#fafbf9;font:17px/1.65 system-ui,sans-serif}a{color:#165c7c}a:focus-visible,input:focus-visible{outline:3px solid #a04605;outline-offset:3px}header{padding:1.5rem 3rem;border-bottom:1px solid #d6dedc;display:flex;gap:2rem;justify-content:space-between;align-items:start}.brand{font-size:1.5rem;font-weight:700;text-decoration:none}form label{display:block;font-size:.8rem}input[type=search]{padding:.5rem;font:inherit;max-width:100%;box-sizing:border-box}.layout{display:grid;grid-template-columns:minmax(12rem,19rem) minmax(0,1fr);max-width:90rem;margin:auto}nav{padding:2rem;border-right:1px solid #d6dedc;font-size:.9rem}nav ul{list-style:none;padding:0}nav li{margin:.4rem 0;overflow-wrap:anywhere}[aria-current=page]{font-weight:700}main{padding:2.5rem 4rem;max-width:54rem;min-width:0}h1,h2,h3{line-height:1.25;letter-spacing:-.02em}h1{font-size:2.3rem}.package{font-size:.85rem;color:#4d606b}pre{padding:1rem;background:#edf1f0;overflow:auto;border-radius:.25rem}code{font-size:.9em}img{max-width:100%;height:auto}figure{margin:1.5rem 0}.caption,figcaption{font-size:.9rem;color:#4d606b}table{border-collapse:collapse;display:block;overflow:auto}th,td{border:1px solid #bbc9c5;padding:.3rem .7rem}blockquote,.callout{border-left:4px solid #688b84;padding:.2rem 1rem;margin:1rem 0}.unsupported{border-left:4px solid #a04605;padding-left:1rem}.skip-link{position:absolute;left:-10000px}.skip-link:focus{left:1rem;top:1rem;background:white;padding:1rem}#search-results{max-width:24rem;font-size:.85rem}@media(max-width:760px){header{padding:1rem;display:block}.layout{display:block}nav{border-right:0;border-bottom:1px solid #d6dedc;padding:1rem}nav ul{max-height:12rem;overflow:auto}main{padding:1.5rem}h1{font-size:1.9rem}}";
const SEARCH: &str = "(()=>{const script=document.currentScript;const root=new URL('../',script.src);const input=document.querySelector('#search');const results=document.querySelector('#search-results');let entries=[];fetch(new URL('search.json',script.src)).then(r=>r.json()).then(v=>{entries=v}).catch(()=>{});input.form.addEventListener('submit',e=>e.preventDefault());input.addEventListener('input',()=>{results.replaceChildren();const q=input.value.trim().toLowerCase();if(!q)return;for(const entry of entries.filter(e=>(e.title+' '+e.text+' '+(e.package||'')).toLowerCase().includes(q)).slice(0,12)){const li=document.createElement('li');const a=document.createElement('a');a.href=new URL(entry.path,root);a.textContent=entry.title+(entry.package?' · '+entry.package:'');li.append(a);results.append(li)}})})();";
