mod support;

use std::collections::BTreeMap;
use std::io::Cursor;

use diplodocus::assembly::assemble_workspace;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::rendering::{RenderedSite, render_fragments, render_site};
use diplodocus::site::{Site, UrlMap};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn snapshot(format: image::ImageFormat) -> (Snapshot, Vec<u8>, String) {
    let root = support::TestWorkspace::new();
    let extension = if format == image::ImageFormat::Png {
        "png"
    } else {
        "jpg"
    };
    root.write("diplodocus.yaml", format!(
        "project:\n  name: Images\npresentation:\n  logo:\n    repository: docs\n    path: docs/image.{extension}\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: ''\n    format: gfm\n"
    ));
    let mut state = 42_u32;
    let image = image::RgbaImage::from_fn(1200, 600, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        image::Rgba([state as u8, (state >> 8) as u8, (state >> 16) as u8, 128])
    });
    let mut bytes = Cursor::new(Vec::new());
    let image = image::DynamicImage::ImageRgba8(image);
    if format == image::ImageFormat::Jpeg {
        image.to_rgb8().write_to(&mut bytes, format).unwrap();
    } else {
        image.write_to(&mut bytes, format).unwrap();
    }
    root.write(format!("docs/image.{extension}"), bytes.get_ref());
    root.write(
        "docs/index.md",
        format!("# Images\n\n![Plot](image.{extension})\n\n[Original](image.{extension})\n"),
    );
    root.write(
        "docs/nested/page.md",
        format!("# Nested\n\n![Plot](../image.{extension})\n"),
    );
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(root);
    let bytes = bytes.into_inner();
    let original = format!("assets/{}.{extension}", fingerprint_bytes(&bytes).value);
    (Snapshot::load(path).unwrap(), bytes, original)
}

fn image_tag(rendered: &RenderedSite, route: &str, alt: &str) -> String {
    let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
    html.split("<img ")
        .find(|tag| tag.contains(&format!("alt=\"{alt}\"")))
        .unwrap()
        .split('>')
        .next()
        .unwrap()
        .to_owned()
}

fn candidates(tag: &str) -> Vec<(String, u32)> {
    tag.split("srcset=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .split(", ")
        .map(|candidate| {
            let (url, width) = candidate.rsplit_once(' ').unwrap();
            (url.to_owned(), width.trim_end_matches('w').parse().unwrap())
        })
        .collect()
}

#[test]
fn responsive_images_preserve_originals_and_export_smaller_content_addressed_variants() {
    for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
        let (snapshot, bytes, original) = snapshot(format);
        let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        assert_eq!(rendered.files()[&original].bytes, bytes);
        let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        assert!(html.contains(&format!("<a href=\"{original}\">Original</a>")));
        let tag = image_tag(&rendered, "index.html", "Plot");
        assert!(tag.contains("width=\"1200\" height=\"600\""));
        assert!(tag.contains("sizes=\"(max-width: 760px)"));
        let sources = candidates(&tag);
        assert_eq!(
            sources.iter().map(|(_, width)| *width).collect::<Vec<_>>(),
            [480, 960, 1200]
        );
        for (path, width) in &sources[..sources.len() - 1] {
            let file = &rendered.files()[path];
            assert!(file.bytes.len() < bytes.len());
            assert!(path.contains(&fingerprint_bytes(&file.bytes).value));
            let image = image::load_from_memory(&file.bytes).unwrap();
            assert_eq!((image.width(), image.height()), (*width, width / 2));
            if format == image::ImageFormat::Png {
                assert_eq!(image.to_rgba8().get_pixel(0, 0).0[3], 128);
            }
        }
        assert_eq!(sources.last().unwrap().0, original);
        let nested = candidates(&image_tag(&rendered, "nested/page.html", "Plot"));
        assert_eq!(
            nested,
            sources
                .iter()
                .map(|(path, width)| (format!("../{path}"), *width))
                .collect::<Vec<_>>()
        );
        let logo = image_tag(&rendered, "index.html", "");
        assert!(logo.contains("sizes=\"1.75rem\""));
        assert_eq!(
            candidates(&logo)
                .iter()
                .map(|(_, width)| *width)
                .collect::<Vec<_>>(),
            [32, 64, 96, 1200]
        );
    }
}

#[test]
fn responsive_fragments_use_mapped_urls_and_conservative_sizes() {
    let (snapshot, _, _) = snapshot(image::ImageFormat::Png);
    let rendered = render_fragments(&Site::new(&snapshot).unwrap()).unwrap();
    let assets = rendered
        .files()
        .keys()
        .filter(|path| path.starts_with("assets/"))
        .map(|path| (path.clone(), format!("https://cdn.example.test/{path}")))
        .collect();
    let site = Site::new(&snapshot)
        .unwrap()
        .with_url_map(UrlMap {
            format_version: 1,
            documents: BTreeMap::new(),
            assets,
        })
        .unwrap();
    let mapped = render_fragments(&site).unwrap();
    let tag = image_tag(&mapped, "nested/page.html", "Plot");
    assert!(tag.contains("sizes=\"100vw\""));
    for (url, _) in candidates(&tag) {
        assert!(url.starts_with("https://cdn.example.test/assets/"));
    }
    assert_eq!(
        rendered.files().keys().collect::<Vec<_>>(),
        mapped.files().keys().collect::<Vec<_>>()
    );
    for (path, file) in rendered
        .files()
        .iter()
        .filter(|(path, _)| path.starts_with("assets/"))
    {
        assert_eq!(file.bytes, mapped.files()[path].bytes);
    }
}
