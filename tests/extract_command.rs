mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::format_diagnostic;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

const CONFIG: &str = "workspace/diplodocus.toml";
const DEFAULT_OUTPUT: &str = "workspace/.diplodocus/documentation.sqlite";

fn extract(root: &Path, output: Option<&Path>, without_runtimes: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    command
        .current_dir(root)
        .args(["extract", "--config", CONFIG]);
    if let Some(output) = output {
        command.arg("--output").arg(output);
    }
    if without_runtimes {
        command.env("PATH", "");
    }
    command.output().unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

fn failure(output: &Output, diagnostic: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(diagnostic), "{stderr}");
}

fn static_workspace() -> support::TestWorkspace {
    let root = support::acceptance_workspace();
    let mut config = support::fixture_configuration(&root, CONFIG);
    for collection in config["content"].as_array_mut().unwrap() {
        collection.as_table_mut().unwrap().insert(
            "execution".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "mode".into(),
                toml::Value::String("never".into()),
            )])),
        );
    }
    root.write(CONFIG, toml::to_string(&config).unwrap());
    root
}

#[test]
fn cli_paths_and_static_snapshot_match_the_contract_without_runtimes() {
    let root = static_workspace();
    let sources = assemble_workspace(root.path().join(CONFIG)).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let expected = Snapshot::from_sources(&sources, &resolved).unwrap();
    assert_eq!(expected.workspace().packages.len(), 2);
    assert_eq!(expected.workspace().concepts.len(), 2);
    assert!(!expected.documents().is_empty());
    assert!(!expected.assets().is_empty());
    let warnings: String = expected
        .workspace()
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{}\n", format_diagnostic(diagnostic)))
        .collect();
    assert!(!warnings.is_empty());
    let expected = expected.canonical_export().unwrap();
    drop((sources, resolved));

    let result = extract(root.path(), None, true);
    success(&result);
    assert_eq!(String::from_utf8(result.stderr).unwrap(), warnings);
    let default = root.path().join(DEFAULT_OUTPUT);
    assert_eq!(
        Snapshot::load(&default)
            .unwrap()
            .canonical_export()
            .unwrap(),
        expected
    );
    assert!(!root.path().join(".diplodocus").exists());
    fs::remove_file(&default).unwrap();

    let portable = tempfile::tempdir().unwrap();
    let absolute = portable.path().join("absolute.sqlite");
    for output in [Path::new("exports/relative.sqlite"), absolute.as_path()] {
        success(&extract(root.path(), Some(output), true));
        assert_eq!(
            Snapshot::load(root.path().join(output))
                .unwrap()
                .canonical_export()
                .unwrap(),
            expected
        );
        assert!(!default.exists());
    }
    assert!(!root.path().join("workspace/exports").exists());
    assert!(!root.path().join("site").exists());
    assert!(!root.path().join("workspace/.diplodocus/cache").exists());
    drop(root);
    assert_eq!(
        Snapshot::load(&absolute)
            .unwrap()
            .canonical_export()
            .unwrap(),
        expected
    );
    assert_eq!(fs::read_dir(portable.path()).unwrap().count(), 1);
}

#[test]
fn generated_snapshots_never_become_sources_or_change_input_fingerprints() {
    let root = static_workspace();
    let config = root.path().join(CONFIG);
    let original = assemble_workspace(&config).unwrap();
    let expected_inputs: Vec<_> = original.input_paths().collect();
    let expected = Snapshot::from_sources(&original, &resolve_workspace(&original).unwrap())
        .unwrap()
        .canonical_export()
        .unwrap();

    // Output names are unrestricted, including extensions understood by extractors.
    for output in [
        DEFAULT_OUTPUT,
        "core/docs/export.md",
        "python/docs/export.qmd",
        "python/python/foo/export.py",
        "python/python/foo/export.pyi",
        "r/R/export.R",
        "r/man/export.Rd",
    ] {
        for _ in 0..2 {
            success(&extract(root.path(), Some(Path::new(output)), true));
            assert_eq!(
                Snapshot::load(root.path().join(output))
                    .unwrap()
                    .canonical_export()
                    .unwrap(),
                expected,
                "{output}"
            );
            let current = assemble_workspace(&config).unwrap();
            assert_eq!(current.input_paths().collect::<Vec<_>>(), expected_inputs);
            assert_eq!(current.workspace(), original.workspace());
            original.revalidate().unwrap();
        }
    }
    let result = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args(["check", "--config", CONFIG])
        .env("PATH", "")
        .output()
        .unwrap();
    success(&result);
}

#[test]
fn temporary_storage_does_not_enter_discovery_or_revalidation() {
    let root = static_workspace();
    let config = root.path().join(CONFIG);
    let original = assemble_workspace(&config).unwrap();
    let expected_inputs: Vec<_> = original.input_paths().collect();
    for directory in [
        "core/docs",
        "python/docs",
        "python/python/foo",
        "r/R",
        "r/man",
    ] {
        for extension in ["md", "qmd", "py", "pyi", "R", "Rd"] {
            for name in [
                format!(".diplodocus-snapshot-leftover.{extension}"),
                format!(".diplodocus-snapshot-leftover.sqlite-wal/page.{extension}"),
                format!(".diplodocus/nested/page.{extension}"),
            ] {
                root.write(format!("{directory}/{name}"), b"\xff\0incomplete storage");
            }
        }
    }
    original.revalidate().unwrap();
    let current = assemble_workspace(&config).unwrap();
    assert_eq!(current.input_paths().collect::<Vec<_>>(), expected_inputs);
    assert_eq!(current.workspace(), original.workspace());
    success(&extract(root.path(), None, true));
}

#[test]
fn older_snapshots_are_excluded_without_touching_their_recovery_files() {
    let root = static_workspace();
    let config = root.path().join(CONFIG);
    let original = assemble_workspace(&config).unwrap();
    // URI punctuation must remain part of the filename during read-only inspection.
    let output = Path::new("core/docs/older ?#%.md");
    success(&extract(root.path(), Some(output), true));
    let path = root.path().join(output);
    assert_eq!(&fs::read(&path).unwrap()[68..72], b"DIPL");
    let database = rusqlite::Connection::open(&path).unwrap();
    database.pragma_update(None, "application_id", 0).unwrap();
    database
        .execute("UPDATE manifest SET storage_version = 1", [])
        .unwrap();
    database.close().unwrap();
    for suffix in ["-journal", "-wal", "-shm"] {
        root.write(format!("{}{suffix}", output.display()), "recovery sentinel");
    }
    let files = support::files_under(root.path());
    let bytes: Vec<_> = files
        .iter()
        .map(|file| fs::read(root.path().join(file)).unwrap())
        .collect();
    let current = assemble_workspace(&config).unwrap();
    assert_eq!(current.workspace(), original.workspace());
    original.revalidate().unwrap();
    assert_eq!(support::files_under(root.path()), files);
    for (file, expected) in files.iter().zip(bytes) {
        assert_eq!(fs::read(root.path().join(file)).unwrap(), expected);
    }
}

#[cfg(unix)]
#[test]
fn source_aliases_to_snapshots_and_storage_are_excluded_within_the_boundary() {
    use std::os::unix::fs::symlink;

    let root = static_workspace();
    let config = root.path().join(CONFIG);
    let original = assemble_workspace(&config).unwrap();
    for (output, alias) in [
        ("core/export.sqlite", "core/docs/alias.md"),
        ("python/export.sqlite", "python/python/foo/alias.py"),
        ("r/export.sqlite", "r/R/alias.R"),
    ] {
        success(&extract(root.path(), Some(Path::new(output)), true));
        symlink(root.path().join(output), root.path().join(alias)).unwrap();
    }
    root.write("core/.diplodocus/staging", b"\xffpartial storage");
    symlink(
        root.path().join("core/.diplodocus/staging"),
        root.path().join("core/docs/staging.md"),
    )
    .unwrap();
    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = format!("core/export.sqlite{suffix}");
        root.write(&sidecar, b"\xffrecovery data");
        symlink(
            root.path().join(sidecar),
            root.path().join(format!("core/docs/sidecar{suffix}.md")),
        )
        .unwrap();
    }
    assert_eq!(
        assemble_workspace(&config).unwrap().workspace(),
        original.workspace()
    );
    original.revalidate().unwrap();

    symlink(
        root.path().join("python/export.sqlite"),
        root.path().join("core/docs/escape.md"),
    )
    .unwrap();
    assert!(assemble_workspace(&config).is_err());
}

#[test]
fn explicit_database_downloads_remain_inputs_and_cannot_be_overwritten() {
    let root = static_workspace();
    let config = root.path().join(CONFIG);
    let output = Path::new("core/docs/download.sqlite");
    success(&extract(root.path(), Some(output), true));
    let bytes = fs::read(root.path().join(output)).unwrap();
    let document = root.read("core/docs/index.md");
    root.write(
        "core/docs/index.md",
        format!("{document}\n[Download snapshot](download.sqlite)\n"),
    );
    let sources = assemble_workspace(&config).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert!(resolved.assets().values().any(|asset| asset.bytes == bytes));
    failure(
        &extract(root.path(), Some(output), true),
        "output overlaps a declared input",
    );
    assert_eq!(fs::read(root.path().join(output)).unwrap(), bytes);
}

#[test]
fn cli_rejects_every_kind_of_declared_input_before_execution() {
    let root = support::acceptance_workspace();
    let config = root.read(CONFIG).replace(
        "declared-environment-inputs = [\"pyproject.toml\"]",
        "declared-environment-inputs = [\"environment.lock\"]",
    );
    root.write(CONFIG, config);
    root.write("python/environment.lock", "environment sentinel");
    root.write("python/execution/must-not-run.qmd", "```{python}\nfrom pathlib import Path\nPath('execution-marker').write_text('executed')\n```\n");
    let before: Vec<_> = support::files_under(root.path())
        .into_iter()
        .map(|path| {
            let bytes = fs::read(root.path().join(&path)).unwrap();
            (path, bytes)
        })
        .collect();
    for output in [
        CONFIG,
        "python/pyproject.toml",
        "python/python/foo/__init__.py",
        "r/DESCRIPTION",
        "r/NAMESPACE",
        "r/man/fit.Rd",
        "core/docs/index.md",
        "core/docs/assets/workspace.svg",
        "python/environment.lock",
        "core/docs",
        "python/python/foo",
        "workspace",
        ".",
    ] {
        failure(
            &extract(root.path(), Some(Path::new(output)), true),
            "output overlaps a declared input",
        );
    }
    for (path, bytes) in &before {
        assert_eq!(fs::read(root.path().join(path)).unwrap(), *bytes);
    }
    assert_eq!(
        support::files_under(root.path()),
        before.into_iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
    assert!(!root.path().join("workspace/.diplodocus").exists());
}

#[cfg(unix)]
#[test]
fn cli_rejects_input_aliases_and_a_default_destination_that_is_an_input() {
    use std::os::unix::fs::symlink;

    let root = static_workspace();
    let asset = root.path().join("core/docs/assets/workspace.svg");
    let bytes = fs::read(&asset).unwrap();
    symlink(&asset, root.path().join("asset-alias.sqlite")).unwrap();
    symlink(
        root.path().join("core/docs"),
        root.path().join("docs-alias"),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("workspace/.diplodocus")).unwrap();
    symlink(&asset, root.path().join(DEFAULT_OUTPUT)).unwrap();
    for output in [
        Some(Path::new("asset-alias.sqlite")),
        Some(Path::new("docs-alias/assets/workspace.svg")),
        Some(Path::new("core/docs/assets/../index.md")),
        None,
    ] {
        failure(
            &extract(root.path(), output, true),
            "output overlaps a declared input",
        );
        assert_eq!(fs::read(&asset).unwrap(), bytes);
    }
    assert!(
        fs::symlink_metadata(root.path().join(DEFAULT_OUTPUT))
            .unwrap()
            .is_symlink()
    );
}

#[test]
fn cli_validation_and_publication_failures_preserve_the_previous_snapshot() {
    let root = static_workspace();
    success(&extract(root.path(), None, true));
    let path = root.path().join(DEFAULT_OUTPUT);
    let before = fs::read(&path).unwrap();
    let page = root.read("core/docs/index.md");
    root.write("core/docs/index.md", "[`missing-reference`]\n");
    failure(
        &extract(root.path(), None, true),
        "unresolved-item-reference",
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    root.write("core/docs/index.md", page);
    let config = root.read(CONFIG);
    root.write(CONFIG, "not valid TOML");
    failure(&extract(root.path(), None, true), "could not parse");
    assert_eq!(fs::read(&path).unwrap(), before);
    root.write(CONFIG, config);
    root.write("core/docs/index.md", "# Refreshed\n");
    root.write(format!("{DEFAULT_OUTPUT}-journal"), "recovery sentinel");
    failure(&extract(root.path(), None, true), "SQLite sidecar exists");
    assert_eq!(fs::read(&path).unwrap(), before);
    root.remove(format!("{DEFAULT_OUTPUT}-journal"));
    success(&extract(root.path(), None, true));
    assert!(
        Snapshot::load(&path)
            .unwrap()
            .workspace()
            .pages
            .values()
            .any(|page| page.title == "Refreshed")
    );
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn cli_executes_python_and_r_and_publishes_portable_outputs_and_assets() {
    use diplodocus::ir::{ExecutionOrigin, ProvenanceActivity};

    let root = support::acceptance_workspace();
    let portable = tempfile::tempdir().unwrap();
    let path = portable.path().join("documentation.sqlite");
    success(&extract(root.path(), Some(&path), false));
    assert!(!root.path().join(DEFAULT_OUTPUT).exists());
    assert!(!root.path().join("site").exists());
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let mut executed = 0;
    let mut generated_assets = 0;
    for id in loaded.workspace().pages.keys() {
        if let Some(page) = loaded.executed_page(id) {
            executed += 1;
            let record = page.record();
            assert!(matches!(
                record.page.collection.as_str(),
                "python-execution" | "r-execution"
            ));
            assert!(matches!(
                record.provenance.as_ref().unwrap().execution.activity,
                ProvenanceActivity::Execution {
                    origin: ExecutionOrigin::Executed,
                    ..
                }
            ));
            for asset in &record.assets {
                generated_assets += 1;
                let stored = &loaded.assets()[&asset.reference.fingerprint.value];
                assert_eq!(stored.bytes.len() as u64, asset.byte_size);
                assert_eq!(stored.media_type, asset.media_type);
            }
        }
    }
    assert_eq!(executed, 4);
    assert_eq!(generated_assets, 2);
    let content = loaded.canonical_export().unwrap();
    assert!(content.contains("Python total: 12"));
    assert!(content.contains("R total: 12"));
    assert!(loaded.assets().len() > generated_assets);
    assert_eq!(fs::read_dir(portable.path()).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
fn executable_workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(CONFIG, "[project]\nname='Extraction'\n[[repository]]\nid='docs'\npath='../repo'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n");
    root.write(
        "repo/docs/index.qmd",
        "# Before execution {#before-execution}\n",
    );
    root
}

#[cfg(target_os = "linux")]
#[test]
fn cli_execution_and_generated_reference_failures_preserve_the_previous_snapshot() {
    let root = executable_workspace();
    success(&extract(root.path(), None, true));
    let path = root.path().join(DEFAULT_OUTPUT);
    let before = fs::read(&path).unwrap();
    let config = root.read(CONFIG);
    let figure = "from IPython.display import SVG, display\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\"))";
    for (kernel, code, diagnostic) in [
        (
            "diplodocus-missing-extract-kernel",
            "print('unreachable')",
            "execution-startup-failed",
        ),
        (
            "python3",
            "raise RuntimeError('failed extraction')",
            "execution-cell-failed",
        ),
        (
            "python3",
            "from IPython.display import Markdown, display\ndisplay(Markdown('[`missing-generated-reference`]'))",
            "unresolved-item-reference",
        ),
    ] {
        root.write(CONFIG, config.replace("python3", kernel));
        root.write(
            "repo/docs/index.qmd",
            format!("# Failure\n\n```{{python}}\n{figure}\n```\n\n```{{python}}\n{code}\n```\n"),
        );
        failure(&extract(root.path(), None, false), diagnostic);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(Snapshot::load(&path).unwrap().assets().is_empty());
        assert!(
            !support::files_under(path.parent().unwrap())
                .iter()
                .any(|entry| {
                    entry
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".diplodocus-snapshot-")
                })
        );
    }
    root.write(CONFIG, config);
    root.write(
        "repo/docs/index.qmd",
        format!("# Recovered\n\n```{{python}}\n{figure}\n```\n"),
    );
    success(&extract(root.path(), None, false));
    assert_eq!(Snapshot::load(&path).unwrap().assets().len(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn cli_protects_assets_referenced_only_by_generated_markdown() {
    use diplodocus::validation::ReferenceTarget;

    let root = executable_workspace();
    root.write("repo/docs/download.bin", "keep this download");
    root.write("repo/docs/nested/generated.qmd", "# Generated references\n\n```{python}\nfrom IPython.display import Markdown, display\ndisplay(Markdown('[Download](../download.bin) [Page](../index.qmd#before-execution)'))\n```\n");
    failure(
        &extract(
            root.path(),
            Some(Path::new("repo/docs/download.bin")),
            false,
        ),
        "output overlaps a declared input",
    );
    assert_eq!(root.read("repo/docs/download.bin"), "keep this download");
    assert!(!root.path().join(DEFAULT_OUTPUT).exists());
    success(&extract(root.path(), None, false));
    let portable = tempfile::tempdir().unwrap();
    let path = portable.path().join("documentation.sqlite");
    fs::copy(root.path().join(DEFAULT_OUTPUT), &path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.assets().len(), 1);
    assert_eq!(
        loaded.assets().values().next().unwrap().bytes,
        b"keep this download"
    );
    assert!(loaded.documents().iter().flat_map(|document| &document.references).any(|reference| {
        matches!(&reference.target, ReferenceTarget::Page { fragment: Some(fragment), .. } if fragment == "before-execution")
    }));
}
