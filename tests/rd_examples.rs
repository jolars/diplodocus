mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::{Block, ProvenanceActivity};
use diplodocus::snapshots::Snapshot;
use std::fs;
use std::process::{Command, Output};

fn workspace(examples: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        r#"
[project]
name = "Rd examples"
[[repository]]
id = "r"
path = "r"
[[package]]
id = "rdex"
name = "Rd example package"
slug = "rdex"
ecosystem = "r"
repository = "r"
path = "."
metadata-path = "DESCRIPTION"
targets = [{ id = "api", extractor = "r", path = ".", role = "public-api" }]
"#,
    );
    root.write("r/DESCRIPTION", "Package: rdex\nVersion: 0.1.0\nTitle: Rd examples\nDescription: A working example package.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <author@example.com>\nEncoding: UTF-8\n");
    root.write("r/NAMESPACE", "export(answer)\nexport(other)\n");
    root.write(
        "r/R/api.R",
        "answer <- function() 42L\nother <- function() 24L\n",
    );
    root.write("r/man/answer.Rd", format!("\\name{{answer}}\n\\alias{{answer}}\n\\title{{Answer}}\n\\usage{{answer()}}\n\\description{{An answer.}}\n\\examples{{\n{examples}\n}}\n"));
    root.write("r/man/other.Rd", "\\name{other}\n\\alias{other}\n\\title{Other}\n\\usage{other()}\n\\description{Another answer.}\n\\examples{stopifnot(!exists('shared', inherits = FALSE))}\n");
    root
}

fn extract(root: &support::TestWorkspace) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args(["extract", "--config", "diplodocus.toml"])
        .output()
        .unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn default_execution_retains_results_figures_illustrative_regions_and_skips() {
    let root = workspace(
        r#"
stopifnot(!exists("root", envir = globalenv(), inherits = FALSE))
shared <- answer()
\dontrun{stop("illustrative code must stay inert")}
\donttest{shared <- shared + 1L}
stopifnot(shared == 43L)
if (1) shared <- shared + 0L
shared
if (interactive()) stop("interactive branch must be skipped")
if (FALSE) stop("false branch must be skipped")
plot(1:3)
lines(3:1)
message("example message")
warning("example warning")
"#,
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert!(
        sources.workspace().packages["rdex"]
            .items
            .values()
            .all(|item| item
                .documentation
                .as_ref()
                .unwrap()
                .provenance
                .iter()
                .all(|p| !matches!(p.activity, ProvenanceActivity::RdExamples { .. })))
    );
    drop(sources);
    success(extract(&root));
    let path = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = Snapshot::load(&path).unwrap();
    let item = snapshot.workspace().packages["rdex"]
        .items
        .values()
        .find(|item| item.name == "answer")
        .unwrap();
    let document = item.documentation.as_ref().unwrap();
    let output = document
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::CodeBlock {
                language: None,
                source,
                ..
            } => Some(source.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(output.contains("[1] 43"), "{output}");
    assert!(output.contains("example message"));
    assert!(output.contains("example warning"));
    let record = document
        .provenance
        .iter()
        .find_map(|record| match &record.activity {
            ProvenanceActivity::RdExamples {
                r_version,
                package_version,
                dependencies,
                executed,
                illustrative,
                skipped,
                ..
            } => Some((
                r_version,
                package_version,
                dependencies,
                executed,
                illustrative,
                skipped,
            )),
            _ => None,
        })
        .unwrap();
    assert!(!record.0.is_empty());
    assert_eq!(record.1, "0.1.0");
    assert!(record.2.contains_key("evaluate"));
    assert!(record.2.contains_key("pkgload"));
    assert!(record.3.iter().any(|code| code.contains("shared + 1L")));
    assert_eq!(record.4.len(), 1);
    assert!(record.4[0].contains("illustrative code"));
    assert!(
        record
            .5
            .iter()
            .any(|reason| reason.contains("interactive()"))
    );
    assert!(record.5.iter().any(|reason| reason.contains("FALSE")));
    assert_eq!(record.5.len(), 2);
    assert!(!snapshot.assets().is_empty());
    assert!(
        snapshot
            .assets()
            .values()
            .all(|asset| asset.media_type == "image/png")
    );
    let portable = snapshot.canonical_export().unwrap();
    assert!(!portable.contains(root.path().to_str().unwrap()));
    assert!(!root.path().join(".diplodocus/cache").exists());
    fs::remove_dir_all(root.path().join("r")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args([
            "generate",
            "--input",
            ".diplodocus/documentation.sqlite",
            "--output",
            "site",
        ])
        .env("PATH", "")
        .output()
        .unwrap();
    success(result);
    assert_eq!(
        Snapshot::load(path).unwrap().canonical_export().unwrap(),
        portable
    );
}

#[test]
fn failed_examples_and_missing_dependencies_preserve_the_previous_snapshot() {
    let root = workspace("answer()");
    success(extract(&root));
    let path = root.path().join(".diplodocus/documentation.sqlite");
    let previous = fs::read(&path).unwrap();
    for code in [
        r#"stop("unexpected example error")"#,
        r#"\donttest{stop("donttest must execute")}"#,
        r#"if (requireNamespace("diplodocus_missing_dependency", quietly = TRUE)) answer()"#,
        r#"if (base::requireNamespace("diplodocus_missing_dependency", quietly = TRUE)) answer()"#,
    ] {
        root.write("r/man/answer.Rd", format!("\\name{{answer}}\\alias{{answer}}\\title{{Answer}}\\usage{{answer()}}\\description{{An answer.}}\\examples{{{code}}}"));
        let output = extract(&root);
        assert!(!output.status.success());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(errors.contains("Rd example execution failed"), "{errors}");
        assert!(errors.contains("r:man/answer.Rd"), "{errors}");
        assert_eq!(fs::read(&path).unwrap(), previous);
    }
}

#[test]
fn loading_requires_prebuilt_extensions_and_never_compiles_them() {
    let root = workspace("answer()");
    root.write(
        "r/NAMESPACE",
        "export(answer)\nexport(other)\nuseDynLib(rdex)\n",
    );
    root.write(
        "r/src/native.c",
        "#error Diplodocus must not compile package code\n",
    );
    let output = extract(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Rd example execution failed"));
    assert!(!root.path().join("r/src/native.o").exists());
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
}

#[test]
fn shared_topics_execute_once_and_custom_example_headings_stay_display_content() {
    let root = workspace("cat('run\\n', file = 'example-runs', append = TRUE)\nanswer()");
    root.remove("r/man/other.Rd");
    let rd = root
        .read("r/man/answer.Rd")
        .replace("\\alias{answer}", "\\alias{answer}\\alias{other}")
        .replace("\\usage{answer()}", "\\usage{answer()\nother()}")
        .replace(
            "\\examples{",
            "\\section{Examples}{Illustrative section prose.}\n\\examples{",
        );
    root.write("r/man/answer.Rd", rd);
    success(extract(&root));
    assert_eq!(root.read("r/example-runs"), "run\n");
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let mut documents = snapshot.workspace().packages["rdex"]
        .items
        .values()
        .map(|item| item.documentation.as_ref().unwrap());
    let first = documents.next().unwrap();
    assert_eq!(first, documents.next().unwrap());
    let index = first.document.blocks.iter().position(|block| matches!(block, Block::Heading { attributes, .. } if attributes.classes.iter().any(|class| class.value == "rd-examples"))).unwrap();
    assert!(
        matches!(&first.document.blocks[index + 2], Block::CodeBlock { language: None, source, .. } if source.contains("[1] 42"))
    );
}

#[tokio::test]
async fn native_execution_is_bounded_and_cancellation_reaps_the_helper() {
    use diplodocus::assembly::AssemblyError;
    use diplodocus::execution::{ExecutionCancellation, ExecutionDeadlines};
    let root = workspace("Sys.sleep(60)");
    let mut sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
    let error = sources
        .execute_rd_examples(
            ExecutionDeadlines {
                cell: 100,
                ..ExecutionDeadlines::default()
            },
            &mut cancellation,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, AssemblyError::Diagnostics(ref diagnostics) if diagnostics[0].message.contains("timed out"))
    );
    let mut sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let mut cancellation: ExecutionCancellation<'_> =
        Box::pin(async { tokio::time::sleep(std::time::Duration::from_millis(100)).await });
    let error = sources
        .execute_rd_examples(ExecutionDeadlines::default(), &mut cancellation)
        .await
        .unwrap_err();
    assert!(matches!(error, AssemblyError::Cancelled));
    assert!(!root.path().join(".diplodocus").exists());
}

#[test]
fn changed_checkout_identity_and_unload_errors_prevent_publication() {
    let root = workspace("answer()");
    success(extract(&root));
    let snapshot = root.path().join(".diplodocus/documentation.sqlite");
    let previous = fs::read(&snapshot).unwrap();
    let original = root.read("r/man/answer.Rd");
    root.write(
        "r/man/answer.Rd",
        original.replace(
            "\\examples{\nanswer()",
            "\\examples{\nsetNamespaceInfo(asNamespace('rdex'), 'path', tempdir())\nanswer()",
        ),
    );
    let output = extract(&root);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Loaded R package does not match the supplied checkout")
    );
    assert_eq!(fs::read(&snapshot).unwrap(), previous);
    root.write("r/man/answer.Rd", original);
    root.write("r/R/api.R", "answer <- function() 42L\nother <- function() 24L\n.onUnload <- function(libpath) stop('cleanup failed')\n");
    let output = extract(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cleanup failed"));
    assert_eq!(fs::read(snapshot).unwrap(), previous);
}

#[test]
fn source_and_runtime_versions_use_native_r_version_equivalence() {
    let root = workspace("answer()");
    root.write(
        "r/DESCRIPTION",
        root.read("r/DESCRIPTION")
            .replace("Version: 0.1.0", "Version: 1.0-1"),
    );
    success(extract(&root));
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let package = &snapshot.workspace().packages["rdex"];
    assert_eq!(package.version.as_deref(), Some("1.0-1"));
    assert!(package.items.values().all(|item| item.documentation.as_ref().unwrap().provenance.iter().any(|record| matches!(&record.activity, ProvenanceActivity::RdExamples { package_version, .. } if package_version == "1.0.1"))));
}

#[test]
fn configured_metadata_must_identify_the_loaded_checkout() {
    let root = workspace("answer()");
    let output = Command::new("R")
        .args([
            "--vanilla",
            "--slave",
            "-e",
            "cat(as.character(utils::packageVersion('stats')))",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let version = String::from_utf8(output.stdout).unwrap();
    let description = root
        .read("r/DESCRIPTION")
        .replace("Version: 0.1.0", &format!("Version: {version}"));
    root.write("r/DESCRIPTION", &description);
    root.write(
        "r/EXTRA_DESCRIPTION",
        description.replace("Package: rdex", "Package: stats"),
    );
    root.write(
        "diplodocus.toml",
        root.read("diplodocus.toml").replace(
            "metadata-path = \"DESCRIPTION\"",
            "metadata-path = \"EXTRA_DESCRIPTION\"",
        ),
    );
    root.write("r/R/api.R", "answer <- function() 42L\nother <- function() 24L\n.onLoad <- function(libname, pkgname) writeLines('loaded', 'marker')\n");
    let output = extract(&root);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("identity does not match the configured DESCRIPTION")
    );
    assert!(!root.path().join("r/marker").exists());
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
}
