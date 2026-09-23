mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::diagnostics::{DiagnosticCode, Severity};
use diplodocus::validation::{ReferenceTarget, resolve_item, resolve_workspace};

#[test]
fn identical_image_and_download_bytes_share_one_asset() {
    let root = support::acceptance_workspace();
    let image = root.read("core/docs/assets/workspace.svg");
    root.write("core/docs/assets/download.bin", image);
    root.write(
        "core/docs/index.md",
        "[download](assets/download.bin) ![image](assets/workspace.svg)\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert_eq!(resolved.assets().len(), 1);
    assert_eq!(
        resolved.assets().values().next().unwrap().media_type,
        "image/svg+xml"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn generated_reference_errors_point_to_the_authored_cell() {
    let root = support::acceptance_workspace();
    root.write("python/execution/generated-markdown.qmd", "# Generated reference\n\nA paragraph before the responsible cell.\n\n```{python}\nfrom IPython.display import Markdown, display\nhandle = display(Markdown('Initial display'), display_id=True)\n```\n\n```{python}\nhandle.update(Markdown('[`missing-generated-reference`]'))\n```\n");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let cell_span = sources
        .prepared_pages()
        .values()
        .find(|p| p.source().contains("missing-generated-reference"))
        .unwrap()
        .preparation()
        .unwrap()
        .cells[1]
        .cell
        .span;
    let stage = tempfile::tempdir().unwrap();
    let executed = sources
        .execute(diplodocus::assembly::WorkspaceExecution {
            staging_parent: stage.path(),
            deadlines: diplodocus::execution::ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        })
        .await
        .unwrap();
    let error = diplodocus::validation::resolve_executed_workspace(&executed).unwrap_err();
    let diagnostic = error
        .diagnostics()
        .iter()
        .find(|d| d.code == DiagnosticCode::UnresolvedItemReference)
        .unwrap();
    assert_eq!(diagnostic.span, Some(cell_span));
    executed.discard().unwrap();
}

#[test]
fn encoded_local_names_and_external_urls_preserve_their_meaning() {
    let root = support::acceptance_workspace();
    root.write("core/docs/assets/a#b.txt", "download");
    root.write(
        "core/docs/index.md",
        "[download](assets/a%23b.txt) [external](https://example.com/a%2Fb)\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(resolved.records().iter().flat_map(|d| &d.references).any(|r| matches!(&r.target, ReferenceTarget::External { url } if url == "https://example.com/a%2Fb")));
}

#[test]
fn authored_svg_accessibility_does_not_expand_the_generated_image_policy() {
    use diplodocus::execution::assets::{validate_authored_image_bytes, validate_image_bytes};
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' aria-labelledby='label'><title id='label'>Example</title></svg>";
    assert!(validate_authored_image_bytes("image/svg+xml", svg).is_ok());
    assert!(validate_image_bytes("image/svg+xml", svg).is_err());
    for markup in [
        "<svg onload='alert(1)'/>",
        "<svg><script>alert(1)</script></svg>",
        "<svg><image href='https://example.com'/></svg>",
    ] {
        assert!(validate_authored_image_bytes("image/svg+xml", markup.as_bytes()).is_err());
    }
}

#[test]
fn links_can_target_anchors_in_pages_visited_earlier() {
    let root = support::acceptance_workspace();
    root.write("python/docs/aa.qmd", "# Target {#target}\n");
    root.write("python/docs/zz.qmd", "[target](aa.qmd#target)\n");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_ok());
}

#[cfg(unix)]
#[test]
fn retargeted_asset_aliases_invalidate_resolution() {
    let root = support::acceptance_workspace();
    root.write("core/docs/assets/first.txt", "first");
    root.write("core/docs/assets/second.txt", "second");
    let alias = root.path().join("core/docs/assets/alias.txt");
    std::os::unix::fs::symlink("first.txt", &alias).unwrap();
    root.write("core/docs/index.md", "[download](assets/alias.txt)\n");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink("second.txt", &alias).unwrap();
    assert!(resolved.revalidate().is_err());
}

#[test]
fn acceptance_references_and_assets_resolve_without_execution_or_output() {
    let first = support::acceptance_workspace();
    let second = support::acceptance_workspace();
    let a = assemble_workspace(first.path().join("workspace/diplodocus.toml")).unwrap();
    let b = assemble_workspace(second.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&a).unwrap();
    assert_eq!(resolved.records(), resolve_workspace(&b).unwrap().records());
    assert_eq!(resolved.assets().len(), 1);
    let references: Vec<_> = resolved
        .records()
        .iter()
        .flat_map(|d| &d.references)
        .collect();
    assert!(
        references.iter().any(
            |r| matches!(&r.target, ReferenceTarget::Item { item } if item.package == "pyfoo")
        )
    );
    assert!(references.iter().any(
        |r| matches!(&r.target, ReferenceTarget::Page { page, .. } if page.contains("python-guide"))
    ));
    assert!(
        references
            .iter()
            .any(|r| matches!(&r.target, ReferenceTarget::Asset { .. }))
    );
    assert!(!first.path().join("workspace/.diplodocus").exists());
    assert!(!first.path().join("workspace/site").exists());
    let portable = serde_json::to_string(resolved.records()).unwrap();
    assert!(!portable.contains(first.path().to_str().unwrap()));
}

#[test]
fn item_resolution_prefers_the_owner_then_requires_workspace_uniqueness() {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let mut workspace = sources.workspace().clone();
    let pyfit = resolve_item(&workspace, Some("pyfoo"), "foo.fit").unwrap();
    assert_eq!(
        resolve_item(&workspace, None, "pyfoo::foo.fit").unwrap(),
        pyfit
    );
    assert_eq!(
        resolve_item(&workspace, Some("rfoo"), "foo.fit").unwrap(),
        pyfit
    );
    workspace
        .packages
        .insert("another".into(), workspace.packages["pyfoo"].clone());
    assert_eq!(
        resolve_item(&workspace, Some("pyfoo"), "foo.fit").unwrap(),
        pyfit
    );
    assert_eq!(
        resolve_item(&workspace, None, "foo.fit"),
        Err(DiagnosticCode::AmbiguousItemReference)
    );
    assert_eq!(
        resolve_item(&workspace, None, "no-such-name"),
        Err(DiagnosticCode::UnresolvedItemReference)
    );
}

#[test]
fn missing_semantic_page_and_anchor_targets_are_errors_with_portable_context() {
    let root = support::acceptance_workspace();
    root.write(
        "core/docs/bad.md",
        "# Bad\n\n[`not-found`] [missing](missing.md) [anchor](index.md#absent)\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let error = resolve_workspace(&sources).unwrap_err();
    let diagnostics = error.diagnostics();
    for code in [
        DiagnosticCode::UnresolvedItemReference,
        DiagnosticCode::UnresolvedDocumentReference,
    ] {
        assert!(diagnostics.iter().any(|d| d.code == code));
    }
    assert!(
        diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .all(|d| d.source.is_some() && d.span.is_some())
    );
}

#[test]
fn local_assets_are_rechecked_and_remote_or_active_images_are_rejected() {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    root.write("core/docs/assets/workspace.svg", "changed");
    assert!(resolved.revalidate().is_err());
    root.write(
        "core/docs/assets/workspace.svg",
        "<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
    );
    assert!(resolve_workspace(&sources).is_err());
    root.write(
        "core/docs/getting-started/workspace.md",
        "![remote](https://example.com/image.png)\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}

#[test]
fn relationships_distinguish_known_mismatches_and_unknown_constraints() {
    let root = support::acceptance_workspace();
    let compatible = assemble_workspace(
        root.path()
            .join("workspace/variants/relationship-compatible.toml"),
    )
    .unwrap();
    assert!(resolve_workspace(&compatible).is_ok());
    let incompatible = assemble_workspace(
        root.path()
            .join("workspace/variants/relationship-incompatible.toml"),
    )
    .unwrap();
    assert!(
        resolve_workspace(&incompatible)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.code == DiagnosticCode::IncompatiblePackageRelationship)
    );
    let config = root
        .read("workspace/variants/relationship-compatible.toml")
        .replace("^1.8", "uninterpretable");
    root.write("workspace/variants/relationship-compatible.toml", config);
    let unknown = assemble_workspace(
        root.path()
            .join("workspace/variants/relationship-compatible.toml"),
    )
    .unwrap();
    assert!(
        resolve_workspace(&unknown)
            .unwrap()
            .diagnostics()
            .iter()
            .any(
                |d| d.code == DiagnosticCode::IndeterminatePackageRelationship
                    && d.severity == Severity::Warning
            )
    );
}

#[cfg(unix)]
#[test]
fn local_asset_links_cannot_escape_declared_sources_through_symlinks() {
    let root = support::acceptance_workspace();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("private.txt"), "private").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("core/docs/assets/outside"))
        .unwrap();
    root.write(
        "core/docs/index.md",
        "[download](assets/outside/private.txt)\n",
    );
    // Directory symlinks are already rejected by authored discovery.
    assert!(assemble_workspace(root.path().join("workspace/diplodocus.toml")).is_err());
    std::fs::remove_file(root.path().join("core/docs/assets/outside")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.txt"),
        root.path().join("core/docs/assets/private.txt"),
    )
    .unwrap();
    root.write("core/docs/index.md", "[download](assets/private.txt)\n");
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}
