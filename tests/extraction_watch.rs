mod support;

use std::path::PathBuf;
use std::time::Duration;

use diplodocus::commands::{ExtractOptions, watch_extract_with};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::snapshots::Snapshot;
use tokio::sync::mpsc;

async fn publication(receiver: &mut mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(20), receiver.recv())
        .await
        .expect("extraction did not publish")
        .expect("watcher stopped")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watches_sibling_packages_preserves_failures_and_removes_deleted_items() {
    let root = support::TestWorkspace::new();
    root.write("docs/diplodocus.yaml", "project:\n  name: \"Watch\"\nrepositories:\n  - id: \"package\"\n    path: \"../package\"\npackages:\n  - id: \"example\"\n    name: \"Example\"\n    slug: \"example\"\n    ecosystem: \"python\"\n    repository: \"package\"\n    path: \".\"\n    metadata-path: \"pyproject.toml\"\n    targets:\n      - id: \"api\"\n        extractor: \"python\"\n        path: \"src/example\"\n        role: \"public-api\"\n");
    root.write(
        "package/pyproject.toml",
        "[project]\nname='example'\nversion='1.0.0'\n",
    );
    let source = "def first():\n    \"\"\"Original documentation.\"\"\"\n    return 1\n";
    root.write("package/src/example/__init__.py", source);
    let output = root.path().join("package/src/example/export.py");
    let (published, mut publications) = mpsc::unbounded_channel();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let expected_output = output.clone();
    let task = tokio::spawn(watch_extract_with(
        ExtractOptions {
            config: root.path().join("docs/diplodocus.yaml"),
            output: Some(output.clone()),
        },
        ExecutionDeadlines::default(),
        Box::pin(async {
            let _ = stopped.await;
        }),
        move |path| {
            assert_eq!(path, expected_output);
            published
                .send(Snapshot::load(path).unwrap().canonical_export().unwrap())
                .unwrap();
            Ok(())
        },
    ));
    let original = publication(&mut publications).await;
    assert!(original.contains("Original documentation."));
    assert!(!root.path().join("site").exists());

    root.write(
        "package/src/example/__init__.py",
        source.replace("Original", "Updated"),
    );
    assert!(
        publication(&mut publications)
            .await
            .contains("Updated documentation.")
    );

    root.write(
        "package/src/example/extra.py",
        "def added():\n    \"\"\"Newly added API.\"\"\"\n    return 2\n",
    );
    assert!(
        publication(&mut publications)
            .await
            .contains("Newly added API.")
    );
    root.remove("package/src/example/extra.py");
    let complete = publication(&mut publications).await;
    assert!(!complete.contains("Newly added API."));

    let bytes = std::fs::read(&output).unwrap();
    root.write("docs/diplodocus.yaml", "project: [");
    assert!(
        tokio::time::timeout(Duration::from_millis(900), publications.recv())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&output).unwrap(), bytes);

    root.write("docs/diplodocus.yaml", "project:\n  name: \"Recovered\"\nrepositories:\n  - id: \"package\"\n    path: \"../package\"\npackages:\n  - id: \"example\"\n    name: \"Example\"\n    slug: \"example\"\n    ecosystem: \"python\"\n    repository: \"package\"\n    path: \".\"\n    metadata-path: \"pyproject.toml\"\n    targets:\n      - id: \"api\"\n        extractor: \"python\"\n        path: \"src/example\"\n        role: \"public-api\"\n");
    assert!(publication(&mut publications).await.contains("Recovered"));
    root.write("docs/guide.md", "# Host-only guide\n");
    root.write("docs/.diplodocus/ignored.py", "generated artifact");
    assert!(
        tokio::time::timeout(Duration::from_millis(700), publications.recv())
            .await
            .is_err()
    );

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn initial_failure_returns_without_publishing() {
    let root = support::TestWorkspace::new();
    let result = watch_extract_with(
        ExtractOptions {
            config: root.path().join("missing.yaml"),
            output: Some(PathBuf::from("unused.sqlite")),
        },
        ExecutionDeadlines::default(),
        Box::pin(std::future::pending()),
        |_| panic!("failed extraction must not publish"),
    )
    .await;
    assert!(result.is_err());
}
