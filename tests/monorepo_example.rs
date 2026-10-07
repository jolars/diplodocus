mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use diplodocus::snapshots::Snapshot;

fn run(root: &Path, arguments: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root)
        .args(arguments)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn monorepo_example_builds_without_runtimes_and_generates_without_sources() {
    let root = support::TestWorkspace::new();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/monorepo");
    root.write(
        "diplodocus.toml",
        fs::read(example.join("diplodocus.toml")).unwrap(),
    );
    for directory in ["docs", "python", "r", "julia"] {
        for relative in support::files_under(&example.join(directory)) {
            let relative = Path::new(directory).join(relative);
            root.write(&relative, fs::read(example.join(&relative)).unwrap());
        }
    }

    run(root.path(), &["check"]);
    assert!(!root.path().join(".diplodocus").exists());
    run(root.path(), &["build"]);
    assert!(!root.path().join(".diplodocus/cache").exists());
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = Snapshot::load(&database).unwrap();
    assert_eq!(snapshot.workspace().repositories.len(), 1);
    assert_eq!(snapshot.workspace().packages.len(), 3);
    assert_eq!(snapshot.workspace().pages.len(), 9);
    assert_eq!(snapshot.workspace().concepts.len(), 6);
    assert_eq!(snapshot.assets().len(), 1);

    let site = root.path().join("site");
    let index = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(index.contains("Comparing predictions"));
    let guide = fs::read_to_string(site.join("guides/comparing-predictions.html")).unwrap();
    assert!(guide.contains("<img "));
    let search: serde_json::Value =
        serde_json::from_slice(&fs::read(site.join("assets/search.json")).unwrap()).unwrap();
    let entries = search.as_array().unwrap();
    let api_paths: Vec<_> = [
        ("python", "tinystats.mean_squared_error"),
        ("r", "mean_squared_error"),
        ("julia", "TinyStats.mean_squared_error"),
    ]
    .into_iter()
    .map(|(ecosystem, name)| {
        let entry = entries
            .iter()
            .find(|entry| entry["title"] == name && entry["ecosystem"] == ecosystem)
            .unwrap();
        entry["path"].as_str().unwrap()
    })
    .collect();
    for path in &api_paths {
        assert!(guide.contains(&format!("href=\"../{path}\"")));
        let file = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .unwrap();
        let html = fs::read_to_string(site.join(file.as_ref())).unwrap();
        assert!(html.contains("Same API in"));
        assert!(html.contains("predicted"));
        assert!(html.contains("mean squared"));
        let other = api_paths.iter().find(|other| other != &path).unwrap();
        let base = url::Url::parse("https://docs.example/").unwrap();
        let relative = base
            .join(path)
            .unwrap()
            .make_relative(&base.join(other).unwrap())
            .unwrap();
        assert!(html.contains(&format!("href=\"{relative}\"")));
    }
    for metric in [
        "mean_absolute_error",
        "root_mean_squared_error",
        "mean_error",
        "r_squared",
        "residuals",
    ] {
        for (ecosystem, title) in [
            ("python", format!("tinystats.{metric}")),
            ("r", metric.into()),
            ("julia", format!("TinyStats.{metric}")),
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry["title"] == title && entry["ecosystem"] == ecosystem)
                .unwrap();
            let path = percent_encoding::percent_decode_str(entry["path"].as_str().unwrap())
                .decode_utf8()
                .unwrap();
            let html = fs::read_to_string(site.join(path.as_ref())).unwrap();
            assert!(html.contains("Same API in"), "{title}");
        }
    }
    assert!(index.contains("aria-label=\"Getting started\""));
    assert!(index.contains("aria-label=\"Guides\""));
    assert!(index.contains("aria-label=\"Reference\""));
    let python_guide =
        fs::read_to_string(site.join("packages/python/guides/evaluating-models.html")).unwrap();
    assert!(python_guide.contains("Evaluating models in Python"));
    for asset in snapshot.assets().values() {
        assert!(support::files_under(&site).iter().any(|path| {
            path.extension().is_some_and(|extension| extension == "svg")
                && fs::read(site.join(path)).unwrap() == asset.bytes
        }));
    }

    let portable = tempfile::tempdir().unwrap();
    fs::copy(&database, portable.path().join("documentation.sqlite")).unwrap();
    run(root.path(), &["extract", "--output", "separate.sqlite"]);
    assert_eq!(
        snapshot.canonical_export().unwrap(),
        Snapshot::load(root.path().join("separate.sqlite"))
            .unwrap()
            .canonical_export()
            .unwrap()
    );
    let expected: Vec<_> = support::files_under(&site)
        .into_iter()
        .map(|path| {
            let bytes = fs::read(site.join(&path)).unwrap();
            (path, bytes)
        })
        .collect();
    drop(root);

    run(
        portable.path(),
        &["generate", "--input", "documentation.sqlite"],
    );
    let generated = portable.path().join("site");
    assert_eq!(support::files_under(&generated).len(), expected.len());
    for (path, bytes) in expected {
        assert_eq!(fs::read(generated.join(path)).unwrap(), bytes);
    }
}
