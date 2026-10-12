mod support;

use std::fs;
use std::future::pending;

use diplodocus::assembly::{AssemblyError, assemble_workspace};
use diplodocus::commands::{CheckOptions, ExtractOptions, check, extract, extract_with};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::ir::{PackageMetadata, ProvenanceActivity};
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn fixture() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Models'\n[[repository]]\nid='repo'\npath='.'\n[[package]]\nid='rmodels'\nname='Models for R'\nslug='r'\necosystem='r'\nrepository='repo'\npath='r'\nmetadata-path='DESCRIPTION'\ntargets=[{id='api', extractor='r', path='.', role='public-api'}]\n");
    root.write("r/DESCRIPTION", "Package: models\nVersion: 1.2.0\nTitle: Tools for Comparing Models\nDescription: Compare model predictions reliably.\nAuthors@R: person('Avery', 'Researcher', role=c('aut', 'cre'), email='avery@example.com')\nDate: 2026-01-01\nLicense: MIT\nURL: https://example.org (Project website), https://example.org/community (Community forum)\nBugReports: https://example.org/issues\n");
    root.write("r/NAMESPACE", "");
    root
}

fn rendered(root: &support::TestWorkspace) -> RenderedSite {
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    render_site(&Site::new(&snapshot).unwrap()).unwrap()
}

fn html<'a>(site: &'a RenderedSite, path: &str) -> &'a str {
    std::str::from_utf8(&site.files()[path].bytes).unwrap()
}

fn completed(root: &support::TestWorkspace) -> Snapshot {
    extract(ExtractOptions {
        config: root.path().join("diplodocus.toml"),
        output: Some(root.path().join("snapshot.sqlite")),
    })
    .unwrap();
    Snapshot::load(root.path().join("snapshot.sqlite")).unwrap()
}

#[test]
fn description_supplies_overview_and_metadata_without_optional_files() {
    let root = fixture();
    let site = rendered(&root);
    let overview = html(&site, "packages/r/index.html");
    assert_eq!(overview.matches("<h1").count(), 1);
    for text in [
        "Models for R",
        "Tools for Comparing Models",
        "Compare model predictions reliably.",
        "1.2.0",
        "MIT",
        "Project website",
        "Community forum",
        "Report an issue",
    ] {
        assert!(overview.contains(text), "{text}");
    }
    assert!(!site.files().contains_key("packages/r/news.html"));
    assert!(!site.files().contains_key("packages/r/community.html"));
    let search = html(&site, "assets/search.json");
    assert!(search.contains("Compare model predictions reliably."));
}

#[test]
fn readme_links_assets_headings_and_news_are_portable() {
    let root = fixture();
    root.write("r/README.md", "# A different README title\n\n![Diagram](diagram.svg)\n\n[Release notes](NEWS.md#changes)\n\n# Another heading\n");
    root.write("r/NEWS.md", "# Changes\n\nNew prediction metric.\n");
    root.write("r/NEWS", "Old plain release notes.");
    root.write("r/diagram.svg", "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'><circle cx='6' cy='6' r='4'/></svg>");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let portable = tempfile::tempdir().unwrap();
    let database = portable.path().join("snapshot.sqlite");
    snapshot.publish(&database).unwrap();
    assert_eq!(snapshot.assets().len(), 1);
    let expected = snapshot.canonical_export().unwrap();
    drop((snapshot, sources, root));
    let loaded = Snapshot::load(&database).unwrap();
    assert_eq!(loaded.canonical_export().unwrap(), expected);
    let site = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let overview = html(&site, "packages/r/index.html");
    assert_eq!(overview.matches("<h1").count(), 1);
    assert!(overview.contains(">Models for R</h1>"));
    assert!(overview.contains("news%2Ehtml#changes"));
    assert!(overview.contains("<img "));
    assert!(html(&site, "packages/r/news.html").contains("New prediction metric."));
    assert!(!html(&site, "packages/r/news.html").contains("Old plain release notes."));
}

#[test]
fn authored_pages_override_routes_and_discovered_sources_are_not_duplicated() {
    let root = fixture();
    root.write(
        "r/docs/index.md",
        "# My package homepage\n\nAn authored introduction.\n",
    );
    root.write("r/docs/news.md", "# My release notes\n");
    root.write("r/README.md", "# Ignored homepage\n\n[News](NEWS.md)\n");
    root.write("r/NEWS.md", "# Automatically discovered news\n");
    root.write("diplodocus.toml", format!("{}\n[[content]]\nid='r-docs'\nowner='rmodels'\nrepository='repo'\npath='r/docs'\nmount=''\nformat='gfm'\n[content.execution]\nmode='never'\n", root.read("diplodocus.toml")));
    let site = rendered(&root);
    assert!(html(&site, "packages/r/index.html").contains("An authored introduction."));
    assert!(!html(&site, "packages/r/index.html").contains("Ignored homepage"));
    assert!(html(&site, "packages/r/index.html").contains("Package information"));
    assert!(html(&site, "packages/r/news.html").contains("My release notes"));

    let root = fixture();
    root.write("r/README.md", "# My README\n");
    root.write("diplodocus.toml", format!("{}\n[[content]]\nid='r-docs'\nowner='rmodels'\nrepository='repo'\npath='r'\nmount='guide'\nformat='gfm'\n[content.execution]\nmode='never'\n", root.read("diplodocus.toml")));
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert_eq!(
        sources
            .workspace()
            .pages
            .values()
            .filter(|page| page
                .document
                .source_location
                .as_ref()
                .is_some_and(|source| source.path.as_str() == "r/README.md"))
            .count(),
        1
    );
    resolve_workspace(&sources).unwrap();
}

#[test]
fn community_fallbacks_rebase_links_and_license_sources_resolve_to_detail_pages() {
    let root = fixture();
    root.write(
        "CONTRIBUTING.md",
        "# Contributing\n\n[Policy](r/CODE_OF_CONDUCT.md)\n",
    );
    root.write(
        "r/CODE_OF_CONDUCT.md",
        "# Code of conduct\n\n[Overview](README.md)\n",
    );
    root.write("r/LICENSE", "Copyright 2026 Avery Researcher");
    root.write(
        "r/README.md",
        "# Models\n\n[License](LICENSE)\n[Contribute](../CONTRIBUTING.md)\n",
    );
    let site = rendered(&root);
    let overview = html(&site, "packages/r/index.html");
    assert!(overview.contains("href=\"license%2Ehtml\""));
    assert!(overview.contains("href=\"community%2Ehtml\""));
    let community = html(&site, "packages/r/community.html");
    assert!(community.contains("Code of conduct"));
    assert!(community.contains("href=\"index%2Ehtml\""));
    assert!(community.contains("href=\"community%2Ehtml\""));
}

#[test]
fn revalidation_detects_new_files_and_edits_and_hidden_packages_stay_out_of_search() {
    let root = fixture();
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    root.write("r/README.md", "# New overview\n");
    assert!(matches!(
        sources.revalidate(),
        Err(AssemblyError::InputsChanged)
    ));
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    root.write("r/README.md", "# Edited overview\n");
    assert!(matches!(
        sources.revalidate(),
        Err(AssemblyError::InputsChanged)
    ));
    root.write(
        "diplodocus.toml",
        root.read("diplodocus.toml")
            .replace("ecosystem='r'", "ecosystem='r'\nvisibility='hidden'"),
    );
    let site = rendered(&root);
    assert!(site.files().contains_key("packages/r/index.html"));
    assert!(!html(&site, "assets/search.json").contains("Models for R"));
    assert!(!html(&site, "index.html").contains("packages/r/"));
}

#[test]
fn checks_never_evaluate_authors_or_citations_and_extraction_failures_preserve_snapshots() {
    let root = fixture();
    let before = completed(&root).canonical_export().unwrap();
    root.write(
        "r/inst/CITATION",
        "writeLines('executed', 'marker')\nstop('citation failed')\n",
    );
    check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    assert!(!root.path().join("r/marker").exists());
    assert!(
        extract(ExtractOptions {
            config: root.path().join("diplodocus.toml"),
            output: Some(root.path().join("snapshot.sqlite"))
        })
        .is_err()
    );
    assert!(root.path().join("r/marker").exists());
    assert_eq!(
        Snapshot::load(root.path().join("snapshot.sqlite"))
            .unwrap()
            .canonical_export()
            .unwrap(),
        before
    );

    root.remove("r/inst/CITATION");
    root.write(
        "r/DESCRIPTION",
        root.read("r/DESCRIPTION").replace(
            "person('Avery', 'Researcher', role=c('aut', 'cre'), email='avery@example.com')",
            "{ writeLines('authors', 'author-marker'); person('Avery', 'Researcher') }",
        ),
    );
    check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    assert!(!root.path().join("r/author-marker").exists());
    completed(&root);
    assert!(root.path().join("r/author-marker").exists());
}

#[test]
fn native_authors_multiple_citations_and_description_fallback_are_retained() {
    let root = fixture();
    let fallback = completed(&root);
    let Some(PackageMetadata::R(metadata)) = &fallback.workspace().packages["rmodels"].metadata
    else {
        panic!("R metadata")
    };
    let runtime = metadata.runtime.as_ref().unwrap();
    assert!(runtime.authors[0].contains("Avery Researcher <avery@example.com> [aut, cre]"));
    assert!(
        runtime
            .citation
            .as_ref()
            .unwrap()
            .bibtex
            .contains("year = {2026}")
    );
    root.write("r/inst/CITATION", "bibentry('Manual', title='First citation', author=person('Avery', 'Researcher'), year='2026')\nbibentry('Article', title='Second citation', author=person('Another', 'Author'), journal='Models', year='2025')\n");
    let snapshot = completed(&root);
    let export = snapshot.canonical_export().unwrap();
    assert!(!export.contains(root.path().to_str().unwrap()));
    assert!(
        snapshot
            .workspace()
            .provenance
            .iter()
            .any(|record| matches!(record.activity, ProvenanceActivity::PackageMetadata { .. }))
    );
    let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let authors = html(&site, "packages/r/authors.html");
    for text in [
        "First citation",
        "Second citation",
        "@Manual",
        "@Article",
        "Avery Researcher",
        "id=\"citation\"",
    ] {
        assert!(authors.contains(text), "{text}");
    }
}

#[test]
fn plain_news_and_rd_news_use_the_supported_fallback_order() {
    let root = fixture();
    root.write(
        "r/NEWS",
        "Changes in models 1.2.0\n\n* Improve comparisons.\n",
    );
    assert!(html(&rendered(&root), "packages/r/news.html").contains("Improve comparisons."));
    root.write("r/NEWS.Rd", "\\name{NEWS}\n\\title{Models release notes}\n\\section{Changes in version 1.2.0}{\\itemize{\\item Improve RD release notes.}}\n");
    let snapshot = completed(&root);
    let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert!(html(&site, "packages/r/news.html").contains("Improve RD release notes."));
    assert!(!html(&site, "packages/r/news.html").contains("Improve comparisons."));
}

#[tokio::test]
async fn native_metadata_timeout_and_cancellation_leave_no_snapshot() {
    let root = fixture();
    root.write("r/inst/CITATION", "Sys.sleep(30)\n");
    let options = || ExtractOptions {
        config: root.path().join("diplodocus.toml"),
        output: Some(root.path().join("snapshot.sqlite")),
    };
    let result = extract_with(
        options(),
        ExecutionDeadlines {
            cell: 50,
            ..ExecutionDeadlines::default()
        },
        Box::pin(pending()),
    )
    .await;
    assert!(
        result
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("timed out"))
    );
    assert!(!root.path().join("snapshot.sqlite").exists());
    let result = extract_with(options(), ExecutionDeadlines::default(), Box::pin(async {})).await;
    assert!(matches!(
        result,
        Err(diplodocus::commands::CommandError::Assembly(
            AssemblyError::Cancelled
        ))
    ));
    assert!(!root.path().join("snapshot.sqlite").exists());
}

#[cfg(unix)]
#[test]
fn package_documentation_rejects_symlinks_outside_the_package() {
    let root = fixture();
    root.write("outside.md", "# Outside the declared package\n");
    std::os::unix::fs::symlink(
        root.path().join("outside.md"),
        root.path().join("r/README.md"),
    )
    .unwrap();
    assert!(assemble_workspace(root.path().join("diplodocus.toml")).is_err());
    fs::remove_file(root.path().join("r/README.md")).unwrap();
    root.write(
        "r/DESCRIPTION",
        root.read("r/DESCRIPTION")
            .replace("License: MIT", "License: file ../outside.md"),
    );
    assert!(assemble_workspace(root.path().join("diplodocus.toml")).is_err());
}
