mod support;

use diplodocus::{
    assembly::assemble_workspace, rendering::render_site, site::Site, snapshots::Snapshot,
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
        "# Components\n\n::: {.component name=\"counter\"}\n\nFallback **content**.\n\n:::\n",
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
    let snapshot = Snapshot::load(path).unwrap();
    let passive = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&passive.files()["index.html"].bytes).unwrap();
    assert!(html.contains("Fallback <strong>content</strong>"));
    assert!(!html.contains("data-component-module"));
    assert!(
        !passive
            .files()
            .keys()
            .any(|p| p.starts_with("assets/components/"))
    );
    let active = render_site(&Site::new(&snapshot).unwrap().with_components(true)).unwrap();
    let html = std::str::from_utf8(&active.files()["index.html"].bytes).unwrap();
    assert!(html.contains("data-component-module"));
    assert!(html.contains("assets/components.js"));
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
