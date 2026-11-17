mod support;

use std::sync::{Arc, Mutex};

use diplodocus::commands::{BuildOptions, CommandRunner};
use diplodocus::incremental::CachePolicy;
use diplodocus::reporting::{Event, Reporter};
use diplodocus::snapshots::Snapshot;

fn r_workspace(packages: &[&str]) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    let mut configuration = "project:\n  name: Native cache regression\nrepositories:\n  - id: repo\n    path: .\npackages:\n".to_owned();
    for package in packages {
        configuration.push_str(&format!("  - id: {package}\n    name: {package}\n    slug: {package}\n    ecosystem: r\n    repository: repo\n    path: {package}\n    metadata-path: DESCRIPTION\n    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n"));
        root.write(format!("{package}/DESCRIPTION"), format!("Package: {package}\nVersion: 0.1.0\nTitle: Native Cache Fixture\nDescription: Exercises native execution cache dependencies.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <example@example.com>\nEncoding: UTF-8\n"));
        root.write(format!("{package}/NAMESPACE"), "export(answer)\n");
        root.write(format!("{package}/R/api.R"), "answer <- function() 42L\n");
        root.write(format!("{package}/man/answer.Rd"), "\\name{answer}\n\\alias{answer}\n\\title{Answer}\n\\usage{answer()}\n\\description{An answer.}\n\\examples{plot(1:3)}\n");
    }
    root.write("diplodocus.yaml", configuration);
    root
}

fn cli_build(root: &support::TestWorkspace) -> Snapshot {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("R_LIBS_USER", root.path().join("library"))
        .args(["build", "--output", "site"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn shared_figures_remain_self_contained_in_each_cached_package() {
    let root = r_workspace(&["first", "second"]);
    let first = cli_build(&root);
    let images: Vec<_> = first
        .assets()
        .iter()
        .filter(|(_, asset)| asset.media_type == "image/png")
        .map(|(digest, _)| digest.clone())
        .collect();
    assert_eq!(
        images.len(),
        1,
        "The packages should produce the same figure."
    );
    root.write(
        "first/man/answer.Rd",
        root.read("first/man/answer.Rd")
            .replace("plot(1:3)", "plot(3:1)"),
    );
    let changed = cli_build(&root);
    assert!(changed.assets().contains_key(&images[0]));
    assert_eq!(
        changed
            .assets()
            .values()
            .filter(|asset| asset.media_type == "image/png")
            .count(),
        2
    );
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn bibliography_changes_invalidate_articles_without_reexecuting_native_examples() {
    let root = r_workspace(&["demo"]);
    root.write("demo/vignettes/article.Rmd", "---\ntitle: Citation cache\nbibliography: references.bib\n---\nCitation [@example].\n\n```{r, echo=FALSE}\ncat('executed\\n', file = '../bibliography-count', append = TRUE)\n```\n");
    root.write(
        "demo/man/answer.Rd",
        root.read("demo/man/answer.Rd").replace(
            "\\examples{",
            "\\examples{cat('executed\\n', file = 'rd-count', append = TRUE)\n",
        ),
    );
    root.write("demo/vignettes/references.bib", "@book{example, author={Alice Alpha}, title={An Example}, year={2020}, publisher={Press}}\n");
    cli_build(&root);
    let original = root.read("site/packages/demo/vignettes/article.html");
    let executed = root.read("demo/bibliography-count");
    let rd_executed = root.read("demo/rd-count");
    assert!(original.contains("Alpha"));
    cli_build(&root);
    assert_eq!(executed, root.read("demo/bibliography-count"));
    assert_eq!(
        original,
        root.read("site/packages/demo/vignettes/article.html")
    );
    root.write(
        "demo/vignettes/references.bib",
        "@book{example, author={Betty Beta}, title={An Example}, year={2024}, publisher={Press}}\n",
    );
    cli_build(&root);
    let changed = root.read("site/packages/demo/vignettes/article.html");
    assert!(changed.contains("Beta"));
    assert_ne!(original, changed);
    assert_eq!(rd_executed, root.read("demo/rd-count"));
    let executed = root.read("demo/bibliography-count");
    cli_build(&root);
    assert_eq!(executed, root.read("demo/bibliography-count"));
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn citation_loaded_dependencies_guard_cached_package_metadata() {
    let root = r_workspace(&["demo"]);
    root.write("dependency/DESCRIPTION", "Package: cachecitation\nVersion: 0.1.0\nTitle: Citation Dependency\nDescription: Supplies author text for citation cache validation.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <example@example.com>\n");
    root.write("dependency/NAMESPACE", "export(author)\n");
    root.write("dependency/R/api.R", "author <- function() readLines(system.file('author.txt', package = 'cachecitation'), warn = FALSE)\n");
    root.write("dependency/inst/author.txt", "First\n");
    for library in ["library", "alternate"] {
        std::fs::create_dir(root.path().join(library)).unwrap();
        let output = std::process::Command::new("R")
            .current_dir(root.path())
            .args([
                "CMD",
                "INSTALL",
                "--no-byte-compile",
                "--no-help",
                "--no-html",
                "--no-test-load",
                "-l",
            ])
            .arg(root.path().join(library))
            .arg(root.path().join("dependency"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    root.write("library/cachecitation/author.txt", "Default\n");
    root.write("demo/inst/CITATION", "cat('executed\\n', file = 'metadata-count', append = TRUE)\nloadNamespace('cachecitation', lib.loc = '../alternate')\nbibentry('Book', title = 'An Example', author = person('Example', cachecitation::author()), year = '2020', publisher = 'Press')\n");
    let citation = |snapshot: &Snapshot| {
        let Some(diplodocus::ir::PackageMetadata::R(metadata)) =
            &snapshot.workspace().packages["demo"].metadata
        else {
            panic!("R metadata")
        };
        metadata
            .runtime
            .as_ref()
            .unwrap()
            .citation
            .as_ref()
            .unwrap()
            .text
            .clone()
    };
    assert!(citation(&cli_build(&root)).contains("First"));
    let executed = root.read("demo/metadata-count");
    assert!(citation(&cli_build(&root)).contains("First"));
    assert_eq!(executed, root.read("demo/metadata-count"));
    root.write("alternate/cachecitation/author.txt", "Second\n");
    assert!(citation(&cli_build(&root)).contains("Second"));
    let executed = root.read("demo/metadata-count");
    assert!(citation(&cli_build(&root)).contains("Second"));
    assert_eq!(executed, root.read("demo/metadata-count"));
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn absent_suggested_dependencies_allow_reuse_and_later_installation_invalidates_it() {
    let root = r_workspace(&["demo"]);
    std::fs::create_dir(root.path().join("library")).unwrap();
    root.write(
        "demo/DESCRIPTION",
        root.read("demo/DESCRIPTION") + "Suggests: cacheoptional\n",
    );
    root.write("demo/inst/CITATION", "cat('executed\\n', file = 'metadata-count', append = TRUE)\nbibentry('Book', title = 'An Example', author = person('Example', 'Author'), year = '2020', publisher = 'Press')\n");
    root.write(
        "demo/man/answer.Rd",
        root.read("demo/man/answer.Rd").replace(
            "plot(1:3)",
            "cat('executed\\n', file = 'rd-count', append = TRUE)\nanswer()",
        ),
    );
    cli_build(&root);
    let metadata = root.read("demo/metadata-count");
    let executed = root.read("demo/rd-count");
    cli_build(&root);
    assert_eq!(metadata, root.read("demo/metadata-count"));
    assert_eq!(executed, root.read("demo/rd-count"));

    root.write("dependency/DESCRIPTION", "Package: cacheoptional\nVersion: 0.1.0\nTitle: Optional Cache Dependency\nDescription: Exercises installation after a completed execution.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <example@example.com>\n");
    root.write("dependency/NAMESPACE", "export(value)\n");
    root.write("dependency/R/api.R", "value <- function() 7L\n");
    let installed = std::process::Command::new("R")
        .current_dir(root.path())
        .args([
            "CMD",
            "INSTALL",
            "--no-byte-compile",
            "--no-help",
            "--no-html",
            "--no-test-load",
            "-l",
        ])
        .arg(root.path().join("library"))
        .arg(root.path().join("dependency"))
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    cli_build(&root);
    assert!(root.read("demo/metadata-count").len() > metadata.len());
    assert!(root.read("demo/rd-count").len() > executed.len());
    let metadata = root.read("demo/metadata-count");
    let executed = root.read("demo/rd-count");
    cli_build(&root);
    assert_eq!(metadata, root.read("demo/metadata-count"));
    assert_eq!(executed, root.read("demo/rd-count"));
}

#[derive(Default)]
struct Events(Mutex<Vec<(String, bool)>>);
impl Reporter for Events {
    fn report(&self, event: Event<'_>) {
        if let Event::Cache { stage, hit, .. } = event {
            self.0.lock().unwrap().push((stage.into(), hit));
        }
    }
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn native_execution_reuses_completed_results_and_figures() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Cached R execution\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Demo\n    slug: demo\n    ecosystem: r\n    repository: repo\n    path: package\n    metadata-path: DESCRIPTION\n    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n");
    root.write("package/DESCRIPTION", "Package: cachedexecution\nVersion: 0.1.0\nTitle: Cached Execution\nDescription: A native execution cache fixture.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <example@example.com>\nEncoding: UTF-8\n");
    root.write("package/NAMESPACE", "export(answer)\n");
    root.write("package/R/api.R", "answer <- function() 42L\n");
    root.write("package/man/answer.Rd", "\\name{answer}\n\\alias{answer}\n\\title{Answer}\n\\usage{answer()}\n\\description{An answer.}\n\\examples{\ncat('executed\\n', file = 'rd-count', append = TRUE)\nanswer()\nplot(1:3)\n}\n");
    root.write("package/vignettes/article.Rmd", "---\ntitle: Cached article\n---\n\n# Article\n\n```{r}\ncat('executed\\n', file = '../article-count', append = TRUE)\nanswer()\nplot(1:3)\n```\n");
    let output = tempfile::tempdir().unwrap();
    let events = Arc::new(Events::default());
    let build = |name: &str, policy| {
        events.0.lock().unwrap().clear();
        CommandRunner::with_reporter(events.clone())
            .with_cache_policy(policy)
            .build(BuildOptions {
                config: root.path().join("diplodocus.yaml"),
                output: output.path().join(name),
            })
            .unwrap();
        events.0.lock().unwrap().clone()
    };
    build("first", CachePolicy::Auto);
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
        .unwrap()
        .canonical_export()
        .unwrap();
    let rd = root.read("package/rd-count");
    let article = root.read("package/article-count");
    let warm = build("warm", CachePolicy::Auto);
    for stage in ["r-metadata", "rd-execution", "rmd-execution"] {
        assert!(warm.contains(&(stage.into(), true)), "{stage}: {warm:?}");
    }
    assert_eq!(rd, root.read("package/rd-count"));
    assert_eq!(article, root.read("package/article-count"));
    assert_eq!(
        snapshot,
        Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
            .unwrap()
            .canonical_export()
            .unwrap()
    );
    support::assert_output_tree(&output.path().join("first"), &output.path().join("warm"));
    root.write(
        "package/R/api.R",
        "# Changed source, including evaluated package inputs.\nanswer <- function() 42L\n",
    );
    let changed = build("changed", CachePolicy::Auto);
    assert!(changed.contains(&("rd-execution".into(), false)));
    assert!(root.read("package/rd-count").len() > rd.len());
    let fresh = build("fresh", CachePolicy::Bypass);
    assert!(fresh.iter().all(|(_, hit)| !hit));
    support::assert_output_tree(&output.path().join("changed"), &output.path().join("fresh"));
}
