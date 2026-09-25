mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::ProvenanceActivity;
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;
use serde_json::{Value, json};

fn fixture(presentation: &str) -> support::TestWorkspace {
    let root = support::acceptance_workspace();
    let config = root.read("workspace/diplodocus.toml");
    let config = config
        .replace("slug = \"python\"", "slug = \"python-api\"")
        .replace("mount = \"guide\"", "mount = \"manual\"")
        .replace(
            "url = \"https://github.com/example/foo-python\"",
            "url = \"https://forge.example/foo-python\"\nrevision = \"release-2.1\"\nsource_link_template = \"https://forge.example/foo-python/blob/{revision}/{path}#L{line}\"",
        );
    root.write(
        "workspace/diplodocus.toml",
        format!("{config}\n{presentation}"),
    );
    root.write(
        "core/docs/diagnostic.md",
        "# Diagnostic\n\n<component name=\"unsupported\" />\n",
    );
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    Snapshot::from_sources(&sources, &resolved).unwrap()
}

fn presentation(snapshot: &Snapshot) -> Value {
    let export: Value = serde_json::from_str(&snapshot.canonical_export().unwrap()).unwrap();
    assert_eq!(export["producer"], env!("CARGO_PKG_VERSION"));
    assert_eq!(snapshot.producer(), env!("CARGO_PKG_VERSION"));
    let records: Vec<_> = export["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["kind"] == "presentation")
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["owner"], "");
    assert_eq!(records[0]["id"], "");
    assert_eq!(records[0]["content"], json!(snapshot.presentation()));
    records[0]["content"].clone()
}

#[test]
fn presentation_defaults_are_explicit_even_when_omitted_in_configuration() {
    let root = fixture("");
    let snapshot = snapshot(&root);
    assert_eq!(
        presentation(&snapshot),
        json!({"title": null, "description": null})
    );
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(html.contains("<title>Foo · Foo</title>"));
    assert!(!html.contains("<meta name=\"description\""));
}

#[test]
fn metadata_survives_relocation_and_loading_without_source_checkouts() {
    let settings =
        "[presentation]\ntitle = 'Foo & friends'\ndescription = 'A \"portable\" <guide>'\n";
    let root = fixture(settings);
    let snapshot = snapshot(&root);
    let expected = snapshot.workspace().clone();
    let export = snapshot.canonical_export().unwrap();
    let relocated = fixture(settings);
    assert_eq!(
        export,
        self::snapshot(&relocated).canonical_export().unwrap()
    );
    for checkout in [root.path(), relocated.path()] {
        assert!(!export.contains(checkout.to_str().unwrap()));
    }
    let target = tempfile::tempdir().unwrap();
    let path = target.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(snapshot);
    drop(root);
    drop(relocated);

    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.workspace(), &expected);
    assert_eq!(loaded.canonical_export().unwrap(), export);
    assert_eq!(
        presentation(&loaded),
        json!({
            "title": "Foo & friends", "description": "A \"portable\" <guide>"
        })
    );
    let workspace = loaded.workspace();
    assert_eq!(workspace.packages["pyfoo"].slug, "python-api");
    assert_eq!(
        workspace.content_collections["python-guide"].mount,
        "manual"
    );
    let repository = &workspace.repositories["python"];
    assert_eq!(
        repository.canonical_url.as_deref(),
        Some("https://forge.example/foo-python")
    );
    assert_eq!(repository.revision.as_deref(), Some("release-2.1"));
    assert_eq!(
        repository.source_link_template.as_deref(),
        Some("https://forge.example/foo-python/blob/{revision}/{path}#L{line}")
    );
    assert_eq!(repository.dirty, None);
    assert!(repository.declared_input_fingerprint.is_some());
    assert!(!workspace.diagnostics.is_empty());
    assert!(
        workspace
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source.is_some() && diagnostic.span.is_some())
    );
    assert!(workspace.provenance.iter().any(|provenance| {
        matches!(&provenance.activity, ProvenanceActivity::Extraction { parsers, inputs, .. }
            if !parsers.is_empty() && !inputs.is_empty())
            && provenance.tools.get("diplodocus").map(String::as_str)
                == Some(env!("CARGO_PKG_VERSION"))
    }));
    let item = &workspace.packages["pyfoo"].items["sid1:python:function:foo.model.fit"];
    let source = item.source_location.as_ref().unwrap();
    assert_eq!(source.repository, "python");
    assert_eq!(source.path.as_str(), "python/foo/model.py");
    assert!(source.span.is_some());

    let rendered = render_site(&Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(html.contains("<title>Foo &amp; friends · Foo &amp; friends</title>"));
    assert!(
        html.contains(
            "<meta name=\"description\" content=\"A &quot;portable&quot; &lt;guide&gt;\">"
        )
    );
    assert!(
        rendered
            .files()
            .contains_key("packages/python-api/manual/guide.html")
    );
}

#[test]
fn observed_repository_revision_and_dirty_state_survive_storage() {
    let root = fixture("");
    let repository = root.path().join("python");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args([
                "-c",
                "user.name=Snapshot test",
                "-c",
                "user.email=snapshot@example.test",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--quiet"]);
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "Fixture"]);
    let revision = git(&["rev-parse", "HEAD"]);
    let config = root
        .read("workspace/diplodocus.toml")
        .replace("revision = \"release-2.1\"\n", "");
    root.write("workspace/diplodocus.toml", config);
    let target = tempfile::tempdir().unwrap();
    for dirty in [false, true] {
        if dirty {
            root.write("python/untracked.txt", "dirty working tree");
        }
        let snapshot = snapshot(&root);
        let path = target.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        let loaded = Snapshot::load(&path).unwrap();
        let evidence = &loaded.workspace().repositories["python"];
        assert_eq!(evidence.revision.as_deref(), Some(revision.as_str()));
        assert_eq!(evidence.dirty, Some(dirty));
    }
}
