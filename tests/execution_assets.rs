use std::fs;
use std::io::Cursor;
use std::path::Path;

use diplodocus::configuration::ExecutionMode;
use diplodocus::diagnostics::DiagnosticPath;
use diplodocus::documents::AuthoredFormat;
use diplodocus::execution::assets::{AssetError, PageAssetStore, validate_image_bytes};
use diplodocus::execution::{ExecutionFailureKind, ExecutionPage};
use diplodocus::ir::SourceLocation;
use diplodocus::provenance::fingerprint_bytes;
use image::{ImageFormat, RgbImage};
use tempfile::TempDir;

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 40" role="img"><rect width="80" height="40" fill="#123456"/><text x="2" y="20" font-family="sans-serif" font-size="12">A &amp; B</text></svg>"##;

fn page(repository: &str, collection: &str, path: &str) -> ExecutionPage {
    ExecutionPage {
        source: SourceLocation {
            repository: repository.into(),
            path: DiagnosticPath::try_from(path).unwrap(),
            span: None,
        },
        collection: collection.into(),
        working_directory: path
            .rsplit_once('/')
            .map(|(p, _)| DiagnosticPath::try_from(p).unwrap()),
        source_fingerprint: fingerprint_bytes(b"source"),
        format: AuthoredFormat::Qmd,
        mode: ExecutionMode::Execute,
        page_veto: false,
        parser_version: "0.29.2".into(),
        qmd_policy: "qmd-mvp-v1".into(),
    }
}

fn raster(format: ImageFormat, value: u8) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    RgbImage::from_pixel(2, 2, image::Rgb([value, 20, 40]))
        .write_to(&mut output, format)
        .unwrap();
    output.into_inner()
}

fn workspace() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("guide")).unwrap();
    fs::write(root.path().join("guide/page.qmd"), "source").unwrap();
    root
}

fn store(root: &Path) -> PageAssetStore {
    PageAssetStore::new(
        page("repo", "guide", "guide/page.qmd"),
        root.to_owned(),
        root.join("staging"),
    )
    .unwrap()
}

#[test]
fn valid_figures_preserve_exact_bytes_and_deduplicate() {
    let root = workspace();
    let mut assets = store(root.path());
    assert!(!root.path().join("staging").exists());
    let png = raster(ImageFormat::Png, 10);
    let jpeg = raster(ImageFormat::Jpeg, 80);
    let mut expected = Vec::new();
    for (mime, bytes) in [
        ("image/png", png.as_slice()),
        ("image/jpeg", &jpeg),
        ("image/svg+xml", SVG),
    ] {
        let record = assets.stage_bytes(mime, bytes).unwrap();
        assert_eq!(record, assets.stage_bytes(mime, bytes).unwrap());
        assert_eq!(record.reference.fingerprint, fingerprint_bytes(bytes));
        assert_eq!(record.byte_size, bytes.len() as u64);
        assert_eq!(record.media_type, mime);
        assert!(
            record
                .reference
                .path
                .as_str()
                .ends_with(&format!("/sha256/{}", fingerprint_bytes(bytes).value))
        );
        expected.push((record, bytes.to_vec()));
    }
    let references = expected
        .iter()
        .rev()
        .map(|(a, _)| a.reference.clone())
        .collect::<Vec<_>>();
    let retained = assets.retain(&references).unwrap();
    assert_eq!(retained.assets.len(), 3);
    assert!(
        retained
            .assets
            .windows(2)
            .all(|w| w[0].reference.fingerprint.value < w[1].reference.fingerprint.value)
    );
    for (record, handle) in retained.assets.iter().zip(&retained.staged_assets) {
        assert_eq!(record.reference, handle.reference);
        assert!(handle.path.starts_with(root.path().join("staging")));
        let bytes = &expected.iter().find(|(a, _)| a == record).unwrap().1;
        assert_eq!(fs::read(&handle.path).unwrap(), *bytes);
    }
    assert!(
        !serde_json::to_string(&retained.assets)
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
}

#[test]
fn namespaces_are_portable_and_encode_identity_without_delimiter_collisions() {
    let first = workspace();
    let second = workspace();
    let a = store(first.path())
        .stage_bytes("image/svg+xml", SVG)
        .unwrap();
    let b = store(second.path())
        .stage_bytes("image/svg+xml", SVG)
        .unwrap();
    assert_eq!(a, b);
    // This page digest was computed independently with Python's hashlib.
    assert!(a.reference.path.as_str().starts_with(
        "execution-assets/81bea735951c3f804c22f3c995f7b0cadfff0c2594d8489ffef1d6ed45a21ed6/sha256/"
    ));
    let mut paths = std::collections::BTreeSet::new();
    for (repository, collection, path) in [
        ("a", "bc", "guide/page.qmd"),
        ("ab", "c", "guide/page.qmd"),
        ("a", "bc", "guide/other.qmd"),
        ("a/b", "c", "guide/page.qmd"),
        ("a", "b/c", "guide/page.qmd"),
    ] {
        let mut store = PageAssetStore::new(
            page(repository, collection, path),
            first.path().to_owned(),
            first.path().join("staging"),
        )
        .unwrap();
        paths.insert(
            store
                .stage_bytes("image/svg+xml", SVG)
                .unwrap()
                .reference
                .path,
        );
    }
    assert_eq!(paths.len(), 5);
}

#[test]
fn rejected_media_never_creates_staging() {
    let root = workspace();
    let mut assets = store(root.path());
    for mime in [
        "image/gif",
        "image/webp",
        "application/pdf",
        "text/html",
        "IMAGE/PNG",
    ] {
        assert_eq!(
            assets.stage_bytes(mime, SVG).unwrap_err(),
            AssetError::UnsupportedMedia
        );
    }
    let png = raster(ImageFormat::Png, 10);
    for (mime, bytes) in [
        ("image/jpeg", png.as_slice()),
        ("image/png", &png[..png.len() / 2]),
        ("image/png", SVG),
    ] {
        assert_eq!(
            assets.stage_bytes(mime, bytes).unwrap_err(),
            AssetError::InvalidMedia
        );
    }
    assert!(!root.path().join("staging").exists());
}

#[test]
fn raster_validation_rejects_missing_end_markers_and_corrupt_pixels() {
    for (mime, format) in [
        ("image/png", ImageFormat::Png),
        ("image/jpeg", ImageFormat::Jpeg),
    ] {
        let bytes = raster(format, 10);
        for remove in [1, 2, 12] {
            assert_eq!(
                validate_image_bytes(mime, &bytes[..bytes.len() - remove]),
                Err(AssetError::InvalidMedia),
                "{mime}, removed {remove} trailing bytes"
            );
        }
    }
    let mut png = raster(ImageFormat::Png, 10);
    let position = png.windows(4).position(|window| window == b"IDAT").unwrap();
    png[position + 5] ^= 0xff;
    assert_eq!(
        validate_image_bytes("image/png", &png),
        Err(AssetError::InvalidMedia)
    );
}

#[test]
fn svg_rejects_active_content_namespaces_and_invalid_values() {
    for body in [
        "<script>alert(1)</script>",
        "<foreignObject/>",
        "<image href='x'/>",
        "<g onclick='x'/>",
        "<g style='fill:red'/>",
        "<g id='x'/>",
        "<rect fill='url(https://example.com/a)'/>",
        "<rect fill='currentColor'/>",
        "<rect width='NaN'/>",
        "<rect width='1e999'/>",
        "<rect width='-2'/>",
        "<path d='M0 0 L1e999 2'/>",
        "<path d='junk'/>",
        "<polygon points='0 0 1'/>",
        "<g transform='scale(1e999)'/>",
        "<g opacity='oops'/>",
        "<g stroke-linecap='unknown'/>",
        "<g xmlns='http://www.w3.org/1999/xhtml'><script/></g>",
        "<g xmlns:x='urn:other' x:width='1'/>",
        "<?instruction value?>",
    ] {
        let svg = format!("<svg xmlns='http://www.w3.org/2000/svg'>{body}</svg>");
        assert_eq!(
            validate_image_bytes("image/svg+xml", svg.as_bytes()),
            Err(AssetError::UnsafeSvg),
            "{body}"
        );
    }
    for svg in [
        "<!DOCTYPE svg><svg/>",
        "<!DOCTYPE svg [<!ENTITY x 'hello'>]><svg>&x;</svg>",
        "<?xml-stylesheet href='evil.css'?><svg/>",
        "<svg xmlns='urn:other'/>",
        "<svg xmlns:x='urn:other'/>",
        "<svg><g></svg>",
    ] {
        assert_eq!(
            validate_image_bytes("image/svg+xml", svg.as_bytes()),
            Err(AssetError::UnsafeSvg),
            "{svg}"
        );
    }
}

#[test]
fn svg_accepts_the_static_geometry_and_text_allowlist() {
    let svg = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="100px" height="50pt" viewBox="0 0 100 50" role="img"><title>Plot</title><desc>A plot.</desc><g transform="translate(1,2) rotate(30) scale(2) skewX(2) matrix(1 0 0 1 0 0)" fill="rgb(20, 30, 40)" stroke="#fff" stroke-width="1" fill-opacity="0.5" stroke-opacity="1" opacity="1" fill-rule="evenodd" stroke-linecap="round" stroke-linejoin="bevel" stroke-miterlimit="4"><path d="M0 0 h1 v1 L2 3 C0 1 2 3 4 5 S1 2 3 4 Q0 1 2 3 T4 5 A2 2 0 0 1 4 4 z"/><rect x="0" y="0" width="20" height="10" rx="2" ry="2"/><circle cx="3" cy="4" r="2"/><ellipse cx="1" cy="2" rx="2" ry="1"/><line x1="0" y1="0" x2="20" y2="10"/><polyline points="0,0 10,10 20,0"/><polygon points="0 0 2 0 1 1"/><text x="1 2" y="3" dx="1" dy="2" font-family="'A Font', sans-serif" font-size="12" font-style="italic" font-weight="700" text-anchor="middle">A<tspan fill="none">B</tspan></text></g></svg>"##;
    validate_image_bytes("image/svg+xml", svg).unwrap();
}

#[test]
fn local_files_are_resolved_from_the_page_and_copied_before_they_change() {
    let root = workspace();
    let png = raster(ImageFormat::Png, 30);
    fs::write(root.path().join("guide/a figure.bin"), &png).unwrap();
    fs::write(root.path().join("figure.svg"), SVG).unwrap();
    let mut assets = store(root.path());
    let a = assets.stage_local("a%20figure.bin").unwrap();
    let b = assets.stage_local("../figure.svg").unwrap();
    assert_eq!(a.media_type, "image/png");
    fs::remove_file(root.path().join("guide/a figure.bin")).unwrap();
    fs::write(root.path().join("figure.svg"), "changed").unwrap();
    let result = assets
        .retain(&[a.reference.clone(), b.reference.clone()])
        .unwrap();
    for handle in result.staged_assets {
        assert_eq!(
            fs::read(handle.path).unwrap(),
            if handle.reference == a.reference {
                png.clone()
            } else {
                SVG.to_vec()
            }
        );
    }
}

#[test]
fn traversal_schemes_and_nonregular_files_fail_the_page() {
    let root = workspace();
    fs::create_dir(root.path().join("guide/directory")).unwrap();
    for target in [
        "../../escape.png",
        "../../guide/../escape.png",
        "/absolute.png",
        "%2fetc/passwd",
        "%2e%2e/%2e%2e/escape.png",
        "C:/image.png",
        "..\\escape.png",
        "file:///image.png",
        "https://example.com/a.png",
        "//example.com/a.png",
        "data:image/png;base64,AA==",
        "%00.png",
        "directory",
    ] {
        let mut assets = store(root.path());
        assert_eq!(
            assets.stage_local(target),
            Err(AssetError::OutsideBoundary),
            "{target}"
        );
        assert_eq!(
            assets.retain(&[]).unwrap_err().kind,
            ExecutionFailureKind::AssetOutsideBoundary
        );
    }
    assert_eq!(
        store(root.path()).stage_local("missing.png"),
        Err(AssetError::Missing)
    );
    assert!(!root.path().join("staging").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_input_or_output_boundaries() {
    use std::os::unix::fs::symlink;
    let root = workspace();
    let outside = workspace();
    fs::write(root.path().join("figure.svg"), SVG).unwrap();
    fs::write(outside.path().join("figure.svg"), SVG).unwrap();
    symlink("../figure.svg", root.path().join("guide/inside.svg")).unwrap();
    assert!(store(root.path()).stage_local("inside.svg").is_ok());
    symlink(outside.path(), root.path().join("guide/escape")).unwrap();
    symlink(root.path(), outside.path().join("return")).unwrap();
    for target in [
        "escape/figure.svg",
        "escape/return/figure.svg",
        "escape/../figure.svg",
    ] {
        assert_eq!(
            store(root.path()).stage_local(target),
            Err(AssetError::OutsideBoundary)
        );
    }
    fs::remove_dir_all(root.path().join("staging")).unwrap();
    symlink(outside.path(), root.path().join("staging")).unwrap();
    assert_eq!(
        store(root.path()).stage_bytes("image/svg+xml", SVG),
        Err(AssetError::OutsideBoundary)
    );
    assert_eq!(fs::read(outside.path().join("figure.svg")).unwrap(), SVG);
}

#[test]
fn drop_rollback_and_retention_remove_unreferenced_files() {
    let root = workspace();
    let mut assets = store(root.path());
    let retained = assets.stage_bytes("image/svg+xml", SVG).unwrap();
    assets
        .stage_bytes("image/png", &raster(ImageFormat::Png, 10))
        .unwrap();
    let result = assets
        .retain(&[retained.reference.clone(), retained.reference])
        .unwrap();
    let private = result.staged_assets[0].path.parent().unwrap();
    assert_eq!(fs::read_dir(private).unwrap().count(), 1);
    let mut assets = store(root.path());
    assets.stage_bytes("image/svg+xml", SVG).unwrap();
    assert_eq!(
        fs::read_dir(root.path().join("staging")).unwrap().count(),
        2
    );
    drop(assets);
    assert_eq!(
        fs::read_dir(root.path().join("staging")).unwrap().count(),
        1
    );
    let mut assets = store(root.path());
    assets.stage_bytes("image/svg+xml", SVG).unwrap();
    assets.rollback().unwrap();
    assert_eq!(
        fs::read_dir(root.path().join("staging")).unwrap().count(),
        1
    );
    let mut assets = store(root.path());
    assets.stage_bytes("image/svg+xml", SVG).unwrap();
    assert!(assets.retain(&[]).unwrap().staged_assets.is_empty());
    assert_eq!(
        fs::read_dir(root.path().join("staging")).unwrap().count(),
        1
    );
}

#[test]
fn cache_bytes_must_match_digest_media_size_and_namespace() {
    let root = workspace();
    let record = store(root.path())
        .stage_bytes("image/svg+xml", SVG)
        .unwrap();
    assert_eq!(
        store(root.path()).stage_cached(&record, SVG).unwrap(),
        record
    );
    for field in ["size", "digest", "media", "path"] {
        let mut altered = record.clone();
        match field {
            "size" => altered.byte_size += 1,
            "digest" => altered.reference.fingerprint = fingerprint_bytes(b"different"),
            "media" => altered.media_type = "image/png".into(),
            "path" => {
                altered.reference.path = DiagnosticPath::try_from("other/figure.svg").unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            store(root.path()).stage_cached(&altered, SVG).is_err(),
            "{field}"
        );
    }
    assert!(
        store(root.path())
            .stage_cached(&record, b"changed")
            .is_err()
    );
}
