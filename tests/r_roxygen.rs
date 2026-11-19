mod support;

use std::fs;
use std::process::{Command, Output};

use diplodocus::ir::{ExtractionMode, ProvenanceActivity};
use diplodocus::snapshots::Snapshot;

fn workspace(settings: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Roxygen workflow\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Roxygen workflow\n    slug: demo\n    ecosystem: r\n    repository: repo\n    path: package\n    metadata-path: DESCRIPTION\n{settings}    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n"));
    root.write("package/DESCRIPTION", "Package: roxyworkflow\nVersion: 0.1.0\nTitle: Roxygen Workflow Fixture\nDescription: Exercises automatic native generation.\nLicense: MIT\nAuthor: Fixture Author\nMaintainer: Fixture Author <fixture@example.com>\nEncoding: UTF-8\nRoxygen: list(markdown = TRUE)\n");
    root.write("package/R/api.R", api("Initial documentation.", "1L"));
    root
}

fn api(prose: &str, default: &str) -> String {
    format!(
        "#' Return a value\n#'\n#' {prose}\n#' @param value A value.\n#' @export\n#' @examples\n#' writeLines('ran', 'example-ran')\nidentity <- local({{ function(value = {default}) value }})\n\n#' Derived value\n#' @inheritParams identity\n#' @export\nderived <- function(value = {default}) value\n"
    )
}

fn command(root: &support::TestWorkspace, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args(args)
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

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
}

#[test]
fn assembly_and_commands_generate_and_discover_runtime_functions() {
    let root = workspace("");
    // Portable string ordering differs from filesystem component ordering here.
    root.write("package/inst/include/widget.h", "#pragma once\n");
    root.write("package/inst/include/widget/api.h", "#pragma once\n");
    let sources =
        diplodocus::assembly::assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let package = &sources.workspace().packages["demo"];
    assert!(package.items.values().any(|item| item.name == "identity"));
    assert_eq!(package.items.len(), 2);
    assert!(root.path().join("package/man/identity.Rd").is_file());
    assert!(root.read("package/NAMESPACE").contains("export(identity)"));
    assert!(!root.path().join("package/example-ran").exists());
    success(command(&root, &["check"]));
    assert!(!root.path().join("package/example-ran").exists());
    success(command(&root, &["extract"]));
    assert!(root.path().join("package/example-ran").exists());
    let before = snapshot(&root).canonical_export().unwrap();
    let loaded = snapshot(&root);
    let workspace = loaded.workspace();
    assert_eq!(workspace.packages["demo"].items.len(), 2);
    assert!(
        workspace
            .provenance
            .iter()
            .any(|record| matches!(record.activity, ProvenanceActivity::RDocumentation { .. }))
    );
    assert!(
        workspace
            .provenance
            .iter()
            .any(|record| matches!(&record.activity, ProvenanceActivity::RRuntime { inputs, .. }
                if ["package/inst/include/widget.h", "package/inst/include/widget/api.h"]
                    .iter().all(|path| inputs.iter().any(|input| input.source.path.as_str() == *path))))
    );
    assert!(workspace.provenance.iter().any(|record| matches!(
        record.activity,
        ProvenanceActivity::Extraction {
            mode: ExtractionMode::Native,
            ..
        }
    )));
    success(command(&root, &["build", "--output", "site"]));
    assert_eq!(before, snapshot(&root).canonical_export().unwrap());
    root.write("package/R/api.R", api("Changed documentation.", "2L"));
    success(command(&root, &["build", "--output", "site"]));
    assert!(
        root.read("package/man/identity.Rd")
            .contains("Changed documentation.")
    );
    assert!(root.read("package/man/identity.Rd").contains("value = 2L"));
    assert_ne!(before, snapshot(&root).canonical_export().unwrap());
    fs::remove_dir_all(root.path().join("package")).unwrap();
    success(command(
        &root,
        &[
            "generate",
            "--input",
            ".diplodocus/documentation.sqlite",
            "--output",
            "detached",
        ],
    ));
}

#[test]
fn malformed_roxygen_never_publishes_partial_or_stale_output() {
    let root = workspace("");
    success(command(&root, &["build", "--output", "site"]));
    let before = fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let site = support::files_under(&root.path().join("site"))
        .into_iter()
        .map(|path| {
            (
                path.clone(),
                fs::read(root.path().join("site").join(path)).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    root.write(
        "package/R/api.R",
        api("Broken documentation.", "2L").replace("@param value A value.", "@param"),
    );
    let output = command(&root, &["build", "--output", "site"]);
    assert!(!output.status.success());
    assert_eq!(
        before,
        fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
    );
    for (path, bytes) in site {
        assert_eq!(
            bytes,
            fs::read(root.path().join("site").join(path)).unwrap()
        );
    }
    root.write("package/R/api.R", api("Recovered documentation.", "3L"));
    success(command(&root, &["build", "--output", "site"]));
    assert!(
        root.read("package/man/identity.Rd")
            .contains("Recovered documentation.")
    );
}

#[test]
fn maintained_rd_override_does_not_invoke_roxygen() {
    let root = workspace("    r:\n      documentation: rd\n");
    root.write("package/NAMESPACE", "export(identity)\n");
    root.write(
        "package/R/api.R",
        "#' @nonexistentTag\nidentity <- function(value = 1L) value\n",
    );
    root.write("package/man/identity.Rd", "\\name{identity}\n\\alias{identity}\n\\title{Maintained documentation}\n\\usage{identity(value = 1L)}\n\\description{Written by hand.}\n");
    let description = root.read("package/DESCRIPTION");
    let rd = root.read("package/man/identity.Rd");
    success(command(&root, &["extract"]));
    assert_eq!(description, root.read("package/DESCRIPTION"));
    assert_eq!(rd, root.read("package/man/identity.Rd"));
    assert!(
        !snapshot(&root)
            .workspace()
            .provenance
            .iter()
            .any(|record| matches!(record.activity, ProvenanceActivity::RDocumentation { .. }))
    );
}

#[test]
fn explicit_generation_and_source_loader_work_without_roxygen_metadata() {
    let root = workspace("    r:\n      documentation: roxygen2\n");
    root.write(
        "package/DESCRIPTION",
        root.read("package/DESCRIPTION")
            .replace("Roxygen: list(markdown = TRUE)\n", ""),
    );
    root.write(
        "package/man/roxygen/meta.R",
        "list(load = 'source', markdown = TRUE)\n",
    );
    success(command(&root, &["extract"]));
    assert!(snapshot(&root).workspace().provenance.iter().any(|record| matches!(&record.activity, ProvenanceActivity::RDocumentation { loader, .. } if loader == "source")));
}

#[test]
fn installed_loader_and_overlapping_custom_outputs_are_diagnosed() {
    let root = workspace("");
    root.write("package/man/roxygen/meta.R", "list(load = 'installed')\n");
    let output = command(&root, &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("loading strategies"));
    let root = workspace("    r:\n      generated-outputs: [R]\n");
    let output = command(&root, &["check"]);
    assert!(!output.status.success());
    assert!(!root.path().join("package/NAMESPACE").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_serve_regenerates_once_and_recovers_after_generation_failure() {
    use diplodocus::commands::{CommandRunner, ServeOptions};
    use diplodocus::execution::ExecutionDeadlines;
    use diplodocus::reporting::{Event, Reporter};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Observer(tokio::sync::mpsc::UnboundedSender<bool>);
    impl Reporter for Observer {
        fn report(&self, event: Event<'_>) {
            if let Event::Error(message) = &event {
                eprintln!("serve failure: {message}");
            }
            if let Event::Diagnostic(diagnostic) = &event {
                eprintln!("serve diagnostic: {diagnostic:?}");
            }
            if let Event::Watching { retained_previous } = event {
                let _ = self.0.send(retained_previous);
            }
        }
    }
    async fn watching(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<bool>, retained: bool) {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(45), receiver.recv())
                .await
                .unwrap(),
            Some(retained)
        );
    }
    let root = workspace("");
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let options = ServeOptions {
        config: root.path().join("diplodocus.yaml"),
        output: root.path().join("site"),
        host: std::net::Ipv4Addr::LOCALHOST.into(),
        port,
        live_reload: false,
        open: false,
    };
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let runner = CommandRunner::with_reporter(Arc::new(Observer(sender)));
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        runner
            .serve_with(
                options,
                ExecutionDeadlines::default(),
                Box::pin(async {
                    let _ = stopped.await;
                }),
            )
            .await
    });
    watching(&mut receiver, false).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(600), receiver.recv())
            .await
            .is_err(),
        "generation writes caused an idle rebuild"
    );
    root.write("package/R/api.R", api("Updated through plain serve.", "2L"));
    watching(&mut receiver, false).await;
    assert!(
        root.read("package/man/identity.Rd")
            .contains("Updated through plain serve.")
    );
    let html = support::files_under(&root.path().join("site"))
        .into_iter()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "html")
                && fs::read_to_string(root.path().join("site").join(path))
                    .unwrap()
                    .contains("Updated through plain serve.")
        })
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stream
        .write_all(
            format!(
                "GET /{} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                html.to_string_lossy()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.contains("Updated through plain serve."));
    assert!(
        tokio::time::timeout(Duration::from_millis(600), receiver.recv())
            .await
            .is_err()
    );
    let previous = fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    root.write(
        "package/R/api.R",
        api("Bad documentation.", "3L").replace("@param value A value.", "@param"),
    );
    watching(&mut receiver, true).await;
    assert_eq!(
        previous,
        fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(600), receiver.recv())
            .await
            .is_err(),
        "partial generation caused a retry loop"
    );
    root.write(
        "package/R/api.R",
        api("Recovered through plain serve.", "3L"),
    );
    watching(&mut receiver, false).await;
    assert!(
        root.read("package/man/identity.Rd")
            .contains("Recovered through plain serve.")
    );
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_timeout_and_cancellation_preserve_the_previous_snapshot() {
    use diplodocus::commands::{CommandRunner, ExtractOptions};
    use diplodocus::execution::ExecutionDeadlines;
    use std::time::Duration;

    for cancel in [false, true] {
        let root = workspace("");
        success(command(&root, &["extract"]));
        let database = root.path().join(".diplodocus/documentation.sqlite");
        let previous = fs::read(&database).unwrap();
        root.write("package/man/roxygen/meta.R", "writeLines(as.character(Sys.getpid()), 'generation-pid')\nSys.sleep(120)\nlist(markdown = TRUE)\n");
        let marker = root.path().join("package/generation-pid");
        let cancellation_marker = marker.clone();
        let cancellation = Box::pin(async move {
            if cancel {
                loop {
                    if cancellation_marker.exists() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            } else {
                std::future::pending::<()>().await;
            }
        });
        let result = CommandRunner::default()
            .extract_with(
                ExtractOptions {
                    config: root.path().join("diplodocus.yaml"),
                    output: None,
                },
                ExecutionDeadlines {
                    cell: if cancel { 30_000 } else { 2_000 },
                    ..ExecutionDeadlines::default()
                },
                cancellation,
            )
            .await;
        assert!(result.is_err());
        if cancel {
            assert!(matches!(
                result,
                Err(diplodocus::commands::CommandError::Assembly(
                    diplodocus::assembly::AssemblyError::Cancelled
                ))
            ));
        }
        assert_eq!(previous, fs::read(database).unwrap());
        if cfg!(target_os = "linux")
            && let Ok(pid) = fs::read_to_string(marker)
        {
            assert!(
                !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists(),
                "generation process survived cleanup"
            );
        }
    }
}

#[test]
fn native_cleanup_failure_is_an_error_even_without_examples() {
    let root = workspace("    r:\n      documentation: rd\n");
    root.write("package/NAMESPACE", "export(answer)\n");
    root.write(
        "package/R/api.R",
        "answer <- function() 42L\n.onUnload <- function(libpath) stop('native cleanup failed')\n",
    );
    root.write("package/man/answer.Rd", "\\name{answer}\\alias{answer}\\title{Answer}\\usage{answer()}\\description{No executable examples.}\n");
    let output = command(&root, &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("native cleanup failed"));
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn custom_outputs_cannot_alias_configuration_or_enter_nested_packages() {
    let root = workspace("    r:\n      generated-outputs: [config-output]\n");
    let config = root
        .read("diplodocus.yaml")
        .replace("    path: .\npackages:", "    path: ..\npackages:");
    root.write("package/config.yaml", config);
    root.write(
        "package/R/api.R",
        "answer <- function() 42L\n.onLoad <- function(...) writeLines('loaded', 'load-marker')\n",
    );
    std::os::unix::fs::symlink("config.yaml", root.path().join("package/config-output")).unwrap();
    let output = command(&root, &["check", "--config", "package/config.yaml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("workspace configuration"));
    assert!(!root.path().join("package/load-marker").exists());

    let root = workspace("    r:\n      generated-outputs: [child/generated]\n");
    root.write("diplodocus.yaml", format!("{}\n  - id: nested\n    name: Nested package\n    slug: nested\n    ecosystem: r\n    repository: repo\n    path: package/child\n    metadata-path: DESCRIPTION\n    targets: []\n", root.read("diplodocus.yaml")));
    root.write(
        "package/child/DESCRIPTION",
        "Package: nestedfixture\nVersion: 0.1.0\nTitle: Nested Fixture\n",
    );
    root.write(
        "package/R/api.R",
        "answer <- function() 42L\n.onLoad <- function(...) writeLines('loaded', 'load-marker')\n",
    );
    let output = command(&root, &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("another package"));
    assert!(!root.path().join("package/load-marker").exists());
}

#[test]
fn documented_datasets_do_not_require_a_runtime_binding() {
    let root = workspace("    r:\n      documentation: rd\n");
    root.write("package/NAMESPACE", "export(dataset)\n");
    root.write("package/R/api.R", "");
    root.write("package/man/dataset.Rd", "\\name{dataset}\\alias{dataset}\\docType{data}\\title{A documented dataset}\\usage{dataset}\\description{Available independently of direct R definitions.}\\format{A numeric vector.}\n");
    success(command(&root, &["extract"]));
    assert!(snapshot(&root).workspace().packages["demo"].items.values().any(|item| matches!(&item.language_data, Some(diplodocus::ir::ItemLanguageData::R(data)) if matches!(data.declaration, diplodocus::ir::RDeclaration::Dataset))));
}
