mod support;

use diplodocus::commands::{self, BuildOptions, ExtractOptions, GenerateOptions};
use diplodocus::snapshots::Snapshot;

fn static_workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Commands'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='qmd'\n");
    root.write(
        "docs/index.qmd",
        "# Welcome\n\n```{python}\n#| echo: false\nraise RuntimeError('must never execute')\n```\n",
    );
    root
}

#[test]
fn build_matches_separate_stages_and_portable_generation() {
    let root = static_workspace();
    let config = root.path().join("diplodocus.toml");
    let output = tempfile::tempdir().unwrap();
    let built = output.path().join("built");
    commands::build(BuildOptions {
        config: config.clone(),
        output: built.clone(),
    })
    .unwrap();
    let first = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
        .unwrap()
        .canonical_export()
        .unwrap();
    let input = output.path().join("portable.sqlite");
    commands::extract(ExtractOptions {
        config,
        output: Some(input.clone()),
    })
    .unwrap();
    assert_eq!(
        Snapshot::load(&input).unwrap().canonical_export().unwrap(),
        first
    );
    drop(root);
    let before = std::fs::read(&input).unwrap();
    let generated = output.path().join("generated");
    commands::generate(GenerateOptions {
        input: input.clone(),
        output: generated.clone(),
    })
    .unwrap();
    support::assert_output_tree(&built, &generated);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    let html = std::fs::read_to_string(generated.join("index.html")).unwrap();
    assert!(html.contains("must never execute"));
    assert!(!html.contains("RuntimeError:"));
}

#[test]
fn failure_preserves_published_snapshot_and_site_and_guards_inputs() {
    let root = static_workspace();
    let config = root.path().join("diplodocus.toml");
    let output = root.path().join("site");
    let build = || {
        commands::build(BuildOptions {
            config: config.clone(),
            output: output.clone(),
        })
    };
    build().unwrap();
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = std::fs::read(&database).unwrap();
    let html = std::fs::read(output.join("index.html")).unwrap();
    root.write("docs/index.qmd", "[`nonexistent`]\n");
    assert!(build().is_err());
    assert_eq!(std::fs::read(&database).unwrap(), snapshot);
    assert_eq!(std::fs::read(output.join("index.html")).unwrap(), html);
    root.write("docs/index.qmd", "# Recovered\n");
    for destination in [
        &config,
        &root.path().join("docs"),
        &root.path().join("docs/index.qmd"),
        root.path(),
    ] {
        assert!(
            commands::build(BuildOptions {
                config: config.clone(),
                output: destination.to_owned()
            })
            .is_err()
        );
        assert!(
            commands::extract(ExtractOptions {
                config: config.clone(),
                output: Some(destination.to_owned())
            })
            .is_err()
        );
    }
    assert!(
        commands::generate(GenerateOptions {
            input: database.clone(),
            output: database.parent().unwrap().into()
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&database).unwrap(), snapshot);
    let valid_config = root.read("diplodocus.toml");
    root.write("diplodocus.toml", format!("{valid_config}\n[[content]]\nid='duplicate-route'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='qmd'\n"));
    assert!(build().is_err());
    assert_ne!(std::fs::read(&database).unwrap(), snapshot);
    assert!(Snapshot::load(&database).is_ok());
    assert_eq!(std::fs::read(output.join("index.html")).unwrap(), html);
    root.write("diplodocus.toml", valid_config);
    let occupied = root.path().join("occupied");
    root.write("occupied/sentinel", "keep");
    assert!(
        commands::build(BuildOptions {
            config,
            output: occupied.clone()
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(occupied.join("sentinel")).unwrap(),
        "keep"
    );
    assert_ne!(std::fs::read(database).unwrap(), snapshot);
    assert_eq!(std::fs::read(output.join("index.html")).unwrap(), html);
}
