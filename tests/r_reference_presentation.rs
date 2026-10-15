mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn render_with_repository(settings: &str) -> RenderedSite {
    let root = support::shared_r_workspace();
    root.write(
        "workspace/diplodocus.toml",
        root.read("workspace/diplodocus.toml")
            .replace("path='../r'", &format!("path='../r'\n{settings}")),
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(snapshot);
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    render_site(&Site::new(&loaded).unwrap()).unwrap()
}

fn html<'a>(rendered: &'a RenderedSite, path: &str) -> &'a str {
    std::str::from_utf8(&rendered.files()[path].bytes).unwrap()
}

#[test]
fn reference_titles_routes_introductions_and_summaries_survive_without_sources() {
    let rendered =
        render_with_repository("url='https://github.com/example/shared'\nrevision='release/v1'");
    for name in ["fit", "fit.default", "fit.model"] {
        let page = html(&rendered, &format!("packages/r/reference/{name}.html"));
        assert!(page.contains("<title>Fit a Model · Shared R help</title>"));
        assert_eq!(page.matches("<h1>").count(), 1);
        assert!(page.contains("<h1>Fit a Model</h1>"));
        assert!(page.contains(&format!("<p class=\"api-name\"><code>{name}</code></p>")));
        assert!(
            page.find("Fit a model with class-specific arguments.")
                .unwrap()
                < page.find(">Usage</h2>").unwrap()
        );
        assert_eq!(page.matches("<pre class=\"syntax-code\">").count(), 3);
        assert!(page.contains("<span class=\"syntax-"));
        assert!(page.contains("src=\"../../../assets/code.js\""));
        assert!(page.contains("https://github.com/example/shared/blob/release%2Fv1/man/fit.Rd"));
        assert!(page.contains("https://github.com/example/shared/blob/release%2Fv1/R/shared.R"));
    }
    let index = html(&rendered, "packages/r/reference/index.html");
    assert!(index.contains("<h1>Reference</h1>"));
    assert!(index.contains("href=\"fit.html\"><code>fit</code></a>"));
    assert!(index.contains("Fit a Model"));
    assert!(index.contains("Fit a model with class-specific arguments."));
    let summaries = index
        .split("<ul class=\"reference-topics\">")
        .nth(1)
        .unwrap();
    assert!(!summaries.contains("fit.default"));
    assert!(!summaries.contains("fit.model"));
    assert_eq!(summaries.matches("Compose Euler Diagrams").count(), 1);
    assert!(
        rendered
            .files()
            .contains_key("packages/r/reference/eulergram-compose.html")
    );
}

#[test]
fn source_links_honor_templates_and_require_an_appropriate_origin_and_revision() {
    let rendered = render_with_repository(
        "revision='v1'\nsource-link-template='https://forge.example/project/{kind}/{revision}/{path}#L{line}'",
    );
    let page = html(&rendered, "packages/r/reference/fit.html");
    assert!(page.contains("https://forge.example/project/blob/v1/R/shared.R"));
    assert!(page.contains("https://forge.example/project/blob/v1/man/fit.Rd"));
    assert!(!page.contains("#L1"));
    for settings in [
        "",
        "url='https://github.com/example/shared'",
        "url='https://forge.example/shared'\nrevision='v1'",
        "revision='v1'\nsource-link-template='javascript:{revision}/{path}'",
    ] {
        let rendered = render_with_repository(settings);
        assert!(
            !html(&rendered, "packages/r/reference/fit.html").contains("class=\"source-links\"")
        );
    }
}

#[test]
fn formatted_titles_and_route_collisions_remain_distinct() {
    let root = support::shared_r_workspace();
    root.write(
        "r/man/fit.Rd",
        root.read("r/man/fit.Rd")
            .replace("\\title{Fit a Model}", "\\title{Fit a \\emph{Model}}"),
    );
    root.write(
        "r/R/extra.R",
        "`a/b` <- function() NULL\n`a|b` <- function() NULL\nindex <- function() NULL\n",
    );
    root.write(
        "r/NAMESPACE",
        root.read("r/NAMESPACE") + "\nexport(\"a/b\", \"a|b\", index)\n",
    );
    // Distinct topic files can have names that normalize to the same route stem.
    root.write(
        "r/man/second.Rd",
        "\\name{a|b}\\alias{a|b}\\title{Extra}\\usage{`a|b`()}\\description{Additional function.}",
    );
    root.write(
        "r/man/a-b.Rd",
        "\\name{a/b}\\alias{a/b}\\title{Extra}\\usage{`a/b`()}\\description{Additional function.}",
    );
    root.write("r/man/index.Rd", "\\name{index}\\alias{index}\\title{Extra}\\usage{index()}\\description{Additional function.}");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert!(
        html(&rendered, "packages/r/reference/fit.html").contains("<h1>Fit a <em>Model</em></h1>")
    );
    let routes: Vec<_> = rendered
        .files()
        .keys()
        .filter(|path| path.starts_with("packages/r/reference/a-b-"))
        .collect();
    assert_eq!(routes.len(), 2);
    assert!(html(&rendered, "packages/r/reference/index.html").contains("<h1>Reference</h1>"));
    assert!(
        rendered
            .files()
            .keys()
            .any(|path| path.starts_with("packages/r/reference/index-"))
    );
}

#[test]
fn introduction_precedes_the_maintained_signature_when_rd_omits_usage() {
    let root = support::shared_r_workspace();
    root.write("r/man/fit.Rd", root.read("r/man/fit.Rd").replace("\\usage{\nfit(x, ...)\n\\method{fit}{default}(x, y, ...)\n\\method{fit}{model}(x, weights = NULL, ...)\n}\n", ""));
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let page = html(&rendered, "packages/r/reference/fit.html");
    assert!(
        page.find("Fit a model with class-specific arguments.")
            .unwrap()
            < page.find("<pre").unwrap()
    );
}

#[test]
fn authored_reference_overviews_keep_their_content() {
    let root = support::shared_r_workspace();
    root.write("workspace/diplodocus.toml", root.read("workspace/diplodocus.toml") + "\n[[content]]\nid='reference'\nowner='rfoo'\nrepository='r'\npath='docs'\nmount='reference'\nformat='gfm'\n[content.execution]\nmode='never'\n");
    root.write(
        "r/docs/index.md",
        "# Custom reference\n\nStart with [`rfoo::fit`].\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let page = html(&rendered, "packages/r/reference/index.html");
    assert!(page.contains("<h1>Custom reference</h1>"));
    assert!(page.contains("href=\"fit.html\"><code>rfoo::fit</code></a>"));
    assert!(!page.contains("class=\"reference-topics\""));
}
