mod support;

use diplodocus::commands::{BuildOptions, CommandRunner};
use diplodocus::incremental::CachePolicy;
use diplodocus::snapshots::Snapshot;

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn conventional_output_names_can_contain_maintained_packages() {
    for directory in ["site", "artifacts"] {
        let root = support::TestWorkspace::new();
        root.write("diplodocus.yaml", format!("project:\n  name: Maintained package\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: example\n    name: Example\n    slug: example\n    ecosystem: python\n    repository: repo\n    path: {directory}\n    metadata-path: pyproject.toml\n    targets:\n      - id: api\n        extractor: python\n        path: src/example\n        role: public-api\n"));
        root.write(
            format!("{directory}/pyproject.toml"),
            "[project]\nname = 'example'\nversion = '1.0.0'\n",
        );
        let source = format!("{directory}/src/example/__init__.py");
        root.write(&source, "def value(number: int = 1) -> int:\n    \"\"\"First documentation.\"\"\"\n    return number\n");
        let options = BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("public"),
        };
        let runner = CommandRunner::default();
        runner.build(options.clone()).unwrap();
        root.write(&source, "def value(number: int = 2) -> int:\n    \"\"\"Updated documentation.\"\"\"\n    return number\n");
        runner.build(options.clone()).unwrap();
        let snapshot = root.path().join(".diplodocus/documentation.sqlite");
        let incremental = Snapshot::load(&snapshot)
            .unwrap()
            .canonical_export()
            .unwrap();
        assert!(
            incremental.contains("Updated documentation."),
            "{directory}"
        );
        assert!(!incremental.contains("First documentation."), "{directory}");
        runner
            .with_cache_policy(CachePolicy::Bypass)
            .build(options)
            .unwrap();
        assert_eq!(
            incremental,
            Snapshot::load(snapshot)
                .unwrap()
                .canonical_export()
                .unwrap(),
            "{directory}"
        );
    }
}
