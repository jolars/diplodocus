mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::configuration::parse_configuration;
use diplodocus::configuration_validation::validate_configuration;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::rendering::{render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(base: &str, style: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!(
        "project:\n  name: \"Preview & docs\"\npresentation:\n  site-url: \"{base}\"\n  canonical-url-style: \"{style}\"\n  description: \"Guides & examples\"\n  open-graph:\n    repository: \"docs\"\n    path: \"branding/og.png\"\n    alt: \"A dinosaur & an open book\"\nrepositories:\n  - id: \"docs\"\n    path: \".\"\ncontent:\n  - id: \"guide\"\n    owner: \"project\"\n    repository: \"docs\"\n    path: \"docs\"\n    mount: \"\"\n    format: \"gfm\"\n"
    ));
    root.write("docs/index.md", "# Preview & docs\n");
    root.write("docs/guide/nested.md", "# Nested & guide\n");
    root.write("branding/og.png", image(image::ImageFormat::Png));
    root
}

fn image(format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(120, 63, image::Rgb([10, 20, 40]))
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

#[test]
fn preview_images_and_absolute_metadata_survive_without_source_files() {
    for (base, style, home, nested) in [
        ("https://example.test/manual", "clean", "", "guide/nested"),
        (
            "https://example.test/manual/",
            "file",
            "index.html",
            "guide/nested.html",
        ),
    ] {
        let root = workspace(base, style);
        let original = snapshot(&root);
        let artifact = tempfile::tempdir().unwrap();
        let path = artifact.path().join("snapshot.sqlite");
        original.publish(&path).unwrap();
        drop(root);
        let loaded = Snapshot::load(path).unwrap();
        assert_eq!(
            loaded.canonical_export().unwrap(),
            original.canonical_export().unwrap()
        );
        let bytes = image(image::ImageFormat::Png);
        let digest = fingerprint_bytes(&bytes).value;
        assert_eq!(loaded.open_graph().unwrap().fingerprint.value, digest);
        let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
        assert_eq!(
            rendered.files()[&format!("assets/{digest}.png")].bytes,
            bytes
        );
        for (route, public, title) in [
            ("index.html", home, "Preview &amp; docs"),
            (
                "guide/nested.html",
                nested,
                "Nested &amp; guide · Preview &amp; docs",
            ),
        ] {
            let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
            for (property, content) in [
                ("og:type", "website".into()),
                ("og:title", title.into()),
                ("og:site_name", "Preview &amp; docs".into()),
                ("og:description", "Guides &amp; examples".into()),
                ("og:url", format!("https://example.test/manual/{public}")),
                (
                    "og:image",
                    format!("https://example.test/manual/assets/{digest}.png"),
                ),
                ("og:image:type", "image/png".into()),
                ("og:image:width", "120".into()),
                ("og:image:height", "63".into()),
                ("og:image:alt", "A dinosaur &amp; an open book".into()),
            ] {
                assert!(
                    html.contains(&format!(
                        "<meta property=\"{property}\" content=\"{content}\">"
                    )),
                    "missing {property} on {route}"
                );
            }
            assert!(html.contains("<meta name=\"twitter:card\" content=\"summary_large_image\">"));
            assert!(html.contains(&format!("<meta name=\"twitter:image\" content=\"https://example.test/manual/assets/{digest}.png\">")));
            assert!(html.contains(
                "<meta name=\"twitter:image:alt\" content=\"A dinosaur &amp; an open book\">"
            ));
        }
        let fragments = render_fragments(&Site::new(&loaded).unwrap()).unwrap();
        for (path, file) in fragments.files() {
            if path.ends_with(".html") {
                assert!(!std::str::from_utf8(&file.bytes).unwrap().contains("<meta "));
            }
        }
    }
}

#[test]
fn jpeg_previews_use_their_real_media_type_and_dimensions() {
    let root = workspace("https://example.test/", "clean");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace("og.png", "og.jpg"),
    );
    root.write("branding/og.jpg", image(image::ImageFormat::Jpeg));
    let rendered = render_site(&Site::new(&snapshot(&root)).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(html.contains("<meta property=\"og:image:type\" content=\"image/jpeg\">"));
    assert!(html.contains("<meta property=\"og:image:width\" content=\"120\">"));
    assert!(rendered.files().keys().any(|path| path.ends_with(".jpg")));
}

#[test]
fn preview_logo_and_document_can_share_one_asset() {
    let root = workspace("https://example.test/", "clean");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml")
            .replace("branding/og.png", "docs/og.png"),
    );
    root.write("docs/og.png", image(image::ImageFormat::Png));
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "presentation:\n",
            "presentation:\n  logo:\n    repository: docs\n    path: branding/og.png\n",
        ),
    );
    root.write("docs/index.md", "# Preview & docs\n\n![Preview](og.png)\n");
    let snapshot = snapshot(&root);
    assert_eq!(snapshot.assets().len(), 1);
    assert_eq!(snapshot.open_graph(), snapshot.logo());
}

#[test]
fn unconfigured_sites_keep_their_original_metadata() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        "project:\n  name: \"Docs\"\npresentation:\n  site-url: \"https://example.test/\"\n",
    );
    let snapshot = snapshot(&root);
    assert!(snapshot.open_graph().is_none());
    assert!(!snapshot.canonical_export().unwrap().contains("open-graph"));
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(!html.contains("og:"));
    assert!(!html.contains("twitter:"));
}

#[test]
fn preview_configuration_requires_a_site_url_repository_and_description() {
    let root = workspace("https://example.test/", "clean");
    let source = root.read("diplodocus.yaml");
    for source in [
        source.replace("site-url: \"https://example.test/\"", ""),
        source.replace("repository: \"docs\"", "repository: \"missing\""),
        source.replace("alt: \"A dinosaur & an open book\"", "alt: \"  \""),
    ] {
        let configuration = parse_configuration(&source).unwrap();
        assert!(!validate_configuration(&configuration).is_empty());
    }
    for source in [
        source.replace("alt: \"A dinosaur & an open book\"", ""),
        source.replace("branding/og.png", "../og.png"),
        source.replace("branding/og.png", "/og.png"),
        source.replace("open-graph", "open_graph"),
    ] {
        assert!(parse_configuration(&source).is_err());
    }
    let configuration = parse_configuration(&source).unwrap();
    assert_eq!(
        parse_configuration(&serde_saphyr::to_string(&configuration).unwrap()).unwrap(),
        configuration
    );
}

#[test]
fn previews_reject_missing_invalid_and_vector_images() {
    let root = workspace("https://example.test/", "clean");
    for (name, bytes) in [
        ("invalid.png", Some(b"not an image".as_slice())),
        ("missing.png", None),
        (
            "vector.svg",
            Some(b"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>".as_slice()),
        ),
    ] {
        let source = root
            .read("diplodocus.yaml")
            .replace("branding/og.png", &format!("branding/{name}"));
        root.write("case.yaml", source);
        if let Some(bytes) = bytes {
            root.write(format!("branding/{name}"), bytes);
        }
        let sources = assemble_workspace(root.path().join("case.yaml")).unwrap();
        assert!(resolve_workspace(&sources).is_err(), "{name}");
    }
}

#[test]
fn changed_preview_bytes_are_rechecked_before_publication() {
    let root = workspace("https://example.test/", "clean");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(
        resolved
            .input_paths()
            .any(|path| path.ends_with("branding/og.png"))
    );
    root.write("branding/og.png", image(image::ImageFormat::Jpeg));
    assert!(Snapshot::from_sources(&sources, &resolved).is_err());
}

#[cfg(unix)]
#[test]
fn previews_cannot_follow_symlinks_outside_the_repository() {
    let root = workspace("https://example.test/", "clean");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        outside.path().join("og.png"),
        image(image::ImageFormat::Png),
    )
    .unwrap();
    root.remove("branding/og.png");
    std::os::unix::fs::symlink(
        outside.path().join("og.png"),
        root.path().join("branding/og.png"),
    )
    .unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}
