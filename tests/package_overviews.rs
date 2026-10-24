mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::{PackageMetadata, PackagePageRole, PageKind};
use diplodocus::rendering::{RenderedSite, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn fixture(language: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    let (manifest, target) = match language {
        "python" => {
            root.write("pkg/pyproject.toml", "[project]\nname = 'models'\nversion = '1.2.0'\ndescription = 'Compare predictions reliably.'\nlicense = 'MIT'\nauthors = [{ name = 'Model Authors' }]\nmaintainers = [{ name = 'Model Maintainer', email = 'maintainer@example.com' }]\n[project.urls]\nWebsite = 'https://example.com/models'\nIssues = 'https://example.com/issues'\nUnsafe = 'javascript:alert(1)'\n");
            root.write("pkg/src/models/__init__.py", "def predict(value: float) -> float:\n    \"\"\"Return a prediction.\"\"\"\n    return value\n");
            ("pyproject.toml", "src/models")
        }
        "julia" => {
            root.write("pkg/Project.toml", "name = 'Models'\nuuid = '18f44767-5045-4ec0-91b2-2113d84f2d30'\nversion = '1.2.0'\nauthors = ['Model Authors']\n");
            root.write("pkg/src/Models.jl", "module Models\nexport predict\n\"\"\"Return a prediction.\"\"\"\npredict(value::Float64) = value\nend\n");
            ("Project.toml", "src/Models.jl")
        }
        _ => unreachable!(),
    };
    root.write("diplodocus.yaml", format!("project:\n  name: Models\nrepositories:\n  - id: repo\n    path: .\n    url: https://github.com/example/models\n    revision: main\npackages:\n  - id: models\n    name: Models for {language}\n    slug: {language}\n    ecosystem: {language}\n    repository: repo\n    path: pkg\n    metadata-path: {manifest}\n    targets:\n      - id: api\n        extractor: {language}\n        path: {target}\n        role: public-api\n"));
    root
}

fn rendered(root: &support::TestWorkspace) -> RenderedSite {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    render_site(&Site::new(&snapshot).unwrap()).unwrap()
}

fn text<'a>(site: &'a RenderedSite, route: &str) -> &'a str {
    std::str::from_utf8(&site.files()[route].bytes).unwrap()
}

fn collection(root: &support::TestWorkspace, mount: &str, format: &str) {
    root.write("diplodocus.yaml", format!("{}content:\n  - id: docs\n    owner: models\n    repository: repo\n    path: pkg/docs\n    mount: '{mount}'\n    format: {format}\n", root.read("diplodocus.yaml")));
}

#[test]
fn readmes_metadata_links_and_assets_survive_without_sources() {
    for language in ["python", "julia"] {
        let root = fixture(language);
        root.write("pkg/README.md", "# Another README title\n\nA useful package introduction.\n\n![Plot](plot.svg)\n\n[Guide](docs/guide.md#details) and [repository note](NOTE.md).\n\n# More examples\n");
        root.write("pkg/plot.svg", "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'><circle cx='6' cy='6' r='4'/></svg>");
        root.write(
            "pkg/docs/guide.md",
            "# Guide\n\n## Details {#details}\n\nA detailed guide.\n",
        );
        root.write("pkg/NOTE.md", "# Repository note\n");
        collection(&root, "guides", "gfm");
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let metadata = snapshot.workspace().packages["models"]
            .metadata
            .as_ref()
            .unwrap();
        assert_eq!(metadata.version(), Some("1.2.0"));
        assert!(matches!(
            metadata,
            PackageMetadata::Python(_) | PackageMetadata::Julia(_)
        ));
        let saved = tempfile::tempdir().unwrap();
        let path = saved.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        let expected = snapshot.canonical_export().unwrap();
        drop((snapshot, sources, root));
        let snapshot = Snapshot::load(&path).unwrap();
        assert_eq!(snapshot.canonical_export().unwrap(), expected);
        let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let route = format!("packages/{language}/index");
        let html = text(&site, &format!("{route}.html"));
        assert_eq!(html.matches("<h1").count(), 1);
        assert!(html.contains(&format!(">Models for {language}</h1>")));
        assert!(html.contains("id=\"another-readme-title\""));
        assert!(html.contains("guides/guide.html#details"));
        assert!(html.contains("https://github.com/example/models/blob/main/pkg/NOTE.md"));
        assert!(html.contains("<img "));
        assert!(
            html.find("class=\"package-metadata\"").unwrap()
                < html.find("class=\"package-overview-body\"").unwrap()
        );
        let markdown = text(&site, &format!("{route}.md"));
        assert_eq!(
            markdown
                .lines()
                .filter(|line| line.starts_with("# "))
                .count(),
            1
        );
        assert!(markdown.contains("**Version:** 1.2.0"));
        assert!(markdown.contains("A useful package introduction."));
        if language == "python" {
            assert!(text(&site, "assets/search.json").contains("Model Authors"));
            for value in ["MIT", "Model Maintainer", "https://example.com/issues"] {
                assert!(html.contains(value), "{value}");
                assert!(markdown.contains(value), "{value}");
            }
            assert!(!html.contains("javascript:alert"));
        } else {
            assert!(html.contains("Model Authors"));
            assert!(!html.contains("<dt>License</dt>"));
        }
        assert!(
            !site
                .files()
                .contains_key(&format!("packages/{language}/news.html"))
        );
    }
}

#[test]
fn authored_index_overrides_configured_file_and_readme() {
    let root = fixture("python");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "    metadata-path:",
            "    overview: introduction.md\n    metadata-path:",
        ),
    );
    root.write(
        "pkg/introduction.md",
        "[An ignored broken link](missing.md)\n",
    );
    root.write("pkg/README.md", "![Ignored missing image](missing.png)\n");
    root.write(
        "pkg/docs/index.qmd",
        "# An authored homepage\n\nAn authored introduction.\n",
    );
    collection(&root, "", "qmd");
    let site = rendered(&root);
    let html = text(&site, "packages/python/index.html");
    assert!(html.contains(">An authored homepage</h1>"));
    assert_eq!(html.matches("<h1").count(), 1);
    assert!(html.contains("An authored introduction."));
    assert!(html.contains("Package information"));
    assert!(!html.contains("ignored"));
}

#[cfg(unix)]
#[test]
fn authored_index_alias_overrides_readme() {
    for language in ["python", "julia"] {
        let root = fixture(language);
        root.write("pkg/README.md", "A README introduction.\n");
        root.write(
            "pkg/docs/intro.md",
            "# An authored homepage\n\nAn authored introduction.\n",
        );
        std::os::unix::fs::symlink("intro.md", root.path().join("pkg/docs/index.md")).unwrap();
        collection(&root, "", "gfm");
        let site = rendered(&root);
        let html = text(&site, &format!("packages/{language}/index.html"));
        assert!(html.contains(">An authored homepage</h1>"));
        assert_eq!(html.matches("<h1").count(), 1);
        assert!(html.contains("An authored introduction."));
        assert!(html.contains("Package information"));
        assert!(!html.contains("A README introduction."));
    }
}

#[test]
fn configured_file_overrides_readme_and_tracks_its_own_asset_base() {
    let root = fixture("python");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "    metadata-path:",
            "    overview: introduction/index.md\n    metadata-path:",
        ),
    );
    root.write(
        "pkg/introduction/index.md",
        "A selected introduction.\n\n![Diagram](figure.svg)\n",
    );
    root.write(
        "pkg/introduction/figure.svg",
        "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'/>",
    );
    root.write("pkg/README.md", "![Ignored image](missing.png)\n");
    let site = rendered(&root);
    let html = text(&site, "packages/python/index.html");
    assert!(html.contains("A selected introduction."));
    assert!(html.contains("<img "));
    assert!(!html.contains("Ignored image"));
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.write("pkg/introduction/index.md", "An updated introduction.\n");
    assert!(sources.revalidate().is_err());
}

#[test]
fn configured_file_inside_a_collection_is_mounted_only_at_the_package_root() {
    let root = fixture("python");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "    metadata-path:",
            "    overview: docs/introduction.md\n    metadata-path:",
        ),
    );
    root.write(
        "pkg/docs/introduction.md",
        "# Selected overview\n\nA selected introduction.\n",
    );
    root.write(
        "pkg/docs/guide.md",
        "# Guide\n\n[Overview](introduction.md)\n",
    );
    collection(&root, "guides", "gfm");
    let site = rendered(&root);
    assert!(text(&site, "packages/python/index.html").contains("A selected introduction."));
    assert!(
        !site
            .files()
            .contains_key("packages/python/guides/introduction.html")
    );
    assert!(text(&site, "packages/python/guides/guide.html").contains("href=\"../index.html\""));
}

#[test]
fn license_values_link_to_packaged_license_files_without_sources() {
    for language in ["python", "julia"] {
        let root = fixture(language);
        root.write(
            "pkg/LICENSE.md",
            "# Package license\n\nActual package license terms.\n",
        );
        root.write(
            "pkg/README.md",
            "# Models\n\nRead the [license](LICENSE.md).\n",
        );
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let saved = tempfile::tempdir().unwrap();
        let path = saved.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        drop((snapshot, sources, root));
        let snapshot = Snapshot::load(&path).unwrap();
        let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let value = if language == "python" {
            "MIT"
        } else {
            "View license"
        };
        let html = text(&site, &format!("packages/{language}/index.html"));
        assert!(html.contains(&format!(
            "<dt>License</dt><dd><a href=\"license.html\">{value}</a></dd>"
        )));
        assert!(
            text(&site, &format!("packages/{language}/index.md"))
                .contains(&format!("**License:** [{value}](<license.html>)"))
        );
        assert!(
            text(&site, &format!("packages/{language}/license.html"))
                .contains("Actual package license terms.")
        );
    }
}

#[cfg(unix)]
#[test]
fn conventional_license_aliases_are_merged_once() {
    for language in ["python", "julia"] {
        let root = fixture(language);
        root.write(
            "pkg/LICENSE.md",
            "# Package license\n\nActual package license terms.\n",
        );
        std::os::unix::fs::symlink("LICENSE.md", root.path().join("pkg/LICENSE")).unwrap();
        root.write("pkg/COPYING.md", "Additional license terms.\n");
        let site = rendered(&root);
        let html = text(&site, &format!("packages/{language}/license.html"));
        assert_eq!(html.matches("Actual package license terms.").count(), 1);
        assert_eq!(html.matches("id=\"package-license\"").count(), 1);
        assert_eq!(html.matches("Additional license terms.").count(), 1);
        let markdown = text(&site, &format!("packages/{language}/license.md"));
        assert_eq!(markdown.matches("Actual package license terms.").count(), 1);
    }
}

#[test]
fn authored_license_pages_override_discovered_license_files() {
    let root = fixture("python");
    root.write("pkg/LICENSE.md", "![Ignored image](missing.png)\n");
    root.write("pkg/README.md", "Read the [license](LICENSE.md).\n");
    root.write(
        "pkg/docs/license.md",
        "# License\n\nThe authored license terms.\n",
    );
    collection(&root, "", "gfm");
    let site = rendered(&root);
    assert!(text(&site, "packages/python/license.html").contains("The authored license terms."));
    assert!(
        text(&site, "packages/python/index.html")
            .contains("<dd><a href=\"license.html\">MIT</a></dd>")
    );
}

#[test]
fn description_fallback_and_readme_discovery_are_tracked() {
    let root = fixture("python");
    let site = rendered(&root);
    assert!(text(&site, "packages/python/index.html").contains("Compare predictions reliably."));
    assert!(text(&site, "assets/search.json").contains("Compare predictions reliably."));
    assert!(text(&site, "packages/python/index.html").contains("<dt>License</dt><dd>MIT</dd>"));
    assert!(!site.files().contains_key("packages/python/license.html"));
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.write(
        "pkg/README.md",
        "# Newly added README\n\nA new introduction.\n",
    );
    assert!(sources.revalidate().is_err());
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.remove("pkg/README.md");
    assert!(sources.revalidate().is_err());
}

#[test]
fn overview_preserves_an_initial_section_heading_below_the_package_title() {
    let root = fixture("python");
    root.write(
        "pkg/README.md",
        "## Installation\n\nInstall the package.\n\n# More examples\n",
    );
    let site = rendered(&root);
    let html = text(&site, "packages/python/index.html");
    assert_eq!(html.matches("<h1").count(), 1);
    assert!(html.contains(">Models for python</h1>"));
    assert!(html.contains(">Installation</h2>"));
    assert!(html.contains(">More examples</h2>"));
}

#[test]
fn every_package_has_an_overview_without_extraction_targets_or_metadata() {
    for language in ["r", "python", "julia", "rust", "component"] {
        let root = support::TestWorkspace::new();
        root.write("pkg/metadata", "An opaque component manifest.\n");
        root.write(
            "pkg/README.md",
            "# Component\n\nA component introduction.\n",
        );
        root.write("diplodocus.yaml", format!("project:\n  name: Components\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: component\n    name: Component documentation\n    slug: {language}\n    ecosystem: {language}\n    repository: repo\n    path: pkg\n    metadata-path: metadata\n    targets: []\n"));
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        assert!(sources.workspace().pages.values().any(|page| page.kind
            == PageKind::PackageDocumentation {
                role: PackagePageRole::Overview
            }));
        let site = rendered(&root);
        let html = text(&site, &format!("packages/{language}/index.html"));
        assert!(html.contains("Package information"));
        assert!(html.contains("A component introduction."));
        assert!(!html.contains("<dt>Version</dt>"));
    }
}
