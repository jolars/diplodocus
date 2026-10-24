mod support;

use std::fs;
use std::path::Path;
use std::process::Command;

use diplodocus::snapshots::Snapshot;

fn run(root: &Path, arguments: &[&str]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    command.current_dir(root).args(arguments);
    if arguments.first() == Some(&"generate") {
        command.env("PATH", "");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn monorepo_example_builds_with_r_and_generates_without_sources_or_runtimes() {
    let root = support::TestWorkspace::new();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/monorepo");
    root.write(
        "diplodocus.yaml",
        fs::read(example.join("diplodocus.yaml")).unwrap(),
    );
    for directory in ["docs", "python", "r", "julia", "rust"] {
        for relative in support::files_under(&example.join(directory)) {
            let relative = Path::new(directory).join(relative);
            root.write(&relative, fs::read(example.join(&relative)).unwrap());
        }
    }

    run(root.path(), &["check"]);
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
    assert!(root.path().join(".diplodocus/cache/rust").is_dir());
    run(root.path(), &["build"]);
    assert!(!root.path().join(".diplodocus/cache/execution").exists());
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = Snapshot::load(&database).unwrap();
    assert_eq!(snapshot.workspace().repositories.len(), 1);
    assert_eq!(snapshot.workspace().packages.len(), 4);
    assert_eq!(snapshot.workspace().pages.len(), 19);
    assert_eq!(snapshot.workspace().concepts.len(), 6);
    assert_eq!(snapshot.assets().len(), 4);
    assert_eq!(
        snapshot
            .assets()
            .values()
            .filter(|asset| asset.media_type == "image/png")
            .count(),
        2
    );
    let logo = snapshot.logo().unwrap();
    assert_eq!(
        snapshot.assets()[&logo.fingerprint.value].bytes,
        fs::read(example.join("docs/assets/logo.svg")).unwrap()
    );

    let Some(diplodocus::ir::PackageMetadata::R(metadata)) =
        &snapshot.workspace().packages["rstats"].metadata
    else {
        panic!("R metadata");
    };
    assert_eq!(metadata.description.name, "tinystats");
    assert_eq!(metadata.description.title, "A Small Statistics Example");
    let runtime = metadata.runtime.as_ref().unwrap();
    assert!(runtime.authors[0].contains("Diplodocus Contributors"));
    assert!(runtime.citation.as_ref().unwrap().bibtex.contains("2026"));
    let site = root.path().join("site");
    for (language, id, published) in [
        ("python", "pystats", "tinystats"),
        ("julia", "juliastats", "TinyStats"),
        ("rust", "ruststats", "tinystats"),
    ] {
        let metadata = snapshot.workspace().packages[id].metadata.as_ref().unwrap();
        assert_eq!(metadata.name(), published);
        assert_eq!(metadata.version(), Some("0.1.0"));
        let overview =
            fs::read_to_string(site.join(format!("packages/{language}/index.html"))).unwrap();
        assert!(overview.contains("Package information"));
        assert!(overview.contains("Compare predictions"));
        assert_eq!(overview.matches("<h1").count(), 1);
    }
    let overview = fs::read_to_string(site.join("packages/r/index.html")).unwrap();
    assert!(overview.contains("vignettes/evaluating-predictions.html"));
    let vignette =
        fs::read_to_string(site.join("packages/r/vignettes/evaluating-predictions.html")).unwrap();
    for text in [
        "Evaluating predictions in R",
        "The mean squared error is 1",
        "mean absolute error is 0.5",
        "<table>",
        "<img ",
        "The last prediction accounts for all of the error.",
    ] {
        assert!(vignette.contains(text), "{text}");
    }
    for text in [
        "Compare predictions",
        "Package information",
        "Project website",
        "Report an issue",
        "mean_absolute_error",
        "<img ",
    ] {
        assert!(overview.contains(text), "{text}");
    }
    for (file, content) in [
        ("news.html", "tinystats 0.1.0"),
        ("authors.html", "Tiny Stats: Prediction Metrics for R"),
        ("license.html", "Permission is hereby granted"),
        ("community.html", "Contributing to Tiny Stats"),
    ] {
        assert!(overview.contains(&format!("href=\"{}\"", file)));
        assert!(
            fs::read_to_string(site.join("packages/r").join(file))
                .unwrap()
                .contains(content)
        );
    }
    let index = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(index.contains("data-layout=\"home\""));
    assert!(index.contains(">Small metrics.\nClearer predictions.</h1>"));
    assert_eq!(index.matches("<h1").count(), 1);
    assert!(index.contains("class=\"home-content\""));
    assert!(index.contains("Compare observations and predictions"));
    assert!(index.contains("href=\"overview.html\">Guide</a>"));
    let get_started = index
        .split_once("<a class=\"home-action\" href=\"getting-started/quick-start.html\">")
        .expect("home action links to the quick start")
        .1
        .split_once("</a>")
        .expect("home action has a closing tag")
        .0;
    assert!(get_started.contains("Get started"));
    assert!(index.contains("Comparing predictions"));
    let introduction = fs::read_to_string(site.join("overview.html")).unwrap();
    assert!(introduction.contains("data-layout=\"docs\""));
    assert!(introduction.contains("<h1>Tiny Stats</h1>"));
    assert!(introduction.contains("href=\"index.html\""));
    let guide = fs::read_to_string(site.join("guides/comparing-predictions.html")).unwrap();
    assert!(guide.contains("<img "));
    let search: serde_json::Value =
        serde_json::from_slice(&fs::read(site.join("assets/search.json")).unwrap()).unwrap();
    let entries = search.as_array().unwrap();
    let api_paths: Vec<_> = [
        ("python", "tinystats.mean_squared_error"),
        ("r", "mean_squared_error"),
        ("julia", "TinyStats.mean_squared_error"),
        ("rust", "tinystats::mean_squared_error"),
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
        let markdown = fs::read_to_string(site.join(file.replace(".html", ".md"))).unwrap();
        assert!(markdown.contains("## Same API in"));
        assert!(markdown.contains(&format!("](<{relative}>)")));
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
            ("rust", format!("tinystats::{metric}")),
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
    assert!(index.contains("aria-label=\"Sections\""));
    assert!(!index.contains("aria-label=\"API reference\""));
    let python_guide =
        fs::read_to_string(site.join("packages/python/guides/evaluating-models.html")).unwrap();
    assert!(python_guide.contains("Evaluating models in Python"));
    assert!(python_guide.contains("aria-label=\"API reference\""));
    assert!(!python_guide.contains("aria-label=\"Getting started\""));
    for asset in snapshot.assets().values() {
        let extension = match asset.media_type.as_str() {
            "image/svg+xml" => "svg",
            "image/png" => "png",
            other => panic!("unexpected example asset: {other}"),
        };
        assert!(support::files_under(&site).iter().any(|path| {
            path.extension().is_some_and(|value| value == extension)
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
