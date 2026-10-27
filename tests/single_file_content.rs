mod support;

use diplodocus::assembly::{AssemblyError, assemble_workspace};
use diplodocus::configuration::load_configuration;
use diplodocus::documents::AuthoredFormat;
use diplodocus::paths::{PathResolutionErrorKind, resolve_workspace_paths};
use diplodocus::provenance::{DeclaredSourceInputs, collect_static_provenance};
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(owner: &str, mount: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        format!(
            r#"project:
  name: Files
repositories:
  - id: repo
    path: .
packages:
  - id: pkg
    name: Package
    slug: pkg
    ecosystem: python
    repository: repo
    path: .
    metadata-path: pyproject.toml
    targets: []
content:
  - id: guide
    owner: project
    repository: repo
    path: docs
    mount: guide
    format: gfm
  - id: changelog
    owner: {owner}
    repository: repo
    path: CHANGELOG.md
    mount: '{mount}'
    format: gfm
  - id: contributing
    owner: {owner}
    repository: repo
    path: CONTRIBUTING.md
    mount: '{mount}'
    format: gfm
"#
        ),
    );
    root.write(
        "pyproject.toml",
        "[project]\nname = 'pkg'\nversion = '1.0.0'\n",
    );
    root.write("docs/index.md", "# Guide\n\n[Changelog](../CHANGELOG.md)\n");
    root.write(
        "CHANGELOG.md",
        "# Changelog\n\n[Contributing](CONTRIBUTING.md#checks)\n\n![Diagram](diagram.svg)\n\n[Download](sample.txt)\n\n```python\nraise RuntimeError('display only')\n```\n",
    );
    root.write(
        "CONTRIBUTING.md",
        "# Contributing\n\n## Checks {#checks}\n\n[Guide](docs/index.md) and [changelog](CHANGELOG.md).\n",
    );
    root.write(
        "diagram.svg",
        "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12'><circle r='3'/></svg>",
    );
    root.write("sample.txt", "A downloadable sample.\n");
    root.write("IGNORED.md", "# Unselected\n\n[Broken](missing.md)\n");
    root
}

#[test]
fn single_files_preserve_routes_links_assets_and_navigation_without_sources() {
    for owner in ["project", "pkg"] {
        for mount in ["", "project"] {
            let root = workspace(owner, mount);
            let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
            assert_eq!(sources.prepared_pages().len(), 3);
            assert_eq!(
                sources.workspace().content_collections["changelog"]
                    .path
                    .as_ref()
                    .unwrap()
                    .as_str(),
                "CHANGELOG.md"
            );
            assert!(
                sources
                    .input_paths()
                    .any(|path| path == root.path().join("CHANGELOG.md"))
            );
            assert!(
                !sources
                    .input_paths()
                    .any(|path| path.ends_with("IGNORED.md"))
            );
            sources.revalidate().unwrap();
            let resolved = resolve_workspace(&sources).unwrap();
            assert!(
                resolved.diagnostics().is_empty(),
                "{:?}",
                resolved.diagnostics()
            );
            let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
            let saved = tempfile::tempdir().unwrap();
            let path = saved.path().join("snapshot.sqlite");
            snapshot.publish(&path).unwrap();
            let expected = snapshot.canonical_export().unwrap();
            drop((snapshot, resolved, sources, root));
            let snapshot = Snapshot::load(path).unwrap();
            assert_eq!(snapshot.canonical_export().unwrap(), expected);
            let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
            let base = if owner == "project" {
                ""
            } else {
                "packages/pkg/"
            };
            let directory = if mount.is_empty() {
                base.to_owned()
            } else {
                format!("{base}{mount}/")
            };
            let html =
                std::str::from_utf8(&site.files()[&format!("{directory}CHANGELOG.html")].bytes)
                    .unwrap();
            assert!(html.contains("CONTRIBUTING.html#checks"));
            assert!(html.contains("<img "));
            assert!(
                site.files()
                    .keys()
                    .any(|path| path.starts_with("assets/") && path.ends_with(".svg"))
            );
            assert!(html.contains("display only"));
            assert!(html.contains(">Changelog</a>"));
            assert!(html.contains(">Contributing</a>"));
            assert!(
                site.files()
                    .contains_key(&format!("{directory}CHANGELOG.md"))
            );
            let search = std::str::from_utf8(&site.files()["assets/search.json"].bytes).unwrap();
            assert!(search.contains("Changelog"));
            assert!(search.contains("Contributing"));
            assert!(!search.contains("Unselected"));
            assert!(
                site.files()
                    .values()
                    .any(|file| file.bytes == b"A downloadable sample.\n")
            );
        }
    }
}

#[test]
fn single_file_inputs_detect_edits_without_selecting_siblings() {
    let root = workspace("project", "project");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.write("IGNORED.md", "# Changed unselected page\n");
    sources.revalidate().unwrap();
    root.write("CHANGELOG.md", "# Changed changelog\n");
    assert!(matches!(
        sources.revalidate(),
        Err(AssemblyError::InputsChanged)
    ));
}

#[test]
fn single_qmd_index_preserves_index_navigation_and_disabled_execution() {
    let root = workspace("project", "project");
    root.write(
        "landing/index.qmd",
        "# A landing page\n\n```{python}\nraise RuntimeError('disabled')\n```\n",
    );
    root.write("landing/ignored.qmd", "[Broken](missing.qmd)\n");
    root.write("diplodocus.yaml", format!("{}  - id: landing\n    owner: project\n    repository: repo\n    path: landing/index.qmd\n    mount: landing\n    format: qmd\n    navigation: index\n", root.read("diplodocus.yaml")));
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert_eq!(sources.prepared_pages().len(), 4);
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let site = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&site.files()["landing/index.html"].bytes).unwrap();
    assert!(html.contains("aria-label=\"A landing page\""));
    assert!(html.contains("disabled"));
    assert!(!site.files().contains_key("landing/ignored.html"));
}

#[test]
fn single_file_provenance_rejects_selection_of_sibling_files() {
    let root = workspace("project", "project");
    let path = root.path().join("diplodocus.yaml");
    let config = load_configuration(&path).unwrap();
    let mut inputs = DeclaredSourceInputs::default();
    inputs
        .content
        .insert("changelog".into(), vec!["CONTRIBUTING.md".into()]);
    let error = collect_static_provenance(path, &config, &inputs).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("single-file collections select only their declared file")
    );
}

#[test]
fn single_files_require_the_declared_formats_extension() {
    let root = workspace("project", "project");
    let path = root.path().join("diplodocus.yaml");
    let mut config = load_configuration(&path).unwrap();
    for (format, extensions) in [
        (AuthoredFormat::Gfm, vec!["md"]),
        (AuthoredFormat::Qmd, vec!["qmd"]),
        (AuthoredFormat::Rmd, vec!["rmd", "Rmd"]),
    ] {
        config.content[1].format = format;
        for extension in extensions {
            let source = format!("articles/page.{extension}");
            root.write(&source, "# Page\n");
            config.content[1].path = source.clone().into();
            let resolved = resolve_workspace_paths(&path, &config).unwrap();
            assert_eq!(resolved.content[1].path, root.path().join(source));
            assert_eq!(
                resolved.content[1].source_root,
                root.path().join("articles")
            );
        }
        config.content[1].path = "sample.txt".into();
        let error = resolve_workspace_paths(&path, &config).unwrap_err();
        assert_eq!(error.field, "content[1] (`changelog`).path");
        assert!(matches!(
            error.kind,
            PathResolutionErrorKind::InvalidPath { .. }
        ));
    }
}
