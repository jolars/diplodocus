mod support;

use std::process::Command;
use std::sync::{Arc, Mutex};

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{
    BuildOptions, CheckOptions, CommandRunner, ExtractOptions, ServeOptions,
};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::reporting::{Event, Phase, Reporter};
use diplodocus::snapshots::Snapshot;

#[test]
fn generated_python_api_is_discovered_from_the_prepared_checkout() {
    let root = support::TestWorkspace::from_fixture("generated-python");
    let output = Command::new("python")
        .current_dir(root.path())
        .args(["-c", "import generated_demo as p; import inspect; assert p.generated(3) == 3; assert p.generated.__doc__ == 'Return the generated value.'; assert str(inspect.signature(p.generated)) == '(value: int = 1) -> int'"])
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let item = sources.workspace().packages["demo"]
        .items
        .values()
        .find(|item| item.qualified_name == "generated_demo.generated")
        .unwrap();
    assert_eq!(item.signatures.len(), 1);
    assert_eq!(
        item.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("Return the generated value.")
    );
    assert!(item.source_location.as_ref().unwrap().span.is_none());
}

#[test]
fn generated_julia_method_and_attached_docs_remain_an_explicit_migration_gap() {
    let root = support::TestWorkspace::from_fixture("generated-julia");
    let output = Command::new("julia")
        .current_dir(root.path())
        .args(["--startup-file=no", "--history-file=no", "--project=.", "-e",
            "using GeneratedDemo; @assert generated(3) == 3; @assert occursin(\"Return the generated value.\", string(@doc generated))"])
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
    assert!(
        matches!(error, diplodocus::assembly::AssemblyError::Diagnostics(ref diagnostics)
        if diagnostics.iter().any(|d| d.code.as_str() == "julia-unresolved-definition")),
        "{error:?}"
    );
}

#[test]
fn commands_share_discovery_and_reporting_does_not_change_portable_evidence() {
    let root = support::TestWorkspace::from_fixture("generated-python");
    root.write("generated_demo/__init__.py", "\"\"\"A static package.\"\"\"\nfrom pathlib import Path as _Path\n_Path('loaded').write_text('yes')\n__all__ = [\"identity\"]\ndef identity(value: int = 1) -> int:\n    \"\"\"Return a value.\n\n    >>> _ = _Path('example-ran').write_text('yes')\n    >>> identity(2)\n    2\n    \"\"\"\n    return value\n");
    let config = root.path().join("diplodocus.yaml");
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let observation = Arc::new(Observation::default());
    let runner = CommandRunner::with_reporter(observation.clone());
    runner
        .check(CheckOptions {
            config: config.clone(),
        })
        .unwrap();
    assert!(root.path().join("loaded").exists());
    assert!(!root.path().join("example-ran").exists());
    runner
        .extract(ExtractOptions {
            config: config.clone(),
            output: None,
        })
        .unwrap();
    assert!(root.path().join("loaded").exists());
    assert!(root.path().join("example-ran").exists());
    let before = Snapshot::load(&database)
        .unwrap()
        .canonical_export()
        .unwrap();
    runner
        .build(BuildOptions {
            config: config.clone(),
            output: root.path().join("site"),
        })
        .unwrap();
    assert_eq!(
        before,
        Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap()
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    *observation.ready.lock().unwrap() = Some(stop);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(runner.serve_with(
            ServeOptions {
                config,
                output: root.path().join("site"),
                host: std::net::Ipv4Addr::LOCALHOST.into(),
                port: 0,
                live_reload: false,
                open: false,
            },
            ExecutionDeadlines::default(),
            Box::pin(async {
                let _ = stopped.await;
            }),
        ))
        .unwrap();
    assert_eq!(
        before,
        Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap()
    );
    let timings = observation.native.lock().unwrap();
    assert!(timings.contains(&Phase::PackageLoading));
    assert!(timings.contains(&Phase::Examples));
    assert!(!before.contains("package-loading"));
    assert!(!before.contains("elapsed"));
}

#[derive(Default)]
struct Observation {
    native: Mutex<Vec<Phase>>,
    ready: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Reporter for Observation {
    fn report(&self, event: Event<'_>) {
        match event {
            Event::NativeTiming { phase, .. } => self.native.lock().unwrap().push(phase),
            Event::Serving { .. } => {
                if let Some(stop) = self.ready.lock().unwrap().take() {
                    let _ = stop.send(());
                }
            }
            _ => {}
        }
    }
}
