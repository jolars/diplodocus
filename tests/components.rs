mod support;

use diplodocus::{
    assembly::assemble_workspace,
    rendering::{render_fragments, render_site},
    site::Site,
    snapshots::Snapshot,
    validation::resolve_workspace,
};

fn workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        r#"project:
  name: Components
repositories:
  - id: docs
    path: .
components:
  - id: counter
    repository: docs
    path: widget
    module: index.js
    stylesheets: [style.css]
content:
  - id: guide
    owner: project
    repository: docs
    path: pages
    mount: ""
    format: gfm
"#,
    );
    root.write(
        "pages/index.md",
        r#"# Components

::: {.component name="counter" #example loading="Loading <counter>" min-height="24.5rem" props='{"counts":[1,2],"label":"<example>"}'}

Fallback **content**.

:::
"#,
    );
    root.write(
        "widget/index.js",
        "export function mount(root) { root.textContent = 'Ready'; }\n",
    );
    root.write("widget/style.css", ".counter { color: blue; }\n");
    root.write("widget/nested/worker.js", "postMessage('ready');\n");
    root
}

#[test]
fn component_bundles_survive_snapshot_round_trip_without_a_checkout() {
    let root = workspace();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let path = root.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    std::fs::remove_dir_all(root.path().join("widget")).unwrap();
    root.remove("pages/index.md");
    root.remove("diplodocus.yaml");
    let snapshot = Snapshot::load(path).unwrap();
    let passive = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&passive.files()["index.html"].bytes).unwrap();
    assert!(html.contains("Fallback <strong>content</strong>"));
    assert!(!html.contains("data-component-module"));
    assert!(!html.contains("data-component-props"));
    assert!(!html.contains("data-component-loading"));
    assert!(!html.contains("24.5rem"));
    let fragments = render_fragments(&Site::new(&snapshot).unwrap().with_components(true)).unwrap();
    let html = std::str::from_utf8(&fragments.files()["index.html"].bytes).unwrap();
    assert!(html.contains("Fallback <strong>content</strong>"));
    assert!(!html.contains("data-component-props"));
    assert!(!html.contains("data-component-loading"));
    assert!(!html.contains("24.5rem"));
    assert!(
        !passive
            .files()
            .keys()
            .any(|p| p.starts_with("assets/components/"))
    );
    let active = render_site(&Site::new(&snapshot).unwrap().with_components(true)).unwrap();
    let html = std::str::from_utf8(&active.files()["index.html"].bytes).unwrap();
    assert!(html.contains("data-component-module"));
    assert!(html.contains("data-component-props=\"{&quot;counts&quot;:[1,2],&quot;label&quot;:&quot;&lt;example&gt;&quot;}\""));
    assert!(html.contains("assets/components.js"));
    assert!(html.contains("data-component-loading role=\"status\" hidden>Loading &lt;counter&gt;"));
    assert!(html.contains("--diplodocus-component-default-min-height:24.5rem"));
    let markdown = std::str::from_utf8(&active.files()["index.md"].bytes).unwrap();
    assert!(markdown.contains("Fallback **content**"));
    assert!(!markdown.contains("<example>"));
    assert!(
        active
            .files()
            .keys()
            .any(|p| p.ends_with("/nested/worker.js"))
    );
}

#[test]
fn missing_modules_and_unknown_component_names_are_diagnosed() {
    for missing_module in [false, true] {
        let root = workspace();
        if missing_module {
            root.remove("widget/index.js");
        } else {
            root.write(
                "pages/index.md",
                "# Missing\n\n::: {.component name=\"unknown\"}\n\nFallback.\n\n:::\n",
            );
        }
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        assert!(resolve_workspace(&sources).is_err());
    }
}

#[test]
fn bundle_changes_invalidate_resolution_and_escaping_symlinks_are_rejected() {
    let root = workspace();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    root.write("widget/index.js", "changed");
    assert!(resolved.revalidate().is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", root.path().join("widget/escape.js")).unwrap();
        assert!(resolve_workspace(&sources).is_err());
    }
}

#[test]
fn validates_qmd_component_props_and_reports_included_gfm_sources() {
    let root = workspace();
    let config = root.path().join("diplodocus.yaml");
    let qmd = std::fs::read_to_string(&config)
        .unwrap()
        .replace("format: gfm", "format: qmd");
    root.write("diplodocus.yaml", &qmd);
    root.remove("pages/index.md");
    root.write(
        "pages/index.qmd",
        r#"# QMD

::: {.component name="counter" props='{"visible":true}' loading="Loading" min-height="0"}

Fallback.

:::
"#,
    );
    let sources = assemble_workspace(&config).unwrap();
    assert!(resolve_workspace(&sources).is_ok());

    root.write("diplodocus.yaml", qmd.replace("format: qmd", "format: gfm"));
    root.remove("pages/index.qmd");
    root.write(
        "pages/index.md",
        "# GFM\n\n<!--@include: ../partial.md-->\n",
    );
    for props in [
        "props='[]'",
        "props='{bad}'",
        "props='{}' props='{}'",
        "loading=' '",
        "loading='one' loading='two'",
        "min-height='-2rem'",
        "min-height='1px;color:red'",
        "min-height='1px' min-height='2px'",
    ] {
        root.write(
            "partial.md",
            format!("::: {{.component name=\"counter\" {props}}}\n\nFallback.\n\n:::\n"),
        );
        let sources = assemble_workspace(&config).unwrap();
        let err = resolve_workspace(&sources).unwrap_err();
        let diplodocus::validation::ResolutionError::Diagnostics(diagnostics) = err else {
            panic!("{err:?}");
        };
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == diplodocus::diagnostics::DiagnosticCode::InvalidComponent
                    && diagnostic.span.is_some()
                    && format!("{:?}", diagnostic.source).contains("partial.md")
                    && diagnostic.message.contains("Included from")
            }),
            "{diagnostics:?}"
        );
    }
}
