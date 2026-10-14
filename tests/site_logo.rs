mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

const LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40" fill="#165c7c"/></svg>"##;

fn workspace(logo: bool) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    let settings = if logo {
        "[presentation.logo]\nrepository='docs'\npath='branding/logo.svg'\n"
    } else {
        ""
    };
    root.write(
        "diplodocus.toml",
        format!(
            "[project]\nname='Logo & docs'\n{settings}[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='gfm'\n"
        ),
    );
    root.write("docs/index.md", "# Home\n");
    root.write("docs/guide/nested.md", "# Nested\n");
    root
}

#[test]
fn logo_survives_snapshot_generation_without_sources_on_every_page() {
    let root = workspace(true);
    root.write("branding/logo.svg", LOGO);
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(
        resolved
            .input_paths()
            .any(|p| p.ends_with("branding/logo.svg"))
    );
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let digest = fingerprint_bytes(LOGO.as_bytes()).value;
    assert_eq!(loaded.logo().unwrap().fingerprint.value, digest);
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    assert_eq!(
        rendered.files()[&format!("assets/{digest}.svg")].bytes,
        LOGO.as_bytes()
    );
    for (route, prefix) in [("index.html", ""), ("guide/nested.html", "../")] {
        let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
        assert!(html.contains(&format!(
            "<a class=\"brand\" href=\"{prefix}index.html\"><img class=\"brand-logo\" src=\"{prefix}assets/{digest}.svg\" alt=\"\"><span>Logo &amp; docs</span></a>"
        )));
    }
}

#[test]
fn raster_logos_are_preserved_and_use_their_image_extension() {
    for (extension, media, format) in [
        ("png", "image/png", image::ImageFormat::Png),
        ("jpeg", "image/jpeg", image::ImageFormat::Jpeg),
    ] {
        let root = workspace(true);
        root.write(
            "diplodocus.toml",
            root.read("diplodocus.toml")
                .replace("branding/logo.svg", &format!("branding/logo.{extension}")),
        );
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(4, 2, image::Rgb([10, 20, 40]))
            .write_to(&mut bytes, format)
            .unwrap();
        root.write(format!("branding/logo.{extension}"), bytes.get_ref());
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let rendered =
            diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
                .unwrap();
        let digest = fingerprint_bytes(bytes.get_ref()).value;
        let suffix = if extension == "jpeg" { "jpg" } else { "png" };
        let asset = &rendered.files()[&format!("assets/{digest}.{suffix}")];
        assert_eq!(asset.media_type, media);
        assert_eq!(&asset.bytes, bytes.get_ref());
    }
}

#[test]
fn logo_and_document_images_share_one_asset() {
    let root = workspace(true);
    root.write(
        "diplodocus.toml",
        root.read("diplodocus.toml")
            .replace("branding/logo.svg", "docs/assets/logo.svg"),
    );
    root.write("docs/assets/logo.svg", LOGO);
    root.write("docs/index.md", "# Home\n\n![Logo](assets/logo.svg)\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    assert_eq!(snapshot.assets().len(), 1);
}

#[test]
fn unconfigured_logo_keeps_the_title_only() {
    let root = workspace(false);
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    assert!(snapshot.logo().is_none());
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(
        html.contains("<a class=\"brand\" href=\"index.html\"><span>Logo &amp; docs</span></a>")
    );
    assert!(!html.contains("brand-logo"));
    assert!(!snapshot.canonical_export().unwrap().contains("logo"));
}

#[test]
fn logo_rejects_missing_nonimage_and_unsafe_svg_files() {
    for bytes in [
        None,
        Some("not an image"),
        Some(r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#),
    ] {
        let root = workspace(true);
        if let Some(bytes) = bytes {
            root.write("branding/logo.svg", bytes);
        }
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        assert!(resolve_workspace(&sources).is_err());
    }
}

#[test]
fn logo_changes_are_rechecked_before_snapshot_publication() {
    let root = workspace(true);
    root.write("branding/logo.svg", LOGO);
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    root.write("branding/logo.svg", LOGO.replace("80", "100"));
    assert!(Snapshot::from_sources(&sources, &resolved).is_err());
}

#[cfg(unix)]
#[test]
fn logo_cannot_follow_a_symlink_outside_its_repository() {
    let root = workspace(true);
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("logo.svg");
    std::fs::write(&target, LOGO).unwrap();
    std::fs::create_dir(root.path().join("branding")).unwrap();
    std::os::unix::fs::symlink(target, root.path().join("branding/logo.svg")).unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}
