mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::configuration::load_configuration;
use diplodocus::diagnostics::Severity;
use diplodocus::extractors::r::{RExtraction, extract_target};
use diplodocus::ir::{Block, Inline, PackageMetadata, TableAlignment};
use diplodocus::paths::resolve_workspace_paths;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{ReferenceTarget, resolve_workspace};

fn fixture() -> support::TestWorkspace {
    let workspace = support::acceptance_workspace();
    support::add_r_rd_content(&workspace);
    workspace
}

fn extract(workspace: &support::TestWorkspace) -> RExtraction {
    let path = workspace.path().join("workspace/diplodocus.yaml");
    let config = load_configuration(&path).unwrap();
    let paths = resolve_workspace_paths(&path, &config).unwrap();
    let package = paths
        .packages
        .iter()
        .find(|package| package.id == "rfoo")
        .unwrap();
    extract_target(
        &paths.repositories[package.repository_index],
        package,
        &package.targets[0],
    )
}

fn replace_topic(workspace: &support::TestWorkspace, content: &str) {
    workspace.write("r/man/rd_content.Rd", format!("\\name{{rd_content}}\\alias{{rd_content}}\\title{{Rd content}}\\usage{{rd_content(x)}}{content}"));
}

#[test]
fn eulerr_content_retains_equations_tables_sections_and_named_values() {
    let workspace = fixture();
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error
                || diagnostic.code.as_str() == "unsupported-rd"),
        "{:?}",
        result.diagnostics
    );
    let item = result
        .items
        .values()
        .find(|item| item.name == "rd_content")
        .unwrap();
    let doc = item.documentation.as_ref().unwrap();
    assert_eq!(
        doc.raw_source.as_deref(),
        Some(workspace.read("r/man/rd_content.Rd").as_str())
    );
    assert_eq!(
        doc.document
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::DisplayMath { .. }))
            .count(),
        4
    );
    let equation = doc
        .document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DisplayMath {
                latex, fallback, ..
            } => Some((latex, fallback.as_ref().unwrap())),
            _ => None,
        })
        .unwrap();
    assert!(equation.0.contains("\\sum_{i=1}^n"));
    assert!(equation.1.contains("\\sum (A_i - \\omega_i)^2"));
    let table = doc
        .document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table {
                alignments, rows, ..
            } => Some((alignments, rows)),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.0.len(), 9);
    assert_eq!(table.0[0], TableAlignment::Left);
    assert!(
        table.0[1..]
            .iter()
            .all(|alignment| *alignment == TableAlignment::Center)
    );
    assert!(
        table
            .1
            .iter()
            .all(|row| !row.header && row.cells.len() == 9)
    );
    let json = serde_json::to_string(&doc.document).unwrap();
    for expected in [
        "original.values",
        "regionError",
        "Graphical parameters",
        "grid::unit()",
        "HTML-only content.",
        "HTML alternative.",
    ] {
        assert!(json.contains(expected), "{expected}");
    }
    assert!(!json.contains("Text-only content."));
    assert_eq!(result.help_links["as.numeric"][0].topic, "numeric");
    assert_eq!(result.help_links["grid::gpar"][0].package, "grid");
}

#[test]
fn rd_conditionals_select_sections_inline_content_and_display_code() {
    let workspace = fixture();
    replace_topic(
        &workspace,
        r"\description{Before \if{html,latex}{selected \eqn{x}} \ifelse{FALSE}{\link{missing}}{alternate} after.
\if{TRUE}{always}\if{latex,text}{\Sexpr{stop('inactive')}}}
\if{html}{\section{Selected}{A section.}}
#ifdef unix
\section{Unix}{Unix content.}
#endif
#ifdef windows
\section{Windows}{Windows content.}
#endif
\examples{\ifelse{html}{rd_content(1)}{stop('inactive')}}",
    );
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error
                || diagnostic.code.as_str() == "unsupported-rd"),
        "{:?}",
        result.diagnostics
    );
    let doc = result
        .items
        .values()
        .find(|item| item.name == "rd_content")
        .unwrap()
        .documentation
        .as_ref()
        .unwrap();
    let json = serde_json::to_string(&doc.document).unwrap();
    for expected in [
        "selected",
        "alternate",
        "always",
        "Selected",
        "rd_content(1)",
    ] {
        assert!(json.contains(expected));
    }
    for excluded in ["missing", "inactive"] {
        assert!(!json.contains(excluded));
    }
    let diplodocus::ir::ProvenanceActivity::Extraction { parsers, .. } =
        &result.provenance.activity
    else {
        panic!()
    };
    let os = &parsers["rd-rds"].settings["os-type"];
    assert!(json.contains(if os == "unix" {
        "Unix content."
    } else {
        "Windows content."
    }));
    assert!(!json.contains(if os == "unix" {
        "Windows content."
    } else {
        "Unix content."
    }));
}

#[test]
fn rd_help_labels_local_aliases_and_external_urls_survive_snapshot_loading() {
    let workspace = fixture();
    replace_topic(
        &workspace,
        r"\description{\link{rd_content} \link[=rd_content]{\emph{friendly label}} \code{\link[foo:rd_content]{rd_content()}} \link[base]{as.numeric} \link[grid:gpar]{graphical parameters} \link{data.frame} \doi{10.1000/a?b#c}}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let item = snapshot.workspace().packages["rfoo"]
        .items
        .values()
        .find(|item| item.name == "rd_content")
        .unwrap();
    let document = item.documentation.as_ref().unwrap();
    assert!(document.document.blocks.iter().any(|block| matches!(block, Block::Paragraph { inlines, .. } if inlines.iter().any(|inline| matches!(inline, Inline::Link { target, .. } if target == "@ref foo::rd_content")))));
    let references: Vec<_> = snapshot
        .documents()
        .iter()
        .flat_map(|document| &document.references)
        .filter(|reference| reference.spelling.starts_with("@ref "))
        .collect();
    assert!(references.iter().any(|reference| reference.spelling == "@ref foo::rd_content" && matches!(&reference.target, ReferenceTarget::Item { item } if item.package == "rfoo")));
    for url in [
        "https://rdrr.io/r/base/numeric.html",
        "https://rdrr.io/r/grid/gpar.html",
        "https://rdrr.io/r/base/data.frame.html",
    ] {
        assert!(references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { url: actual } if actual == url)), "{url}");
    }
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let export = snapshot.canonical_export().unwrap();
    drop(sources);
    drop(workspace);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.canonical_export().unwrap(), export);
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let html = rendered
        .files()
        .values()
        .filter(|file| file.media_type.starts_with("text/html"))
        .map(|file| std::str::from_utf8(&file.bytes).unwrap())
        .find(|html| html.contains("<p class=\"api-name\"><code>rd_content</code></p>"))
        .unwrap();
    assert!(html.contains("><em>friendly label</em></a>"));
    assert!(html.contains("><code>rd_content()</code></a>"));
    assert!(html.contains("https://doi.org/10.1000/a%3Fb%23c"));
}

#[test]
fn local_help_wins_over_installed_topics_and_missing_links_fail() {
    let workspace = fixture();
    workspace.write("r/man/local.Rd", "\\name{local}\\alias{data.frame}\\docType{data}\\title{Local data}\\usage{data.frame}\\description{Local topic.}");
    replace_topic(&workspace, r"\description{\link{data.frame}}");
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(
        resolved
            .records()
            .iter()
            .flat_map(|document| &document.references)
            .any(|reference| reference.spelling == "@ref data.frame"
                && matches!(&reference.target, ReferenceTarget::Item { .. }))
    );
    replace_topic(
        &workspace,
        r"\description{\link[=no_such_help_topic]{missing}}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn malformed_rd_shapes_are_diagnosed_without_discarding_source() {
    for content in [
        r"\description{\link[=]{empty}}",
        r"\details{\tabular{lx}{one\tab two\cr}}",
        r"\description{\doi{not-a-doi}}",
    ] {
        let workspace = fixture();
        replace_topic(&workspace, content);
        let result = extract(&workspace);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error),
            "{content}: {:?}",
            result.diagnostics
        );
        assert!(result.inputs.values().any(|input| {
            input
                .text
                .as_ref()
                .is_some_and(|text| text.contains(content))
        }));
    }
}

#[test]
fn math_and_help_evidence_render_without_the_source_workspace() {
    let workspace = fixture();
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let Some(PackageMetadata::R(metadata)) = &snapshot.workspace().packages["rfoo"].metadata else {
        panic!()
    };
    assert!(!metadata.help_links.is_empty());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(workspace);
    let loaded = Snapshot::load(&path).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let generated = directory.path().join("site");
    let unavailable_runtime = directory.path().join("empty-path");
    std::fs::create_dir(&unavailable_runtime).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["generate", "--input"])
        .arg(&path)
        .arg("--output")
        .arg(&generated)
        .env("PATH", &unavailable_runtime)
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(generated.join("assets/katex/katex.min.css").is_file());
    assert!(rendered.files().contains_key("assets/katex/katex.min.css"));
    assert!(!rendered.files().contains_key("assets/katex/katex.min.js"));
    assert!(
        rendered
            .files()
            .contains_key("assets/katex/fonts/KaTeX_Main-Regular.woff2")
    );
    let html = rendered
        .files()
        .values()
        .filter(|file| file.media_type.starts_with("text/html"))
        .map(|file| std::str::from_utf8(&file.bytes).unwrap())
        .find(|html| html.contains("<p class=\"api-name\"><code>rd_content</code></p>"))
        .unwrap();
    assert_eq!(
        html.matches("class=\"math math-rendered math-display\"")
            .count(),
        4
    );
    assert!(html.contains("class=\"math math-rendered\""));
    assert!(html.contains("class=\"align-center\""));
    assert!(html.contains(">Links</h3>"));
    assert!(!html.contains("[Unsupported Rd:"));
    assert!(
        !std::str::from_utf8(&rendered.files()["index.html"].bytes)
            .unwrap()
            .contains("katex.min.js")
    );
}

#[test]
fn invalid_and_untrusted_equations_have_readable_escaped_output() {
    let workspace = fixture();
    replace_topic(
        &workspace,
        r"\description{\eqn{\unknowncommand{x}}{<fallback>}}
\details{\deqn{\href{javascript:alert(1)}{unsafe}}{unsafe fallback}
\deqn{\htmlClass{injected}{x}}{class fallback}}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let html = rendered
        .files()
        .values()
        .filter(|file| file.media_type.starts_with("text/html"))
        .map(|file| std::str::from_utf8(&file.bytes).unwrap())
        .find(|html| html.contains("<p class=\"api-name\"><code>rd_content</code></p>"))
        .unwrap();
    assert!(html.contains("&lt;fallback&gt;"));
    assert!(html.contains("math-fallback"));
    assert!(!html.contains("href=\"javascript:"));
    assert!(!html.contains("class=\"injected\""));
}

fn add_peer(workspace: &support::TestWorkspace, name: &str) {
    workspace.write(
        format!("{name}/DESCRIPTION"),
        workspace
            .read("r/DESCRIPTION")
            .replace("Package: foo", &format!("Package: {name}")),
    );
    workspace.write(format!("{name}/NAMESPACE"), "export(other)\n");
    workspace.write(format!("{name}/LICENSE"), workspace.read("r/LICENSE"));
    workspace.write(format!("{name}/R/other.R"), "other <- function(x) x\n");
    workspace.write(format!("{name}/man/other.Rd"), "\\name{other}\\alias{other}\\alias{peer_alias}\\title{Peer}\\usage{other(x)}\\description{Peer documentation.}");
    let mut configuration = support::fixture_configuration(workspace, "workspace/diplodocus.yaml");
    configuration["repositories"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": name, "path": format!("../{name}")
        }));
    configuration["packages"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": format!("{name}-workspace"), "name": name, "slug": name, "ecosystem": "r",
            "repository": name, "path": ".", "metadata-path": "DESCRIPTION",
            "targets": [{"id": "api", "extractor": "r", "path": ".", "role": "public-api"}]
        }));
    workspace.write(
        "workspace/diplodocus.yaml",
        serde_saphyr::to_string(&configuration).unwrap(),
    );
}

#[test]
fn published_package_qualifiers_and_unique_workspace_aliases_resolve_locally() {
    let workspace = fixture();
    add_peer(&workspace, "peer");
    replace_topic(
        &workspace,
        r"\description{\link[peer:peer_alias]{Peer label} and \link{peer_alias}.}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let references: Vec<_> = resolved
        .records()
        .iter()
        .flat_map(|record| &record.references)
        .filter(|reference| {
            matches!(
                reference.spelling.as_str(),
                "@ref peer::peer_alias" | "@ref peer_alias"
            )
        })
        .collect();
    assert_eq!(references.len(), 2);
    assert!(references.iter().all(|reference| matches!(&reference.target, ReferenceTarget::Item { item } if item.package == "peer-workspace")));
    replace_topic(
        &workspace,
        r"\description{\link[peer:no_such_alias]{Missing peer topic}.}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn ambiguous_workspace_help_aliases_require_a_package_qualifier() {
    let workspace = fixture();
    add_peer(&workspace, "peer");
    add_peer(&workspace, "another");
    replace_topic(&workspace, r"\description{\link{peer_alias}}");
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let error = resolve_workspace(&sources).unwrap_err();
    assert!(error.diagnostics().iter().any(|diagnostic| diagnostic.code
        == diplodocus::diagnostics::DiagnosticCode::AmbiguousItemReference));
    replace_topic(
        &workspace,
        r"\description{\link[another:peer_alias]{Another package}}",
    );
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_ok());
}

#[test]
fn grouped_nested_lists_retain_block_structure() {
    let workspace = fixture();
    replace_topic(
        &workspace,
        r"\description{\describe{\item{Options}{Before {\itemize{\item First\item Second {\enumerate{\item Nested}}}} After.}}}",
    );
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error || d.code.as_str() == "unsupported-rd"),
        "{:?}",
        result.diagnostics
    );
    let doc = result
        .items
        .values()
        .find(|item| item.name == "rd_content")
        .unwrap()
        .documentation
        .as_ref()
        .unwrap();
    fn lists(blocks: &[Block], counts: &mut Vec<(bool, usize)>) {
        for block in blocks {
            if let Block::List { ordered, items, .. } = block {
                counts.push((*ordered, items.len()));
                for item in items {
                    lists(&item.blocks, counts);
                }
            }
        }
    }
    let mut counts = vec![];
    lists(&doc.document.blocks, &mut counts);
    assert_eq!(counts, vec![(false, 1), (false, 2), (true, 1)]);
    let encoded = serde_json::to_string(&doc.document).unwrap();
    for text in ["Before", "First", "Second", "Nested", "After."] {
        assert!(encoded.contains(text));
    }
}

#[test]
fn constants_and_deprecated_aliases_survive_snapshot_and_link_resolution() {
    let workspace = fixture();
    workspace.write("r/R/constants.R", "PAD <- 3");
    workspace.write("r/man/PAD.Rd", r"\name{PAD}\alias{PAD}\title{Padding}\usage{PAD}\description{Padding in points.}\keyword{internal}");
    workspace.write("r/man/deprecated.Rd", r"\name{foo_deprecated}\alias{foo_deprecated}\alias{old_api}\title{Deprecated APIs}\description{Use the new API.}\keyword{internal}");
    replace_topic(&workspace, r"\description{\link{PAD} and \link{old_api}.}");
    let sources = assemble_workspace(workspace.path().join("workspace/diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    for spelling in ["@ref PAD", "@ref old_api"] {
        assert!(resolved.records().iter().flat_map(|record| &record.references).any(|reference| reference.spelling == spelling && matches!(&reference.target, ReferenceTarget::Item { item } if item.package == "rfoo")));
    }
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let expected = snapshot.canonical_export().unwrap();
    drop(workspace);
    drop(sources);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.canonical_export().unwrap(), expected);
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let pages: Vec<_> = rendered
        .files()
        .values()
        .filter(|file| file.media_type.starts_with("text/html"))
        .map(|file| std::str::from_utf8(&file.bytes).unwrap())
        .collect();
    assert!(pages.iter().any(|html| html.contains("Padding in points.")));
    assert!(pages.iter().any(|html| html.contains("Use the new API.")));
}

#[test]
fn described_lists_preserve_interstitial_prose_and_nested_content() {
    let workspace = fixture();
    replace_topic(
        &workspace,
        r"\description{\describe{\item{First}{Before \itemize{\item Nested}.}Between.\item{Second}{After.}}}",
    );
    let result = extract(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error || d.code.as_str() == "unsupported-rd"),
        "{:?}",
        result.diagnostics
    );
    let doc = result
        .items
        .values()
        .find(|item| item.name == "rd_content")
        .unwrap()
        .documentation
        .as_ref()
        .unwrap();
    let encoded = serde_json::to_string(&doc.document).unwrap();
    let positions: Vec<_> = ["First", "Before", "Nested", "Between.", "Second", "After."]
        .iter()
        .map(|text| encoded.find(text).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
