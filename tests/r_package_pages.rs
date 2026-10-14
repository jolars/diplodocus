mod support;

use std::fs;
use std::future::pending;

use diplodocus::assembly::{AssemblyError, assemble_workspace};
use diplodocus::commands::{CheckOptions, ExtractOptions, check, extract, extract_with};
use diplodocus::diagnostics::{DiagnosticCode, DiagnosticSource};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::ir::{PackageMetadata, ProvenanceActivity};
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{ReferenceTarget, resolve_workspace};

fn fixture() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Models'\n[[repository]]\nid='repo'\npath='.'\n[[package]]\nid='rmodels'\nname='Models for R'\nslug='r'\necosystem='r'\nrepository='repo'\npath='r'\nmetadata-path='DESCRIPTION'\ntargets=[{id='api', extractor='r', path='.', role='public-api'}]\n");
    root.write("r/DESCRIPTION", "Package: models\nVersion: 1.2.0\nTitle: Tools for Comparing Models\nDescription: Compare model predictions reliably.\nAuthors@R: person('Avery', 'Researcher', role=c('aut', 'cre'), email='avery@example.com')\nDate: 2026-01-01\nLicense: MIT\nURL: https://example.org (Project website), https://example.org/community (Community forum)\nBugReports: https://example.org/issues\n");
    root.write("r/NAMESPACE", "");
    root
}

fn add_authored_collection(root: &support::TestWorkspace, path: &str, mount: &str) {
    root.write(
        "diplodocus.toml",
        format!(
            "{}\n[[content]]\nid='r-docs'\nowner='rmodels'\nrepository='repo'\n\
             path='{path}'\nmount='{mount}'\nformat='gfm'\n[content.execution]\nmode='never'\n",
            root.read("diplodocus.toml")
        ),
    );
}

fn repository_settings(root: &support::TestWorkspace, settings: &str) {
    root.write(
        "diplodocus.toml",
        root.read("diplodocus.toml").replace(
            "path='.'\n[[package]]",
            &format!("path='.'\n{settings}\n[[package]]"),
        ),
    );
}

#[test]
fn discovered_package_links_fall_back_to_existing_repository_targets() {
    let root = fixture();
    repository_settings(
        &root,
        "url='https://github.com/example/models.git/'\nrevision='release/v2'",
    );
    root.write("CONTRIBUTING.md", "# Contributing\n\n[Specification](spec/README.md#contract)\n[Trust](TRUST.md)\n[Operations](OPERATIONS.md)\n[Schema](spec/schema.json)\n[Directory](spec/)\n[Encoded](spec/a%20%23%25%C3%A9.md#part%20one)\n[Policy](r/CODE_OF_CONDUCT.md#policy)\n");
    root.write(
        "spec/README.md",
        "<div>Unselected, unsupported markup.</div>\n",
    );
    root.write("TRUST.md", "---\ntitle: [invalid\n---\n");
    root.write("OPERATIONS.md", "# Operations\n");
    root.write("spec/schema.json", "{}\n");
    root.write("spec/a #%é.md", "# Encoded name\n");
    root.write(
        "r/CODE_OF_CONDUCT.md",
        "# Policy\n\n[Trust again](../TRUST.md)\n",
    );
    root.write("r/README.md", "# Models\n\n[Community](../CONTRIBUTING.md)\n[Download](data.csv)\n![Diagram](diagram.svg)\n[Unselected](internal.md)\n");
    root.write("r/internal.md", "# Internal guide\n");
    root.write("r/data.csv", "x\n1\n");
    root.write(
        "r/diagram.svg",
        "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'/>",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(resolved.diagnostics().is_empty());
    assert_eq!(resolved.assets().len(), 2);
    assert!(!resolved.records().iter().flat_map(|d| &d.references).any(|r|
        matches!(&r.target, ReferenceTarget::Repository { path, .. } if path.as_str().contains("CODE_OF_CONDUCT"))));
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let portable = tempfile::tempdir().unwrap();
    let database = portable.path().join("snapshot.sqlite");
    snapshot.publish(&database).unwrap();
    drop((snapshot, resolved, sources, root));
    let loaded = Snapshot::load(database).unwrap();
    let site = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let community = html(&site, "packages/r/community.html");
    let overview = html(&site, "packages/r/index.html");
    for target in [
        "blob/release%2Fv2/spec/README.md#contract",
        "blob/release%2Fv2/TRUST.md",
        "blob/release%2Fv2/OPERATIONS.md",
        "blob/release%2Fv2/spec/schema.json",
        "tree/release%2Fv2/spec",
        "blob/release%2Fv2/spec/a%20%23%25%C3%A9.md#part%20one",
        "blob/release%2Fv2/r/internal.md",
    ] {
        let href = format!("href=\"https://github.com/example/models/{target}\"");
        assert!(
            community.contains(&href) || overview.contains(&href),
            "{target}"
        );
    }
    assert!(community.contains("href=\"community.html#policy\""));
    assert!(overview.contains("href=\"community.html\""));
    assert!(
        !site
            .files()
            .keys()
            .any(|path| path.as_str().contains("TRUST"))
    );
}

#[test]
fn repository_templates_support_other_forges_and_preserve_authored_fragments() {
    let root = fixture();
    repository_settings(
        &root,
        "revision='v2'\nsource-link-template='https://forge.example/models/{kind}/{revision}/{path}#L{line}'",
    );
    root.write(
        "CONTRIBUTING.md",
        "[Policy](TRUST.md#details) [File](TRUST.md) [Directory](spec/)\n",
    );
    root.write("TRUST.md", "# Trust\n");
    root.write("spec/schema.json", "{}\n");
    let site = rendered(&root);
    let community = html(&site, "packages/r/community.html");
    for url in [
        "https://forge.example/models/blob/v2/TRUST.md#details",
        "https://forge.example/models/blob/v2/TRUST.md",
        "https://forge.example/models/tree/v2/spec",
    ] {
        assert!(community.contains(&format!("href=\"{url}\"")), "{url}");
    }
}

#[test]
fn file_templates_do_not_require_a_directory_placeholder() {
    let root = fixture();
    repository_settings(
        &root,
        "revision='release/v2'\nsource-link-template='https://forge.example/models/blob/{revision}/{path}#L{line}'",
    );
    root.write("CONTRIBUTING.md", "[Trust](TRUST.md)\n");
    root.write("TRUST.md", "# Trust\n");
    let site = rendered(&root);
    assert!(
        html(&site, "packages/r/community.html")
            .contains("href=\"https://forge.example/models/blob/release%2Fv2/TRUST.md\"")
    );
    root.write("CONTRIBUTING.md", "[Directory](spec/)\n");
    root.write("spec/schema.json", "{}\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn repository_fallback_uses_the_observed_git_revision() {
    let root = fixture();
    repository_settings(&root, "url='https://github.com/example/models'");
    root.write("CONTRIBUTING.md", "[Trust](TRUST.md)\n");
    root.write("TRUST.md", "# Trust\n");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Package Link Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.org")
            .env("GIT_COMMITTER_NAME", "Package Link Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.org")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--quiet"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--quiet",
        "-m",
        "fixture",
    ]);
    let revision = git(&["rev-parse", "HEAD"]);
    let site = rendered(&root);
    assert!(html(&site, "packages/r/community.html").contains(&format!(
        "href=\"https://github.com/example/models/blob/{revision}/TRUST.md\""
    )));
}

#[test]
fn repository_fallback_requires_a_supported_origin_and_known_revision() {
    for settings in [
        "",
        "url='https://github.com/example/models'",
        "revision='v2'",
        "url='https://forge.example/models'\nrevision='v2'",
        "url='https://github.com/example/models/tree/main'\nrevision='v2'",
        "url='https://user:password@github.com/example/models'\nrevision='v2'",
        "revision='v2'\nsource-link-template='javascript:{revision}/{path}'",
        "revision='v2'\nsource-link-template='https://user:password@forge.example/{revision}/{path}'",
        "revision='v2'\nsource-link-template='https://forge.example/{unknown}/{revision}/{path}'",
        "revision='v2'\nsource-link-template='https://forge.example/main/{path}'",
    ] {
        let root = fixture();
        repository_settings(&root, settings);
        root.write("CONTRIBUTING.md", "[Trust](TRUST.md)\n");
        root.write("TRUST.md", "# Trust\n");
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        assert!(resolve_workspace(&sources).is_err(), "{settings}");
    }
}

#[test]
fn missing_targets_invalid_page_anchors_and_images_do_not_use_repository_fallback() {
    for link in [
        "[Missing](MISSING.md)",
        "[Missing directory](missing/)",
        "[Bad anchor](r/README.md#missing)",
        "![Remote image](TRUST.md)",
        "![Undeclared image](diagram.svg)",
    ] {
        let root = fixture();
        repository_settings(
            &root,
            "url='https://github.com/example/models'\nrevision='v2'",
        );
        root.write("CONTRIBUTING.md", format!("# Contributing\n\n{link}\n"));
        root.write("r/README.md", "# Models\n");
        root.write("TRUST.md", "# Trust\n");
        root.write("diagram.svg", "<svg xmlns='http://www.w3.org/2000/svg'/>");
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let error = resolve_workspace(&sources).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedDocumentReference
                    && d.source.is_some()
                    && d.span.is_some()),
            "{link}"
        );
    }
}

#[test]
fn authored_package_overrides_keep_strict_local_reference_rules() {
    let root = fixture();
    repository_settings(
        &root,
        "url='https://github.com/example/models'\nrevision='v2'",
    );
    root.write("CONTRIBUTING.md", "# Original\n");
    root.write("TRUST.md", "# Trust\n");
    root.write(
        "r/docs/community.md",
        "# Authored community\n\n[Trust](../../TRUST.md)\n",
    );
    add_authored_collection(&root, "r/docs", "");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn repository_targets_are_rechecked_before_snapshot_publication() {
    let root = fixture();
    repository_settings(
        &root,
        "url='https://github.com/example/models'\nrevision='v2'",
    );
    root.write("CONTRIBUTING.md", "[Trust](TRUST.md)\n");
    root.write("TRUST.md", "# Trust\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(
        resolved
            .input_paths()
            .any(|path| path == root.path().join("TRUST.md"))
    );
    root.remove("TRUST.md");
    assert!(resolved.revalidate().is_err());
    assert!(Snapshot::from_sources(&sources, &resolved).is_err());
}

#[cfg(unix)]
#[test]
fn repository_fallback_rejects_symlink_escapes_and_retargeted_aliases() {
    let root = fixture();
    repository_settings(
        &root,
        "url='https://github.com/example/models'\nrevision='v2'",
    );
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("TRUST.md"), "# Outside\n").unwrap();
    std::os::unix::fs::symlink(root.path(), outside.path().join("back")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
    root.write("TRUST.md", "# Trust\n");
    for target in ["outside/TRUST.md", "outside/back/TRUST.md"] {
        root.write("CONTRIBUTING.md", format!("[Trust]({target})\n"));
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        assert!(resolve_workspace(&sources).is_err(), "{target}");
    }
    root.write("CONTRIBUTING.md", "[Trust](alias.md)\n");
    std::os::unix::fs::symlink("TRUST.md", root.path().join("alias.md")).unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(resolved.records().iter().flat_map(|d| &d.references).any(
        |r| matches!(&r.target, ReferenceTarget::Repository { path, .. } if path.as_str() == "TRUST.md")
    ));
    root.remove("alias.md");
    std::os::unix::fs::symlink("outside/TRUST.md", root.path().join("alias.md")).unwrap();
    assert!(resolved.revalidate().is_err());
}

fn rendered(root: &support::TestWorkspace) -> RenderedSite {
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    render_site(&Site::new(&snapshot).unwrap()).unwrap()
}

fn html<'a>(site: &'a RenderedSite, path: &str) -> &'a str {
    std::str::from_utf8(&site.files()[path].bytes).unwrap()
}

fn html_text(html: &str) -> String {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    fn append(node: &Handle, text: &mut String) {
        if let NodeData::Text { contents } = &node.data {
            text.push_str(&contents.borrow());
        }
        for child in node.children.borrow().iter() {
            append(child, text);
        }
    }

    let document = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut text = String::new();
    append(&document.document, &mut text);
    text
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
    assert!(overview.contains("news.html#changes"));
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
    add_authored_collection(&root, "r/docs", "");
    let site = rendered(&root);
    assert!(html(&site, "packages/r/index.html").contains("An authored introduction."));
    assert!(!html(&site, "packages/r/index.html").contains("Ignored homepage"));
    assert!(html(&site, "packages/r/index.html").contains("Package information"));
    assert!(html(&site, "packages/r/news.html").contains("My release notes"));

    let root = fixture();
    root.write("r/README.md", "# My README\n");
    add_authored_collection(&root, "r", "guide");
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
fn community_override_skips_unused_guide_syntax_and_preserves_source_links() {
    let root = fixture();
    root.write(
        "CONTRIBUTING.md",
        include_str!("fixtures/r-package-pages/CONTRIBUTING.md"),
    );
    root.write(
        "r/CODE_OF_CONDUCT.md",
        "<div>Unused conduct policy.</div>\n",
    );
    root.write(
        "r/README.md",
        "# Models\n\n[Contribute](../CONTRIBUTING.md)\n\n[Policy](CODE_OF_CONDUCT.md)\n",
    );
    root.write(
        "r/docs/community.md",
        "# Community\n\nMaintained community page.\n",
    );
    add_authored_collection(&root, "r/docs", "");

    let report = check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let site = rendered(&root);
    let overview = html(&site, "packages/r/index.html");
    assert!(overview.contains("href=\"community.html\">Contribute</a>"));
    assert!(overview.contains("href=\"community.html\">Policy</a>"));
    let community = html(&site, "packages/r/community.html");
    assert!(community.contains("Maintained community page."));
    assert!(!community.contains("Unsupported content"));
    assert!(!community.contains("Unused conduct policy"));

    root.write(
        "r/docs/community.md",
        "# Community\n\n<div>Authored unsupported content.</div>\n",
    );
    let report = check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code,
        DiagnosticCode::UnsupportedAuthoredSyntax
    );
    assert!(matches!(
        &report.diagnostics[0].source,
        Some(DiagnosticSource::Repository { path, .. }) if path.as_str() == "r/docs/community.md"
    ));
}

#[test]
fn overrides_skip_invalid_overview_news_and_license_documents() {
    for (route, source) in [
        ("index", "README.md"),
        ("news", "NEWS.md"),
        ("license", "LICENSE.md"),
    ] {
        let root = fixture();
        root.write(
            format!("r/{source}"),
            "---\ntitle: [broken\n---\n\n<div>Unused content.</div>\n",
        );
        root.write(
            format!("r/docs/{route}.md"),
            format!("# Maintained {route}\n\n[Original source](../{source})\n"),
        );
        add_authored_collection(&root, "r/docs", "");
        let report = check(CheckOptions {
            config: root.path().join("diplodocus.toml"),
        })
        .unwrap();
        assert!(
            report.diagnostics.is_empty(),
            "{route}: {:?}",
            report.diagnostics
        );
        let site = rendered(&root);
        let page = html(&site, &format!("packages/r/{route}.html"));
        assert!(page.contains(&format!("Maintained {route}")));
        assert!(page.contains(&format!("href=\"{route}.html\"")));
        assert!(!page.contains("Unused content"));

        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        root.write(format!("r/{source}"), [0xff]);
        assert!(matches!(
            sources.revalidate(),
            Err(AssemblyError::InputsChanged)
        ));
        assert!(
            check(CheckOptions {
                config: root.path().join("diplodocus.toml"),
            })
            .unwrap()
            .diagnostics
            .is_empty()
        );
        root.remove(format!("r/docs/{route}.md"));
        root.write(format!("r/{source}"), "---\ntitle: [broken\n---\n");
        let error = check(CheckOptions {
            config: root.path().join("diplodocus.toml"),
        })
        .unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidEmbeddedYaml)
        );
    }
}

#[test]
fn explicitly_included_original_documents_still_report_unsupported_syntax() {
    let root = fixture();
    root.write("r/README.md", "<div>Included README content.</div>\n");
    root.write("r/index.md", "# Maintained homepage\n");
    add_authored_collection(&root, "r", "");
    let report = check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    assert_eq!(report.diagnostics.len(), 1);
    assert!(matches!(
        &report.diagnostics[0].source,
        Some(DiagnosticSource::Repository { path, .. }) if path.as_str() == "r/README.md"
    ));
    let site = rendered(&root);
    assert!(html(&site, "packages/r/index.html").contains("Maintained homepage"));
    assert!(html(&site, "packages/r/README.html").contains("Included README content."));
}

#[test]
fn authored_news_skips_unused_rd_parsing_and_native_conversion() {
    let root = fixture();
    root.write("r/NEWS.Rd", "\\section{Unclosed release notes}{");
    root.write(
        "r/docs/news.md",
        "# Maintained news\n\n[Original source](../NEWS.Rd)\n",
    );
    add_authored_collection(&root, "r/docs", "");
    check(CheckOptions {
        config: root.path().join("diplodocus.toml"),
    })
    .unwrap();
    let snapshot = completed(&root);
    let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let news = html(&site, "packages/r/news.html");
    assert!(news.contains("Maintained news"));
    assert!(news.contains("href=\"news.html\""));
    assert!(!news.contains("Unclosed release notes"));
    root.remove("r/docs/news.md");
    assert!(
        check(CheckOptions {
            config: root.path().join("diplodocus.toml"),
        })
        .is_err()
    );
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
    assert!(overview.contains("href=\"license.html\""));
    assert!(overview.contains("href=\"community.html\""));
    let community = html(&site, "packages/r/community.html");
    assert!(community.contains("Code of conduct"));
    assert!(community.contains("href=\"index.html\""));
    assert!(community.contains("href=\"community.html\""));
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
    // Syntax highlighting can split citation tokens across HTML elements.
    let authors_text = html_text(authors);
    for text in [
        "First citation",
        "Second citation",
        "@Manual",
        "@Article",
        "Avery Researcher",
    ] {
        assert!(authors_text.contains(text), "{text}");
    }
    assert!(authors.contains("id=\"citation\""));
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
    root.write("r/docs/index.md", "# Maintained homepage\n");
    root.write("r/docs/license.md", "# Maintained license\n");
    add_authored_collection(&root, "r/docs", "");
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

#[test]
fn conventional_conduct_filename_resolves_from_contribution_guidelines() {
    let root = fixture();
    root.write(
        "r/CONTRIBUTING.md",
        "# Contributing\n\n[Policy](CONDUCT.md)\n",
    );
    root.write("r/CONDUCT.md", "# Conduct\n\nCommunity policy.\n");
    let site = rendered(&root);
    let community = html(&site, "packages/r/community.html");
    assert!(community.contains("Community policy."));
    assert!(community.contains("href=\"community.html\""));
}
