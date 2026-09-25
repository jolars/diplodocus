mod support;

use std::io::Cursor;

use diplodocus::assembly::assemble_workspace;
use diplodocus::provenance::fingerprint_bytes;
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{ReferenceTarget, resolve_workspace};
use image::{ImageFormat, RgbImage};

const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>";

fn images() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let mut images = vec![("svg", "image/svg+xml", SVG.as_bytes().to_vec())];
    for (extension, media, format) in [
        ("png", "image/png", ImageFormat::Png),
        ("jpg", "image/jpeg", ImageFormat::Jpeg),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        RgbImage::from_pixel(2, 2, image::Rgb([10, 20, 40]))
            .write_to(&mut bytes, format)
            .unwrap();
        images.push((extension, media, bytes.into_inner()));
    }
    images
}

fn workspace(execute: bool) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        format!(
            "[project]\nname='Assets'\n[[repository]]\nid='docs'\npath='.'\n\
             [[content]]\nid='guide'\nowner='project'\nrepository='docs'\n\
             path='guide'\nmount='guide'\nformat='qmd'\n[content.execution]\n{}",
            if execute {
                "mode='execute'\nengine='jupyter'\nkernel='python3'\n"
            } else {
                "mode='never'\n"
            }
        ),
    );
    root
}

fn handoff(snapshot: Snapshot, expected: &[(&str, &str, Vec<u8>)]) -> Snapshot {
    let export = snapshot.canonical_export().unwrap();
    let producer = tempfile::tempdir().unwrap();
    let original = producer.path().join("original.sqlite");
    snapshot.publish(&original).unwrap();
    let consumer = tempfile::tempdir().unwrap();
    let copy = consumer.path().join("copy.sqlite");
    std::fs::copy(original, &copy).unwrap();
    drop(snapshot);
    producer.close().unwrap();

    let database =
        rusqlite::Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let count: i64 = database
        .query_row("SELECT count(*) FROM assets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, expected.len() as i64);
    for (_, media, bytes) in expected {
        let (stored_media, stored_bytes): (String, Vec<u8>) = database
            .query_row(
                "SELECT media_type, bytes FROM assets WHERE digest = ?1",
                [fingerprint_bytes(bytes).value],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored_media, *media);
        assert_eq!(stored_bytes, *bytes);
    }
    drop(database);
    let loaded = Snapshot::load(&copy).unwrap();
    assert_eq!(loaded.canonical_export().unwrap(), export);
    assert_eq!(std::fs::read_dir(consumer.path()).unwrap().count(), 1);
    loaded
}

fn assert_rendered_assets(snapshot: &Snapshot, expected: &[(&str, &str, Vec<u8>)]) {
    let rendered = render_site(&Site::new(snapshot).unwrap()).unwrap();
    assert_eq!(snapshot.assets().len(), expected.len());
    for (extension, media, bytes) in expected {
        let fingerprint = fingerprint_bytes(bytes);
        let asset = &snapshot.assets()[&fingerprint.value];
        assert_eq!(asset.fingerprint, fingerprint);
        assert_eq!(asset.bytes, *bytes);
        assert_eq!(asset.media_type, *media);
        let route = format!("assets/{}.{extension}", fingerprint.value);
        assert_eq!(rendered.files()[&route].bytes, *bytes);
        assert_eq!(rendered.files()[&route].media_type, *media);
    }
}

#[test]
fn checked_in_images_and_downloads_survive_without_a_checkout() {
    let root = workspace(false);
    let mut expected = images();
    expected.push((
        "bin",
        "application/octet-stream",
        b"\0\xffdownload\r\n".to_vec(),
    ));
    expected.push(("bin", "application/octet-stream", Vec::new()));
    for (extension, _, bytes) in &expected[..3] {
        root.write(format!("guide/picture.{extension}"), bytes);
    }
    root.write("guide/alias.bin", &expected[1].2);
    root.write("guide/report.pdf", &expected[3].2);
    root.write("guide/empty.bin", &expected[4].2);
    let source = "# Assets\n\n[PNG download](alias.bin)\n\n\
        ![SVG](picture.svg) ![PNG](picture.png) ![JPEG](picture.jpg)\n\n\
        [Report](report.pdf#page=2) [Empty](empty.bin)\n";
    root.write("guide/index.qmd", source);
    root.write("guide/another.qmd", source);
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let source_path = root.path().to_owned();
    drop((sources, resolved, root));
    assert!(!source_path.exists());

    let loaded = handoff(snapshot, &expected);
    assert_rendered_assets(&loaded, &expected);
    assert_eq!(loaded.documents().len(), 2);
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    for document in loaded.documents() {
        assert_eq!(document.references.len(), 6);
        for reference in &document.references {
            let ReferenceTarget::Asset { asset, fragment } = &reference.target else {
                panic!("expected an asset for {}", reference.spelling);
            };
            let index = match reference.spelling.as_str() {
                "picture.svg" => 0,
                "alias.bin" | "picture.png" => 1,
                "picture.jpg" => 2,
                "report.pdf#page=2" => 3,
                "empty.bin" => 4,
                spelling => panic!("unexpected asset reference: {spelling}"),
            };
            let (extension, _, bytes) = &expected[index];
            assert_eq!(asset.fingerprint, fingerprint_bytes(bytes));
            assert_eq!(loaded.assets()[&asset.fingerprint.value].bytes, *bytes);
            assert_eq!(
                asset.path.as_str(),
                format!("content-assets/sha256/{}", asset.fingerprint.value)
            );
            let mut url = format!("../assets/{}%2E{extension}", asset.fingerprint.value);
            if reference.spelling == "report.pdf#page=2" {
                assert_eq!(fragment.as_deref(), Some("page=2"));
                url.push_str("#page%3D2");
            } else {
                assert!(fragment.is_none());
            }
            assert!(html.contains(&format!("\"{url}\"")), "{url}");
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn generated_figures_survive_without_sources_staging_or_execution_cache() {
    use std::collections::{BTreeMap, BTreeSet};

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use diplodocus::assembly::WorkspaceExecution;
    use diplodocus::execution::{ExecutionDeadlines, ValidatedRepresentationRef};
    use diplodocus::ir::{ExecutionOrigin, ProvenanceActivity};
    use diplodocus::validation::resolve_executed_workspace;

    let root = workspace(true);
    let mut expected = images();
    let visible = "<svg xmlns='http://www.w3.org/2000/svg'><circle r='7'/></svg>";
    expected.push(("svg", "image/svg+xml", visible.as_bytes().to_vec()));
    let bundle = serde_json::json!({
        "image/svg+xml": SVG,
        "image/png": STANDARD.encode(&expected[1].2),
        "image/jpeg": STANDARD.encode(&expected[2].2),
    });
    root.write("guide/download.bin", &expected[1].2);
    let source = format!(
        "# Figures\n\n[PNG download](download.bin)\n\n\
         ```{{python}}\n#| include: false\n\
         from IPython.display import display\ndisplay({bundle}, raw=True)\n```\n\n\
         ```{{python}}\n#| echo: false\n\
         display({{'image/svg+xml': {}}}, raw=True)\n```\n",
        serde_json::to_string(visible).unwrap(),
    );
    root.write("guide/index.qmd", &source);
    root.write("guide/another.qmd", &source);
    let stage = tempfile::tempdir().unwrap();
    let mut snapshot = None;
    for origin in [ExecutionOrigin::Executed, ExecutionOrigin::Cache] {
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let executed = sources
            .execute(WorkspaceExecution {
                staging_parent: stage.path(),
                deadlines: ExecutionDeadlines::default(),
                cancellation: Box::pin(std::future::pending()),
            })
            .await
            .unwrap();
        assert_eq!(executed.executed_pages().len(), 2);
        for page in executed.executed_pages().values() {
            assert!(matches!(
                page.validated().record().provenance.as_ref().unwrap().execution.activity,
                ProvenanceActivity::Execution { origin: actual, .. } if actual == origin
            ));
        }
        let resolved = resolve_executed_workspace(&executed).unwrap();
        snapshot = Some(Snapshot::from_executed(&executed, &resolved).unwrap());
        executed.discard().unwrap();
        assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
    }
    assert!(!support::files_under(&root.path().join(".diplodocus/cache/execution")).is_empty());
    let source_path = root.path().to_owned();
    let stage_path = stage.path().to_owned();
    drop(root);
    stage.close().unwrap();
    assert!(!source_path.exists());
    assert!(!stage_path.exists());

    let loaded = handoff(snapshot.unwrap(), &expected);
    assert_rendered_assets(&loaded, &expected);
    let mut paths_by_digest: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for id in loaded.workspace().pages.keys() {
        let page = loaded.executed_page(id).unwrap();
        assert_eq!(page.record().assets.len(), 4);
        assert_eq!(page.record().cells.len(), 2);
        for cell in &page.record().cells {
            assert_eq!(cell.outputs.len(), 1);
            let output = &cell.outputs[0];
            assert_eq!(
                output.output.representations.len(),
                if cell.ordinal == 0 { 3 } else { 1 }
            );
            for index in 0..output.output.representations.len() {
                let Some(ValidatedRepresentationRef::Asset(asset)) =
                    page.representation(cell.ordinal, output.slot, index)
                else {
                    panic!("missing restored figure");
                };
                let stored = &loaded.assets()[&asset.reference.fingerprint.value];
                assert_eq!(asset.media_type, stored.media_type);
                assert_eq!(asset.byte_size, stored.bytes.len() as u64);
                paths_by_digest
                    .entry(asset.reference.fingerprint.value.clone())
                    .or_default()
                    .insert(asset.reference.path.clone());
            }
        }
    }
    assert_eq!(paths_by_digest.len(), 4);
    assert!(paths_by_digest.values().all(|paths| paths.len() == 2));
    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    let visible_digest = fingerprint_bytes(visible.as_bytes()).value;
    assert!(html.contains(&format!("src=\"../assets/{visible_digest}%2Esvg\"")));
    for (extension, _, bytes) in &expected[..3] {
        let digest = fingerprint_bytes(bytes).value;
        assert!(!html.contains(&format!("src=\"../assets/{digest}%2E{extension}\"")));
    }
    let png_digest = fingerprint_bytes(&expected[1].2).value;
    assert!(html.contains(&format!("href=\"../assets/{png_digest}%2Epng\"")));
}
