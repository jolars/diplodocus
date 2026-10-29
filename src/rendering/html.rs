use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::images::ImageRole;
use super::{
    RenderedFile, RenderedSite, present_cell, present_prepared_cell, render_preformatted_text,
};
use crate::configuration::ConceptKind;
use crate::execution::ValidatedRepresentationRef;
use crate::execution::output_safety::{HtmlAttribute, HtmlNode};
use crate::ir::*;
use crate::site::{PageModel, Site, SiteError};
use crate::validation::{DocumentIdentity, ReferenceKind};

const FRAGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Render one complete site from its prepared model, with no source or database I/O.
pub fn render_site(site: &Site<'_>) -> Result<RenderedSite, SiteError> {
    render(site, true)
}

/// Render document bodies, assets, and a JSON index for an existing website.
///
/// `documents.json` is a versioned [`super::FragmentIndex`] describing HTML and
/// Markdown artifacts, public names, semantic identities, and heading anchors.
/// Apply [`crate::site::UrlMap`] to the site to mount links on another website.
pub fn render_fragments(site: &Site<'_>) -> Result<RenderedSite, SiteError> {
    render(site, false)
}

fn brand(
    html: &mut String,
    site: &Site<'_>,
    route: &str,
    images: &super::images::Images,
) -> Result<(), SiteError> {
    write!(
        html,
        "<a class=\"brand\" href=\"{}\">",
        escape(&site.url(route, "index.html")),
    )
    .unwrap();
    if let Some(logo) = site.logo {
        write!(
            html,
            "<img class=\"brand-logo\" src=\"{}\" alt=\"\"{}{}>",
            escape(&site.url(route, &site.asset_route(&logo.fingerprint.value)?)),
            image_dimensions(site, &logo.fingerprint.value)?,
            images.attributes(site, route, &logo.fingerprint.value, ImageRole::Logo, true)?
        )
        .unwrap();
    }
    write!(html, "<span>{}</span></a>", escape(site.title())).unwrap();
    Ok(())
}

fn image_dimensions(site: &Site<'_>, digest: &str) -> Result<String, SiteError> {
    let asset = site.assets.get(digest).ok_or(SiteError::Evidence)?;
    if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
        return Ok(String::new());
    }
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&asset.bytes))
        .with_guessed_format()
        .map_err(|_| SiteError::Evidence)?
        .into_dimensions()
        .map_err(|_| SiteError::Evidence)?;
    Ok(format!(" width=\"{width}\" height=\"{height}\""))
}

fn navigation_shell_fingerprint(
    site: &Site<'_>,
    images: &super::images::Images,
) -> Result<String, SiteError> {
    // Render from one route so page selection and relative URLs cannot split a shell.
    let mut shell = String::new();
    super::navigation::sections(&mut shell, site, "index.html", "desktop-sections");
    super::navigation::sections(&mut shell, site, "index.html", "mobile-sections");
    super::navigation::repositories(&mut shell, site);
    super::navigation::theme_control(&mut shell);
    brand(&mut shell, site, "index.html", images)?;
    super::footer::render(&mut shell, site);
    // A retained document must not combine runtimes or styles from different builds.
    let syntax_style = super::highlighting::stylesheet();
    for source in [
        STYLE,
        &syntax_style,
        THEME_SCRIPT,
        NAV_SCRIPT,
        SEARCH,
        include_str!("code.js"),
        include_str!("tabsets.js"),
        include_str!("components.js"),
        include_str!("markdown_worker.js"),
    ] {
        shell.push('\0');
        shell.push_str(source);
    }
    Ok(crate::provenance::fingerprint_bytes(shell.as_bytes()).value)
}

fn render(site: &Site<'_>, standalone: bool) -> Result<RenderedSite, SiteError> {
    let mut files = BTreeMap::new();
    let images = super::images::Images::default();
    let shell_fingerprint = standalone
        .then(|| navigation_shell_fingerprint(site, &images))
        .transpose()?;
    if standalone {
        files.insert(
            "__diplodocus/worker.js".into(),
            file(
                include_bytes!("markdown_worker.js").to_vec(),
                "text/javascript; charset=utf-8",
            ),
        );
    }
    let mut search = Vec::new();
    let mut documents = Vec::new();
    let mut uses_math = false;
    let math_context = katex::KatexContext::default();
    let highlighter = super::highlighting::Highlighter::new();
    for (route, page) in &site.pages {
        let home = site.home(route);
        let full_layout = page.has_full_layout();
        let markdown_path = format!(
            "{}.md",
            route.strip_suffix(".html").ok_or(SiteError::Evidence)?
        );
        let page_math = page
            .document
            .is_some_and(|document| super::math::contains_math(&document.document.blocks))
            || page.item.is_some_and(|item| {
                julia_members(item).iter().any(|member| {
                    site.workspace.packages[&member.package].items[&member.item]
                        .documentation
                        .as_ref()
                        .is_some_and(|document| {
                            super::math::contains_math(&document.document.blocks)
                        })
                })
            })
            || page.python_class.as_ref().is_some_and(|class| {
                class.members.iter().any(|member| {
                    !member.inherited
                        && member.item.documentation.as_ref().is_some_and(|document| {
                            super::math::contains_math(&document.document.blocks)
                        })
                })
            })
            || page.rust_page.as_ref().is_some_and(|rust| {
                rust.members().any(|member| {
                    member.item.documentation.as_ref().is_some_and(|document| {
                        super::math::contains_math(&document.document.blocks)
                    })
                })
            });
        uses_math |= page_math;
        let mut renderer = Renderer {
            images: &images,
            standalone,
            site,
            page,
            resolved: page.resolved,
            route,
            document_route: route,
            math_context: &math_context,
            highlighter: &highlighter,
            cell: 0,
            headings: Vec::new(),
            identifiers: page
                .resolved
                .map(|document| document.anchors.clone())
                .unwrap_or_default(),
            document_navigation: true,
            heading_offset: usize::from(
                page.item
                    .is_some_and(|item| super::rust::data(item).is_some()),
            ),
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
        let package_overview =
            super::package_metadata::is_overview(site, route, page.owner.as_deref());
        let mut html = String::new();
        if standalone {
            html = format!(
                "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><script>{THEME_SCRIPT}</script>",
            );
            if let Some(description) = &site.presentation.description {
                write!(
                    html,
                    "<meta name=\"description\" content=\"{}\">",
                    escape(description)
                )
                .unwrap();
            }
            if let Some(url) = site.canonical_urls.get(route) {
                write!(html, "<link rel=\"canonical\" href=\"{}\">", escape(url)).unwrap();
            }
            super::social::render(&mut html, site, route, &page.title)?;
            write!(html, "<title>{} · {}</title><link rel=\"stylesheet\" href=\"{}\"><script defer src=\"{}\"></script><script defer src=\"{}\"></script><script defer src=\"{}\"></script><script defer src=\"{}\"></script>", escape(&page.title), escape(site.title()), escape(&site.url(route, "assets/site.css")), escape(&site.url(route, "assets/search.js")), escape(&site.url(route, "assets/nav.js")), escape(&site.url(route, "assets/code.js")), escape(&site.url(route, "assets/tabsets.js"))).unwrap();
            if page_math {
                write!(
                    html,
                    "<link rel=\"stylesheet\" href=\"{}\">",
                    escape(&site.url(route, "assets/katex/katex.min.css"))
                )
                .unwrap();
            }
            write!(
                html,
                "<link rel=\"alternate\" type=\"text/markdown\" href=\"{}\">",
                escape(&site.url(route, &markdown_path))
            )
            .unwrap();
            if site.components_enabled && !site.components.is_empty() {
                write!(
                    html,
                    "<script defer src=\"{}\"></script>",
                    escape(&site.url(route, "assets/components.js"))
                )
                .unwrap();
            }
            write!(html, "</head><body data-diplodocus-navigation=\"1\" data-diplodocus-shell=\"{}\" data-layout=\"{}\"><a class=\"skip-link\" href=\"#main\">Skip to content</a>", shell_fingerprint.as_deref().expect("standalone shell fingerprint"), if home.is_some() { "home" } else if full_layout { "full" } else { "docs" }).unwrap();
            html.push_str("<header>");
            brand(&mut html, site, route, &images)?;
            super::navigation::sections(&mut html, site, route, "desktop-sections");
            html.push_str("<div class=\"header-actions\"><form class=\"site-search\" role=\"search\"><label class=\"visually-hidden\" for=\"search\">Search documentation</label><div class=\"search-control\"><svg class=\"search-icon\" aria-hidden=\"true\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\"><circle cx=\"10.5\" cy=\"10.5\" r=\"6.5\"></circle><path d=\"m16 16 4 4\"></path></svg><input id=\"search\" type=\"search\" placeholder=\"Search\" autocomplete=\"off\" aria-controls=\"search-results\"></div><ul id=\"search-results\" aria-live=\"polite\"></ul></form>");
            super::navigation::theme_control(&mut html);
            super::navigation::repositories(&mut html, site);
            html.push_str("<details class=\"mobile-menu\"><summary aria-label=\"Navigation menu\"><svg aria-hidden=\"true\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\"><path d=\"M4 6h16M4 12h16M4 18h16\"></path></svg></summary><div class=\"mobile-menu-panel\">");
            super::navigation::sections(&mut html, site, route, "mobile-sections");
            super::navigation::theme_control(&mut html);
            super::navigation::repositories(&mut html, site);
            html.push_str("</div></details></div>");
            html.push_str("</header><div class=\"layout\"><nav aria-label=\"Documentation\">");
            html.push_str(
                "<details class=\"nav-disclosure\" open><summary><svg aria-hidden=\"true\" width=\"20\" height=\"20\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\"><path d=\"M4 6h16M4 12h10M4 18h16\"></path></svg>Menu</summary>",
            );
            write!(
                html,
                "<div class=\"contextual-nav\" data-section=\"{}\">",
                escape(
                    site.presentation
                        .sidebar
                        .as_ref()
                        .and_then(|sidebar| sidebar.for_route(route))
                        .map(|(prefix, _)| prefix)
                        .unwrap_or_else(|| site.project_section(route).map_or_else(
                            || page.owner.as_deref().unwrap_or(""),
                            |section| section.path.as_str(),
                        ))
                )
            )
            .unwrap();
            super::navigation::contextual(&mut html, site, route);
            html.push_str("</div></details></nav><div class=\"page-content\">");
            if home.is_some() {
                html.push_str("<main id=\"main\" class=\"home-page\">");
            } else if package_overview {
                html.push_str("<main id=\"main\" class=\"package-overview\">");
            } else {
                html.push_str("<main id=\"main\">");
            }
        } else if package_overview {
            html.push_str("<div class=\"package-overview\">");
        }
        let julia_method = page.item.filter(|item| {
            item.kind == ItemKind::Method
                && matches!(&item.language_data, Some(ItemLanguageData::Julia(_)))
        });
        // Keep the authored heading's formatting and anchor when it supplies the title.
        let authored_title = page.item.is_none()
            && page.concept.is_none()
            && page.document.is_some_and(|document| {
                matches!(
                    document.document.blocks.first(),
                    Some(Block::Heading { level: 1, inlines, .. })
                        if home.is_some() || plain(inlines) == page.title
                )
            });
        if let Some(home) = home {
            super::home::render(&mut html, site, route, home, &images, standalone)?;
            if page.document.is_some() {
                html.push_str("<div class=\"home-content\">");
            }
        } else if !authored_title {
            let title = if let Some(inlines) = page.item.and_then(super::r_topics::title) {
                renderer.inlines(inlines)?
            } else if page.item.is_some() {
                format!(
                    "<code>{}</code>",
                    escape(
                        julia_method
                            .map_or(page.title.as_str(), |item| item.qualified_name.as_str())
                    )
                )
            } else {
                escape(&page.title)
            };
            write!(html, "<h1>{title}</h1>").unwrap();
        } else {
            let document = page.document.expect("authored title");
            html.push_str(&renderer.blocks(&document.document.blocks[..1])?);
        }
        if let Some(owner) = &page.owner
            && !package_overview
        {
            let package = &site.workspace.packages[owner];
            write!(
                html,
                "<p class=\"package\">{} · {}{}{}</p>",
                escape(&package.name),
                escape(&package.ecosystem),
                package
                    .version
                    .as_ref()
                    .map(|v| format!(" · {}", escape(v)))
                    .unwrap_or_default(),
                if julia_method.is_some() {
                    " · Method"
                } else {
                    ""
                }
            )
            .unwrap();
        }
        if let Some(item) = page.item
            && matches!(&item.language_data, Some(ItemLanguageData::R(_)))
        {
            write!(
                html,
                "<p class=\"api-name\"><code>{}</code></p>",
                escape(&item.qualified_name)
            )
            .unwrap();
            html.push_str(&super::r_topics::source_links(site, item));
        }
        if !standalone
            && let Some(item) = page.item
            && !matches!(&item.language_data, Some(ItemLanguageData::R(_)))
        {
            html.push_str(&super::r_topics::source_links(site, item));
        }
        if standalone
            && let Some(item) = page.item
            && matches!(&item.language_data, Some(ItemLanguageData::Rust(_)))
        {
            html.push_str(&super::r_topics::source_links(site, item));
        }
        if let Some(document) = page.document
            && matches!(
                document.source_format,
                DocumentFormat::Authored {
                    format: crate::documents::AuthoredFormat::Rmd
                }
            )
            && let Some(MetadataValue::Mapping { entries, .. }) = &document.document.frontmatter
        {
            let metadata: Vec<_> = ["author", "date"]
                .into_iter()
                .filter_map(|key| entries.iter().find(|e| e.key.value == key))
                .filter_map(|e| match &e.value {
                    MetadataValue::Scalar { value, .. } => Some(escape(value)),
                    _ => None,
                })
                .collect();
            if !metadata.is_empty() {
                write!(
                    html,
                    "<p class=\"article-metadata\">{}</p>",
                    metadata.join(" · ")
                )
                .unwrap();
            }
        }
        if package_overview
            && let Some(owner) = &page.owner
            && let Some(PackageMetadata::R(metadata)) = &site.workspace.packages[owner].metadata
        {
            write!(
                html,
                "<p class=\"package-introduction\">{}</p>",
                escape(&metadata.description.title)
            )
            .unwrap();
        }
        if package_overview {
            html.push_str(&super::package_metadata::panel(
                site,
                route,
                page.owner.as_deref().expect("package overview owner"),
            ));
            html.push_str("<div class=\"package-overview-body\">");
        }
        let mut rd_body_start = None;
        if let Some(item) = page.item
            && matches!(&item.language_data, Some(ItemLanguageData::R(_)))
            && let Some(document) = page.document
        {
            let title_end = usize::from(super::r_topics::title(item).is_some());
            let introduction_end = document.document.blocks[title_end..]
                .iter()
                .position(|block| matches!(block, Block::Heading { .. }))
                .map_or(document.document.blocks.len(), |index| index + title_end);
            html.push_str(
                &renderer.blocks(&document.document.blocks[title_end..introduction_end])?,
            );
            rd_body_start = Some(introduction_end);
        }
        if let Some(item) = page.item {
            let ecosystem = page
                .owner
                .as_ref()
                .map(|p| site.workspace.packages[p].ecosystem.as_str())
                .unwrap_or("");
            let name = if ecosystem == "python" {
                page.title.rsplit('.').next().unwrap_or(&item.name)
            } else {
                &item.name
            };
            let rd_usage = matches!(&item.language_data, Some(ItemLanguageData::R(_)))
                && item.documentation.as_ref().is_some_and(|document| document.document.blocks.iter().any(|block|
                    matches!(block, Block::Heading { level: 2, inlines, .. } if plain(inlines) == "Usage")));
            if !rd_usage {
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
                    let anchor = overloads
                        .get(index)
                        .map(|member| crate::site::python_overload_anchor(&member.item));
                    if let Some(anchor) = &anchor {
                        renderer.identifiers.insert(anchor.clone());
                        write!(html, "<div id=\"{}\">", escape(anchor)).unwrap();
                    }
                    html.push_str(&highlighter.render_signature(
                        &super::signatures::signature_for_site(
                            site,
                            name,
                            &signature.signature,
                            ecosystem,
                        ),
                        Some(ecosystem),
                    ));
                    if anchor.is_some() {
                        html.push_str("</div>");
                    }
                }
            }
            if let Some(class) = &page.python_class {
                if !class.bases.is_empty() {
                    write!(
                        html,
                        "<p>Bases: {}</p>",
                        class
                            .bases
                            .iter()
                            .map(|base| format!(
                                "<code>{}</code>",
                                escape(&super::signatures::expression_for_site(
                                    site, base, "python"
                                ))
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .unwrap();
                }
                if !class.constructors.is_empty() {
                    html.push_str("<div id=\"constructor\">");
                }
                for signature in &class.constructors {
                    html.push_str(&highlighter.render_signature(
                        &super::signatures::signature_for_site(site, name, signature, "python"),
                        Some("python"),
                    ));
                }
                if !class.constructors.is_empty() {
                    html.push_str("</div>");
                }
                if let Some(owner) = &class.constructor_owner
                    && site.workspace.packages[&owner.package].items[&owner.item].qualified_name
                        != item.qualified_name
                {
                    let target = site.target(
                        route,
                        &crate::validation::ReferenceTarget::Item {
                            item: owner.clone(),
                        },
                    )?;
                    write!(
                        html,
                        "<p>Constructor inherited from <a href=\"{}\"><code>{}</code></a>.</p>",
                        escape(&target),
                        escape(&site.workspace.packages[&owner.package].items[&owner.item].name)
                    )
                    .unwrap();
                }
            }
        }
        if let Some(document) = page.document {
            let blocks = if let Some(start) = rd_body_start {
                &document.document.blocks[start..]
            } else if authored_title || page.item.and_then(super::r_topics::title).is_some() {
                &document.document.blocks[1..]
            } else {
                &document.document.blocks
            };
            html.push_str(&renderer.blocks(blocks)?);
        }
        if let Some(item) = page.item {
            let members = julia_members(item);
            if !members.is_empty() {
                html.push_str("<h2 id=\"methods\">Methods</h2>");
                renderer
                    .headings
                    .push((2, "methods".into(), "Methods".into()));
                for member in members {
                    let method = &site.workspace.packages[&member.package].items[&member.item];
                    let method_page = &site.embedded_pages[member];
                    let anchor = &site.overload_anchors[member];
                    renderer.headings.push((
                        3,
                        anchor.clone(),
                        method
                            .signatures
                            .first()
                            .map(|s| {
                                super::signatures::signature(&method.name, &s.signature, "julia")
                            })
                            .unwrap_or_else(|| method.name.clone()),
                    ));
                    let title = method
                        .signatures
                        .first()
                        .map(|s| {
                            super::signatures::signature_for_site(
                                site,
                                &method.name,
                                &s.signature,
                                "julia",
                            )
                        })
                        .unwrap_or_else(|| method.name.clone());
                    write!(
                        html,
                        "<section id=\"{}\"><h3><code>{}</code></h3>",
                        escape(anchor),
                        escape(&title)
                    )
                    .unwrap();
                    if let Some(document) = &method.documentation {
                        // Method references keep their own resolution context on the family page.
                        let mut method_renderer = Renderer {
                            images: &images,
                            standalone,
                            site,
                            page: method_page,
                            resolved: method_page.resolved,
                            route,
                            document_route: route,
                            math_context: &math_context,
                            highlighter: &highlighter,
                            cell: 0,
                            headings: Vec::new(),
                            identifiers: BTreeSet::new(),
                            document_navigation: true,
                            heading_offset: 2,
                        };
                        html.push_str(&method_renderer.blocks(&document.document.blocks)?);
                        renderer.headings.extend(method_renderer.headings);
                    }
                    html.push_str("</section>");
                }
            }
        }
        html.push_str(&renderer.python_members()?);
        html.push_str(&renderer.rust_members()?);
        if page.reference_index {
            html.push_str(&super::r_topics::index(
                site,
                route,
                page.owner.as_deref().expect("reference index owner"),
            ));
        }
        if let (Some(item), Some(owner)) = (page.item, page.owner.as_deref()) {
            html.push_str(&super::r_topics::navigation(
                site,
                route,
                owner,
                item,
                |title| {
                    renderer.blocks(&[Block::Heading {
                        level: 2,
                        attributes: Attributes::default(),
                        inlines: vec![Inline::Text {
                            value: title.into(),
                            span: SourceSpan { start: 0, end: 0 },
                        }],
                        span: SourceSpan { start: 0, end: 0 },
                    }])
                },
            )?);
        }
        if let Some(concept) = page.concept {
            html.push_str(&concept_links(site, route, concept)?);
        }
        if let Some(identity) = site.routes.iter().find_map(|(id, path)| {
            let overload = matches!(id, DocumentIdentity::Item { item }
                    if site.overload_anchors.contains_key(item));
            (path == route && !overload).then_some(id)
        }) && let DocumentIdentity::Item { item } = identity
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
        if page.document.is_none()
            && page.item.is_none()
            && page.concept.is_none()
            && !page.reference_index
            && home.is_none()
        {
            html.push_str("<ul>");
            for (path, candidate) in &site.pages {
                if candidate.visible
                    && path != route
                    && (page.owner.is_none() || candidate.owner == page.owner)
                {
                    write!(
                        html,
                        "<li><a href=\"{}\">{}</a></li>",
                        escape(&site.url(route, path)),
                        if candidate.item.is_some() {
                            format!("<code>{}</code>", escape(&candidate.title))
                        } else {
                            escape(&candidate.title)
                        }
                    )
                    .unwrap();
                }
            }
            html.push_str("</ul>");
        }
        if package_overview {
            html.push_str("</div>");
            if !standalone {
                html.push_str("</div>");
            }
        }
        if home.is_some() && page.document.is_some() {
            html.push_str("</div>");
        }
        files.insert(
            markdown_path.clone(),
            file(
                super::markdown::page(site, route, page)?.into_bytes(),
                "text/markdown; charset=utf-8",
            ),
        );
        if standalone {
            html.push_str("</main>");
            if home.is_none() && !full_layout {
                html.push_str(&renderer.page_navigation());
            }
            html.push_str("</div></div>");
            super::footer::render(&mut html, site);
            html.push_str("</body></html>\n");
        } else {
            let identities: Vec<_> = site
                .routes
                .iter()
                .filter(|(_, path)| *path == route)
                .map(|(identity, _)| identity.clone())
                .collect();
            documents.push(super::FragmentDocument {
                path: route.clone(),
                markdown_path,
                url: site.url("index.html", route),
                title: page.title.clone(),
                package: page.owner.clone(),
                package_version: page
                    .owner
                    .as_ref()
                    .and_then(|id| site.workspace.packages[id].version.clone()),
                qualified_name: page.item.map(|item| item.qualified_name.clone()),
                public_name: page.public_name.clone(),
                visible: page.visible,
                identities,
                headings: renderer.headings,
            });
        }
        if page.visible {
            let package = page.owner.as_ref().map(|id| &site.workspace.packages[id]);
            let mut search_text = page
                .document
                .and_then(|document| {
                    document
                        .generated_source
                        .as_deref()
                        .or(document.raw_source.as_deref())
                })
                .unwrap_or("")
                .to_owned();
            if let Some(home) = home {
                write!(search_text, "\n{}\n{}", home.tagline, home.description).unwrap();
                for feature in &home.features {
                    write!(search_text, "\n{}\n{}", feature.title, feature.description).unwrap();
                }
            }
            if let Some(item) = page.item {
                for member in julia_members(item) {
                    if let Some(raw) = site.workspace.packages[&member.package].items[&member.item]
                        .documentation
                        .as_ref()
                        .and_then(|d| d.raw_source.as_deref())
                    {
                        search_text.push('\n');
                        search_text.push_str(raw);
                    }
                }
            }
            if let Some(class) = &page.python_class {
                for member in &class.members {
                    search_text.push('\n');
                    search_text.push_str(&member.item.qualified_name);
                    for alias in &member.item.aliases {
                        search_text.push('\n');
                        search_text.push_str(&alias.qualified_name);
                    }
                    if let Some(raw) = member
                        .item
                        .documentation
                        .as_ref()
                        .and_then(|document| document.raw_source.as_deref())
                    {
                        search_text.push('\n');
                        search_text.push_str(raw);
                    }
                }
            }
            if let Some(rust) = &page.rust_page {
                for member in rust.members() {
                    let public = super::rust::data(member.item)
                        .expect("Rust member")
                        .public_path
                        .as_str();
                    let mut text = member
                        .item
                        .documentation
                        .as_ref()
                        .and_then(|document| {
                            document
                                .generated_source
                                .as_deref()
                                .or(document.raw_source.as_deref())
                        })
                        .unwrap_or("")
                        .to_owned();
                    for alias in &member.item.aliases {
                        text.push('\n');
                        text.push_str(&alias.qualified_name);
                    }
                    search_text.push('\n');
                    search_text.push_str(public);
                    search_text.push('\n');
                    search_text.push_str(&text);
                    let destination = site.target(
                        "index.html",
                        &crate::validation::ReferenceTarget::Item {
                            item: member.reference.clone(),
                        },
                    )?;
                    search.push(serde_json::json!({"title": public, "path": destination, "package": package.map(|package| &package.name), "ecosystem": "rust", "api": true, "text": text}));
                }
            }
            if package_overview {
                search_text.push_str(&format!(
                    "\n{}",
                    super::package_metadata::search_text(
                        site,
                        page.owner.as_deref().expect("package overview owner")
                    )
                ));
            }
            if let Some(item) = page.item
                && package
                    .is_some_and(|package| matches!(package.ecosystem.as_str(), "python" | "rust"))
            {
                search_text.push('\n');
                search_text.push_str(&item.qualified_name);
                for alias in &item.aliases {
                    search_text.push('\n');
                    search_text.push_str(&alias.qualified_name);
                }
            }
            let search_title = page
                .item
                .filter(|item| matches!(&item.language_data, Some(ItemLanguageData::R(_))))
                .map_or(page.title.as_str(), |item| item.qualified_name.as_str());
            search.push(serde_json::json!({"title": search_title, "path": site.url("index.html",route), "package": package.map(|p| &p.name), "ecosystem": package.map(|p| &p.ecosystem), "api": page.item.is_some(), "text": search_text}));
        }
        files.insert(
            route.clone(),
            file(html.into_bytes(), "text/html; charset=utf-8"),
        );
    }
    if uses_math {
        for (path, bytes, media_type) in super::katex_assets::ASSETS {
            files.insert((*path).into(), file(bytes.to_vec(), media_type));
        }
    }
    files.extend(images.into_files());
    for (digest, asset) in site.assets {
        files.insert(
            site.asset_route(digest)?,
            file(asset.bytes.clone(), &asset.media_type),
        );
    }
    let style = if standalone {
        format!("{STYLE}\n{}", super::highlighting::stylesheet())
    } else {
        super::highlighting::fragment_stylesheet()
    };
    files.insert(
        if standalone {
            "assets/site.css"
        } else {
            "assets/content.css"
        }
        .into(),
        file(style.into_bytes(), "text/css; charset=utf-8"),
    );
    if standalone && site.components_enabled && !site.components.is_empty() {
        files.insert(
            "assets/components.js".into(),
            file(
                include_str!("components.js").as_bytes().to_vec(),
                "text/javascript; charset=utf-8",
            ),
        );
        for (id, bundle) in site.components {
            let prefix = bundle.prefix(id);
            for (path, digest) in &bundle.files {
                files.insert(
                    format!("{prefix}{path}"),
                    file(
                        site.assets[digest].bytes.clone(),
                        crate::components::media_type(path),
                    ),
                );
            }
        }
    }
    if standalone {
        files.insert(
            "assets/search.js".into(),
            file(SEARCH.as_bytes().to_vec(), "text/javascript; charset=utf-8"),
        );
        files.insert(
            "assets/nav.js".into(),
            file(
                NAV_SCRIPT.as_bytes().to_vec(),
                "text/javascript; charset=utf-8",
            ),
        );
        files.insert(
            "assets/code.js".into(),
            file(
                include_str!("code.js").as_bytes().to_vec(),
                "text/javascript; charset=utf-8",
            ),
        );
    }
    files.insert(
        "assets/tabsets.js".into(),
        file(
            include_str!("tabsets.js").as_bytes().to_vec(),
            "text/javascript; charset=utf-8",
        ),
    );
    files.insert(
        "assets/search.json".into(),
        file(
            serde_json::to_vec(&search).map_err(|_| SiteError::Evidence)?,
            "application/json",
        ),
    );
    if standalone {
        super::sitemap::render(site, &mut files)?;
        if !site.redirects.is_empty() {
            files.insert(
                "_redirects".into(),
                file(
                    site.redirect_rules().into_bytes(),
                    "text/plain; charset=utf-8",
                ),
            );
        }
    } else {
        files.insert(
            "documents.json".into(),
            file(
                serde_json::to_vec(&super::FragmentIndex {
                    format_version: 2,
                    documents,
                })
                .map_err(|_| SiteError::Evidence)?,
                "application/json",
            ),
        );
    }
    if let Some(map) = &site.url_map {
        map.validate_assets(&files)?;
    }
    Ok(RenderedSite {
        files,
        redirects: if standalone {
            site.redirects.clone()
        } else {
            BTreeMap::new()
        },
    })
}

pub(super) fn julia_members(item: &Item) -> &[ItemReference] {
    match &item.language_data {
        Some(ItemLanguageData::Julia(data)) => match &data.declaration {
            JuliaDeclaration::Callable {
                role: JuliaCallableRole::Family { methods },
                ..
            } => methods,
            JuliaDeclaration::Type { constructors, .. } => constructors,
            _ => &[],
        },
        _ => &[],
    }
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
        if target == route {
            continue;
        }
        let package = &site.workspace.packages[&item.package];
        write!(
            html,
            "<li><a href=\"{}\">{}: <code>{}</code></a></li>",
            escape(&site.url(route, target)),
            escape(&package.name),
            escape(if package.ecosystem == "python" {
                &site.pages[target].title
            } else {
                &package.items[&item.item].qualified_name
            })
        )
        .unwrap();
    }
    html.push_str("</ul></section>");
    Ok(html)
}
struct Renderer<'a, 'b> {
    images: &'a super::images::Images,
    standalone: bool,
    highlighter: &'a super::highlighting::Highlighter,
    math_context: &'a katex::KatexContext,
    site: &'a Site<'b>,
    page: &'a PageModel<'b>,
    resolved: Option<&'b crate::validation::ResolvedDocument>,
    route: &'a str,
    document_route: &'a str,
    cell: usize,
    headings: Vec<(usize, String, String)>,
    identifiers: BTreeSet<String>,
    document_navigation: bool,
    heading_offset: usize,
}
impl Renderer<'_, '_> {
    fn rust_members(&mut self) -> Result<String, SiteError> {
        let Some(rust) = &self.page.rust_page else {
            return Ok(String::new());
        };
        let mut html = String::new();
        for section in &rust.sections {
            self.headings
                .push((2, section.anchor.clone(), section.title.clone()));
            let title = if let Some(target) = section
                .implementation
                .and_then(|implementation| implementation.trait_target.as_ref())
            {
                format!(
                    "<a href=\"{}\">{}</a>",
                    escape(&self.site.target(
                        self.route,
                        &crate::validation::ReferenceTarget::Item {
                            item: target.clone()
                        }
                    )?),
                    escape(&section.title)
                )
            } else {
                escape(&section.title)
            };
            write!(
                html,
                "<section class=\"rust-members\"><h2 id=\"{}\">{}</h2>",
                escape(&section.anchor),
                title
            )
            .unwrap();
            if let Some(implementation) = section.implementation {
                html.push_str(&super::r_topics::source_locations(
                    self.site,
                    implementation.source_location.iter(),
                ));
                html.push_str(&self.highlighter.render_signature(
                    &super::signatures::expression_for_site(
                        self.site,
                        &implementation.signature,
                        "rust",
                    ),
                    Some("rust"),
                ));
            }
            if let Some(documentation) = &section.documentation {
                html.push_str(&self.rust_document(documentation, 2)?);
            }
            for member in &section.members {
                html.push_str(&self.rust_member(member, 3)?);
            }
            html.push_str("</section>");
        }
        Ok(html)
    }

    fn rust_member(
        &mut self,
        member: &super::rust::RustMember<'_>,
        level: usize,
    ) -> Result<String, SiteError> {
        self.headings
            .push((level, member.anchor.clone(), member.item.name.clone()));
        let mut html = format!(
            "<section id=\"{}\"><h{level}><code>{}</code></h{level}>",
            escape(&member.anchor),
            escape(&member.item.name)
        );
        html.push_str(&super::r_topics::source_links(self.site, member.item));
        for signature in &member.item.signatures {
            html.push_str(&self.highlighter.render_signature(
                &super::signatures::signature_for_site(
                    self.site,
                    &member.item.name,
                    &signature.signature,
                    "rust",
                ),
                Some("rust"),
            ));
        }
        html.push_str(&self.rust_document(member, level)?);
        for child in &member.children {
            html.push_str(&self.rust_member(child, level + 1)?);
        }
        html.push_str("</section>");
        Ok(html)
    }

    fn rust_document(
        &mut self,
        member: &super::rust::RustMember<'_>,
        level: usize,
    ) -> Result<String, SiteError> {
        let mut html = String::new();
        if let Some(document) = &member.item.documentation {
            let page = &self.site.embedded_pages[&member.reference];
            let mut renderer = Renderer {
                site: self.site,
                images: self.images,
                standalone: self.standalone,
                page,
                resolved: member.resolved,
                route: self.route,
                document_route: self.route,
                math_context: self.math_context,
                highlighter: self.highlighter,
                cell: 0,
                headings: Vec::new(),
                identifiers: BTreeSet::new(),
                document_navigation: true,
                heading_offset: level,
            };
            html.push_str(&renderer.blocks(&document.document.blocks)?);
            self.headings.extend(renderer.headings);
        }
        Ok(html)
    }

    fn python_members(&mut self) -> Result<String, SiteError> {
        let Some(class) = &self.page.python_class else {
            return Ok(String::new());
        };
        let mut html = String::new();
        for member in class.members.iter().filter(|member| !member.inherited) {
            self.identifiers.insert(member.anchor.clone());
            self.headings
                .push((2, member.anchor.clone(), member.item.name.clone()));
            write!(
                html,
                "<section><h2 id=\"{}\"><code>{}</code></h2>",
                escape(&member.anchor),
                escape(&member.item.name)
            )
            .unwrap();
            html.push_str(&super::r_topics::source_links(self.site, member.item));
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
                let anchor = overloads
                    .get(index)
                    .map(|overload| crate::site::python_overload_anchor(&overload.item));
                if let Some(anchor) = &anchor {
                    write!(html, "<div id=\"{}\">", escape(anchor)).unwrap();
                }
                html.push_str(&self.highlighter.render_signature(
                    &super::signatures::signature_for_site(
                        self.site,
                        &member.item.name,
                        signature,
                        "python",
                    ),
                    Some("python"),
                ));
                if anchor.is_some() {
                    html.push_str("</div>");
                }
            }
            if let Some(document) = &member.item.documentation {
                // A method's short references resolve in its defining context.
                let mut renderer = Renderer {
                    site: self.site,
                    images: self.images,
                    standalone: self.standalone,
                    page: self.page,
                    resolved: member.resolved,
                    route: self.route,
                    document_route: self.route,
                    math_context: self.math_context,
                    highlighter: self.highlighter,
                    cell: 0,
                    headings: Vec::new(),
                    identifiers: BTreeSet::new(),
                    document_navigation: false,
                    heading_offset: 1,
                };
                html.push_str(&renderer.blocks(&document.document.blocks)?);
            }
            html.push_str("</section>");
        }
        let inherited: Vec<_> = class
            .members
            .iter()
            .filter(|member| member.inherited)
            .collect();
        if !inherited.is_empty() {
            html.push_str("<h2>Inherited members</h2><ul>");
            for member in inherited {
                let target = self.site.target(
                    self.route,
                    &crate::validation::ReferenceTarget::Item {
                        item: member.reference.clone(),
                    },
                )?;
                write!(
                    html,
                    "<li><a href=\"{}\"><code>{}</code></a></li>",
                    escape(&target),
                    escape(&member.item.name)
                )
                .unwrap();
            }
            html.push_str("</ul>");
        }
        Ok(html)
    }
    fn heading_id(&mut self, attributes: &Attributes, label: &str) -> String {
        let prefix = self
            .page
            .anchor_prefix
            .as_ref()
            .map(|anchor| format!("{anchor}-"))
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
        while !self.identifiers.insert(identifier.clone()) {
            identifier = format!("{base}-{suffix}");
            suffix += 1;
        }
        identifier
    }

    fn page_navigation(&self) -> String {
        if self.headings.is_empty() {
            return "<aside class=\"page-toc page-toc-empty\"><nav aria-label=\"Page\"><a class=\"back-to-top\" href=\"\">Back to top</a></nav></aside>".into();
        }
        let mut html = String::from(
            "<aside class=\"page-toc\"><nav aria-label=\"On this page\"><details class=\"toc-disclosure\" open><summary>On this page</summary><ul><li class=\"toc-back-to-top\"><a class=\"back-to-top\" href=\"\">Back to top</a></li>",
        );
        for (level, identifier, title) in &self.headings {
            write!(
                html,
                "<li class=\"toc-level-{level}\"><a href=\"#{}\">{}</a></li>",
                escape(&percent_encoding::utf8_percent_encode(identifier, FRAGMENT).to_string()),
                escape(title)
            )
            .unwrap();
        }
        html.push_str("</ul></details></nav></aside>");
        html
    }

    fn reference(&self, kind: ReferenceKind, spelling: &str) -> Result<String, SiteError> {
        let reference = self
            .resolved
            .and_then(|d| {
                d.references
                    .iter()
                    .find(|r| r.kind == kind && r.spelling == spelling)
            })
            .ok_or(SiteError::Evidence)?;
        if self.document_route != self.route
            && let crate::validation::ReferenceTarget::Anchor { fragment } = &reference.target
        {
            // Embedded headings link to their method page to avoid ambiguous local anchors.
            return Ok(format!(
                "{}#{}",
                self.site.url(self.route, self.document_route),
                crate::site::encode(fragment)
            ));
        }
        if let crate::validation::ReferenceTarget::Anchor { fragment } = &reference.target
            && let Some(anchor) = &self.page.anchor_prefix
        {
            return Ok(format!(
                "#{}",
                crate::site::encode(&format!("{anchor}-{fragment}"))
            ));
        }
        self.site.target(self.route, &reference.target)
    }
    fn image_attributes(&self, spelling: &str) -> Result<String, SiteError> {
        let reference = self
            .resolved
            .and_then(|document| {
                document.references.iter().find(|reference| {
                    reference.kind == ReferenceKind::Image && reference.spelling == spelling
                })
            })
            .ok_or(SiteError::Evidence)?;
        match &reference.target {
            crate::validation::ReferenceTarget::Asset { asset, .. } => Ok(format!(
                "{}{}",
                image_dimensions(self.site, &asset.fingerprint.value)?,
                self.images.attributes(
                    self.site,
                    self.route,
                    &asset.fingerprint.value,
                    ImageRole::Content,
                    self.standalone
                )?
            )),
            _ => Ok(String::new()),
        }
    }
    fn inlines(&self, inlines: &[Inline]) -> Result<String, SiteError> {
        let mut html = String::new();
        for inline in inlines {
            match inline {
                Inline::Styled { style, inlines, .. } => {
                    let tag = match style {
                        InlineStyle::Superscript => "sup",
                        InlineStyle::Subscript => "sub",
                        InlineStyle::SmallCaps => "span class=\"small-caps\"",
                    };
                    write!(
                        html,
                        "<{tag}>{}</{}>",
                        self.inlines(inlines)?,
                        tag.split(' ').next().unwrap()
                    )
                    .unwrap();
                }
                Inline::Citation { raw, inlines, .. } => {
                    if inlines.is_empty() {
                        html.push_str(&escape(raw));
                    } else {
                        html.push_str(&self.inlines(inlines)?);
                    }
                }
                Inline::Math {
                    latex, fallback, ..
                } => html.push_str(&super::math::render(
                    self.math_context,
                    latex,
                    fallback.as_deref(),
                    false,
                )),
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
                    "<img src=\"{}\" alt=\"{}\"{}{}{}>",
                    escape(&self.reference(ReferenceKind::Image, target)?),
                    escape(&plain(alt)),
                    self.image_attributes(target)?,
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
                Inline::Unsupported {
                    source_kind, raw, ..
                } => {
                    let label = unsupported_rd_label(source_kind);
                    write!(
                        html,
                        "<span class=\"unsupported\">{}</span>",
                        escape(label.as_deref().unwrap_or(raw))
                    )
                    .unwrap()
                }
            }
        }
        Ok(html)
    }
    fn tabset(&mut self, attributes: &Attributes, blocks: &[Block]) -> Result<String, SiteError> {
        let level = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .min();
        let mut html = format!("<div class=\"panel-tabset\"{}", id(attributes));
        if let Some(group) = attributes
            .key_values
            .iter()
            .find(|pair| pair.key.value == "group")
        {
            write!(
                html,
                " data-tabset-group=\"{}\"",
                escape(&group.value.value)
            )
            .unwrap();
        }
        html.push('>');
        let mut start = 0;
        let mut panel_open = false;
        for (index, block) in blocks.iter().enumerate() {
            let Block::Heading {
                level: heading_level,
                attributes,
                inlines,
                ..
            } = block
            else {
                continue;
            };
            if Some(*heading_level) != level {
                continue;
            }
            html.push_str(&self.blocks(&blocks[start..index])?);
            if panel_open {
                html.push_str("</section>");
            }
            let label = plain(inlines);
            let identifier = self.heading_id(attributes, &label);
            let active = attributes
                .classes
                .iter()
                .any(|class| class.value == "active");
            let heading_level = (*heading_level + self.heading_offset).clamp(1, 6);
            write!(html, "<section class=\"tabset-panel\"><h{heading_level} class=\"tabset-heading\" id=\"{}\" data-tab-active=\"{active}\">{}</h{heading_level}>", escape(&identifier), self.inlines(inlines)?).unwrap();
            panel_open = true;
            start = index + 1;
        }
        html.push_str(&self.blocks(&blocks[start..])?);
        if panel_open {
            html.push_str("</section>");
        }
        html.push_str("</div>");
        Ok(html)
    }

    fn blocks(&mut self, blocks: &[Block]) -> Result<String, SiteError> {
        let mut html = String::new();
        let mut rd_usage = false;
        for block in blocks {
            if let Block::Heading {
                level: 2, inlines, ..
            } = block
            {
                rd_usage = plain(inlines) == "Usage"
                    && self.page.document.is_some_and(|document| {
                        matches!(&document.source_format, crate::ir::DocumentFormat::Extracted { name } if name == "rd")
                    });
            }
            match block {
                Block::Component {
                    attributes, blocks, ..
                } => {
                    if self.standalone && self.site.components_enabled {
                        let name =
                            crate::components::name(attributes).ok_or(SiteError::Evidence)?;
                        let bundle = self.site.components.get(name).ok_or(SiteError::Evidence)?;
                        let prefix = bundle.prefix(name);
                        let module = self
                            .site
                            .url(self.route, &format!("{prefix}{}", bundle.module));
                        let styles: Vec<_> = bundle
                            .stylesheets
                            .iter()
                            .map(|s| self.site.url(self.route, &format!("{prefix}{s}")))
                            .collect();
                        write!(html, "<div class=\"diplodocus-component\"{} data-component-module=\"{}\" data-component-styles=\"{}\"><div data-component-fallback>{}</div></div>", id(attributes), escape(&module), escape(&serde_json::to_string(&styles).map_err(|_| SiteError::Evidence)?), self.blocks(blocks)?).unwrap();
                    } else {
                        write!(
                            html,
                            "<div{}>{}</div>",
                            id(attributes),
                            self.blocks(blocks)?
                        )
                        .unwrap();
                    }
                }
                Block::Tabset {
                    attributes, blocks, ..
                } => {
                    html.push_str(&self.tabset(attributes, blocks)?);
                }
                Block::Container {
                    kind,
                    identifier,
                    blocks,
                    ..
                } => {
                    let (tag, class) = match kind {
                        DocumentContainerKind::DefinitionList => ("dl", "definition-list"),
                        DocumentContainerKind::DefinitionTerm => ("dt", "definition-term"),
                        DocumentContainerKind::Definition => ("dd", "definition-body"),
                        DocumentContainerKind::Figure => ("figure", "article-figure"),
                        DocumentContainerKind::Footnotes => ("section", "footnotes"),
                        DocumentContainerKind::Footnote => ("div", "footnote"),
                        DocumentContainerKind::BibliographyEntry => ("div", "bibliography-entry"),
                        DocumentContainerKind::GeneratedChunk => ("div", "code-cell"),
                        DocumentContainerKind::GeneratedOutput => ("div", "code-output"),
                    };
                    write!(
                        html,
                        "<{tag} class=\"{class}\"{}>",
                        identifier
                            .as_ref()
                            .map(|id| format!(" id=\"{}\"", escape(id)))
                            .unwrap_or_default()
                    )
                    .unwrap();
                    if *kind == DocumentContainerKind::Footnotes {
                        html.push_str("<h2>Notes</h2>");
                    }
                    if *kind == DocumentContainerKind::Footnote
                        && let Some(number) =
                            identifier.as_ref().and_then(|id| id.rsplit('-').next())
                    {
                        write!(
                            html,
                            "<span class=\"footnote-number\">{}.</span>",
                            escape(number)
                        )
                        .unwrap();
                    }
                    let navigation = self.document_navigation;
                    if matches!(
                        kind,
                        DocumentContainerKind::GeneratedChunk
                            | DocumentContainerKind::GeneratedOutput
                    ) {
                        self.document_navigation = false;
                    }
                    if *kind == DocumentContainerKind::GeneratedOutput {
                        for block in blocks {
                            if let Block::CodeBlock { source, .. } = block {
                                html.push_str(&render_preformatted_text(source));
                            } else {
                                html.push_str(&self.blocks(std::slice::from_ref(block))?);
                            }
                        }
                    } else if *kind == DocumentContainerKind::Figure {
                        if let Some(first) = blocks.first() {
                            html.push_str(&self.blocks(std::slice::from_ref(first))?);
                        }
                        if blocks.len() > 1 {
                            write!(
                                html,
                                "<figcaption>{}</figcaption>",
                                self.blocks(&blocks[1..])?
                            )
                            .unwrap();
                        }
                    } else {
                        html.push_str(&self.blocks(blocks)?);
                    }
                    self.document_navigation = navigation;
                    write!(html, "</{tag}>").unwrap();
                }
                Block::DisplayMath {
                    latex, fallback, ..
                } => html.push_str(&super::math::render(
                    self.math_context,
                    latex,
                    fallback.as_deref(),
                    true,
                )),
                Block::Paragraph { inlines, .. } => {
                    write!(html, "<p>{}</p>", self.inlines(inlines)?).unwrap()
                }
                Block::Heading {
                    level,
                    attributes,
                    inlines,
                    ..
                } => {
                    let level = (*level + self.heading_offset).clamp(1, 6);
                    let label = plain(inlines);
                    let identifier = if self.document_navigation && level > 1 {
                        let identifier = self.heading_id(attributes, &label);
                        self.headings.push((level, identifier.clone(), label));
                        format!(" id=\"{}\"", escape(&identifier))
                    } else if self.document_navigation {
                        id(attributes)
                    } else {
                        String::new()
                    };
                    write!(
                        html,
                        "<h{level}{identifier}>{}</h{level}>",
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
                Block::CodeBlock {
                    source,
                    language,
                    span,
                    ..
                } => {
                    let julia_doctest = language
                        .as_deref()
                        .is_some_and(|l| l.starts_with("jldoctest"));
                    let hidden: Vec<_> = self
                        .page
                        .document
                        .into_iter()
                        .flat_map(|d| &d.provenance)
                        .filter_map(|p| match &p.activity {
                            ProvenanceActivity::ApiExamples {
                                language, examples, ..
                            } if language == "julia" => Some(examples),
                            _ => None,
                        })
                        .flatten()
                        .filter(|e| {
                            span.start <= e.span.start
                                && e.span.end <= span.end
                                && e.options.get("output").is_some_and(|v| v == "false")
                        })
                        .map(|e| e.source.as_str())
                        .collect();
                    let hidden_source = hidden.join("\n");
                    let source = if hidden.is_empty() {
                        source
                    } else {
                        &hidden_source
                    };
                    let language = if julia_doctest {
                        Some("julia")
                    } else {
                        language.as_deref()
                    };
                    html.push_str(&if rd_usage {
                        self.highlighter.render_signature(source, language)
                    } else {
                        self.highlighter.render(source, language)
                    });
                }
                Block::CodeCell(cell) => html.push_str(&self.code_cell(cell)?),
                Block::Table {
                    caption,
                    rows,
                    alignments,
                    ..
                } => {
                    html.push_str("<table>");
                    if !caption.is_empty() {
                        write!(html, "<caption>{}</caption>", self.inlines(caption)?).unwrap();
                    }
                    for row in rows {
                        html.push_str("<tr>");
                        let tag = if row.header { "th" } else { "td" };
                        for (column, cell) in row.cells.iter().enumerate() {
                            let alignment = match alignments.get(column) {
                                Some(TableAlignment::Left) => " class=\"align-left\"",
                                Some(TableAlignment::Center) => " class=\"align-center\"",
                                Some(TableAlignment::Right) => " class=\"align-right\"",
                                _ => "",
                            };
                            write!(
                                html,
                                "<{tag}{alignment}>{}</{tag}>",
                                self.blocks(&cell.blocks)?
                            )
                            .unwrap();
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
                Block::Unsupported {
                    source_kind, raw, ..
                } => {
                    html.push_str("<div class=\"unsupported\">");
                    if let Some(label) = unsupported_rd_label(source_kind) {
                        write!(html, "<p>{}</p>", escape(&label)).unwrap();
                    } else {
                        html.push_str("<p>Unsupported content</p>");
                        html.push_str(&render_preformatted_text(raw));
                    }
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
            html.push_str(
                &self
                    .highlighter
                    .render(&cell.source, cell.language.as_deref()),
            );
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
            html.push_str(&self.highlighter.render(
                &source.iter().map(|s| s.text.as_str()).collect::<String>(),
                cell.language.as_deref(),
            ));
        }
        let mut figure = 0;
        for output in view.outputs {
            let page = executed.ok_or(SiteError::Evidence)?;
            if let Some(representation) = page.representation(ordinal, output.slot, 0) {
                match representation {
                    ValidatedRepresentationRef::Text(text) => {
                        html.push_str(&render_console_output(text))
                    }
                    ValidatedRepresentationRef::Markdown(value) => {
                        // Executed output must not become part of the document's navigation.
                        self.document_navigation = false;
                        let output = self.blocks(value.blocks());
                        self.document_navigation = true;
                        html.push_str(&output?);
                    }
                    ValidatedRepresentationRef::Html(value) => {
                        html.push_str(&self.html_nodes(value.nodes())?)
                    }
                    ValidatedRepresentationRef::Asset(asset) => {
                        let options = options.ok_or(SiteError::Evidence)?;
                        write!(
                            html,
                            "<figure><img src=\"{}\" alt=\"{}\"{}{}>",
                            escape(&self.site.url(
                                self.route,
                                &self.site.asset_route(&asset.reference.fingerprint.value)?
                            )),
                            escape(options.fig_alt.value.as_deref().unwrap_or("")),
                            image_dimensions(self.site, &asset.reference.fingerprint.value)?,
                            self.images.attributes(
                                self.site,
                                self.route,
                                &asset.reference.fingerprint.value,
                                ImageRole::Content,
                                self.standalone
                            )?
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
                html.push_str(&render_console_output(&format!(
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
                            HtmlAttribute::Asset(asset) => self.site.url(
                                self.route,
                                &self.site.asset_route(&asset.reference.fingerprint.value)?,
                            ),
                        };
                        write!(html, " {key}=\"{}\"", escape(&value)).unwrap();
                    }
                    if name == "img"
                        && !attributes.contains_key("width")
                        && !attributes.contains_key("height")
                        && let Some(HtmlAttribute::Asset(asset)) = attributes.get("src")
                    {
                        html.push_str(&image_dimensions(
                            self.site,
                            &asset.reference.fingerprint.value,
                        )?);
                    }
                    if name == "img"
                        && let Some(HtmlAttribute::Asset(asset)) = attributes.get("src")
                    {
                        html.push_str(&self.images.attributes(
                            self.site,
                            self.route,
                            &asset.reference.fingerprint.value,
                            ImageRole::Content,
                            self.standalone,
                        )?);
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
fn render_console_output(text: &str) -> String {
    format!(
        "<div class=\"code-output\">{}</div>",
        render_preformatted_text(text)
    )
}

fn unsupported_rd_label(source_kind: &str) -> Option<String> {
    // Older snapshots may store the entire Rd file in each unsupported node.
    source_kind
        .strip_prefix("rd-")
        .map(|kind| format!("[Unsupported Rd: {kind}]"))
}

pub(super) fn plain(nodes: &[Inline]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Inline::Math {
                latex, fallback, ..
            } => fallback.as_ref().unwrap_or(latex).clone(),
            Inline::Text { value, .. } | Inline::Code { value, .. } => value.clone(),
            Inline::SemanticReference { target, .. } => target.clone(),
            Inline::Emphasis { inlines, .. }
            | Inline::Strong { inlines, .. }
            | Inline::Styled { inlines, .. }
            | Inline::Citation { inlines, .. }
            | Inline::Strikeout { inlines, .. }
            | Inline::Link { inlines, .. } => plain(inlines),
            Inline::Image { alt, .. } => plain(alt),
            Inline::Unsupported {
                source_kind, raw, ..
            } => unsupported_rd_label(source_kind).unwrap_or_else(|| raw.clone()),
            _ => " ".into(),
        })
        .collect()
}
const STYLE: &str = concat!(
    include_str!("assets/styles/base.css"),
    include_str!("assets/styles/layout.css"),
    include_str!("assets/styles/navigation.css"),
    include_str!("assets/styles/search.css"),
    include_str!("assets/styles/content.css"),
    include_str!("assets/styles/home.css"),
);
const NAV_SCRIPT: &str = include_str!("navigation.js");
const THEME_SCRIPT: &str = include_str!("theme.js");
const SEARCH: &str = include_str!("search.js");
