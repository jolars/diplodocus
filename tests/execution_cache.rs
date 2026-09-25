#![cfg(target_os = "linux")]
mod support;

use diplodocus::commands::{self, BuildOptions};
use diplodocus::ir::{ExecutionOrigin, ProvenanceActivity};
use diplodocus::snapshots::Snapshot;
use serde_json::Value;

fn producing_origin(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("kind").is_some_and(|v| v == "execution")
                && fields.get("origin").is_some_and(|v| v == "cache")
            {
                fields.insert("origin".into(), Value::String("executed".into()));
            }
            fields.values_mut().for_each(producing_origin);
        }
        Value::Array(values) => values.iter_mut().for_each(producing_origin),
        _ => {}
    }
}

fn assert_page_origins(snapshot: &Snapshot, expected: impl Fn(&str) -> Option<ExecutionOrigin>) {
    assert_eq!(snapshot.workspace().pages.len(), 3);
    for (id, page) in &snapshot.workspace().pages {
        let expected = expected(&page.title);
        let activities: Vec<_> = page
            .document
            .provenance
            .iter()
            .filter_map(|p| {
                if let ProvenanceActivity::Execution { origin, .. } = p.activity {
                    Some((p, origin))
                } else {
                    None
                }
            })
            .collect();
        if let Some(origin) = expected {
            assert_eq!(activities.len(), 1, "{id}");
            assert_eq!(activities[0].1, origin, "{id}");
            let executed = snapshot.executed_page(id).unwrap();
            assert_eq!(
                &executed.record().provenance.as_ref().unwrap().execution,
                activities[0].0
            );
        } else {
            assert!(activities.is_empty(), "{id}");
            assert!(snapshot.executed_page(id).is_none(), "{id}");
        }
    }
}

fn enabled_origin(title: &str, origin: ExecutionOrigin) -> Option<ExecutionOrigin> {
    (title != "Disabled").then_some(origin)
}

fn copy_cache(source: &std::path::Path, destination: &std::path::Path) {
    // Empty asset directories belong to valid text-only cache entries, too.
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_cache(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn assert_portable_results_equal(expected: &Snapshot, actual: &Snapshot) {
    let mut workspace = serde_json::to_value(actual.workspace()).unwrap();
    producing_origin(&mut workspace);
    assert_eq!(
        workspace,
        serde_json::to_value(expected.workspace()).unwrap()
    );
    assert_eq!(expected.assets(), actual.assets());
    for id in expected.workspace().pages.keys() {
        match (expected.executed_page(id), actual.executed_page(id)) {
            (Some(expected), Some(actual)) => {
                let mut record = serde_json::to_value(actual.record()).unwrap();
                producing_origin(&mut record);
                assert_eq!(
                    record,
                    serde_json::to_value(expected.record()).unwrap(),
                    "{id}"
                );
            }
            (None, None) => {}
            _ => panic!("execution presence differs for {id}"),
        }
    }
}

#[test]
fn python_and_r_builds_restore_portable_results_and_identical_sites() {
    for (kernel, source) in [
        (
            "python3",
            "# Cached page\n\n```{python}\nfrom pathlib import Path\np = Path('executions.txt')\nwith p.open('a') as f: f.write('executed\\n')\nvalue = 40\n```\n\n```{python}\nprint(value + 2)\nfrom IPython.display import display, HTML, Markdown, SVG\ndisplay(HTML('<strong>safe html</strong>'))\ndisplay(Markdown('**safe markdown**'))\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\"))\n```\n",
        ),
        (
            "ir",
            "# Cached page\n\n```{r}\ncat('executed\\n', file='executions.txt', append=TRUE)\nvalue <- 40\n```\n\n```{r}\ncat(value + 2, '\\n')\nIRdisplay::display_html('<strong>safe html</strong>')\nIRdisplay::display_markdown('**safe markdown**')\nIRdisplay::display_svg(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\")\n```\n",
        ),
    ] {
        let root = support::TestWorkspace::new();
        root.write("diplodocus.toml", format!("[project]\nname='Cache'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='qmd'\n[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='{kernel}'\ndeclared_environment_inputs=['environment.txt']\n"));
        root.write("diplodocus.toml", format!("{}\n[[content]]\nid='disabled'\nowner='project'\nrepository='docs'\npath='disabled'\nmount='disabled'\nformat='qmd'\n[content.execution]\nmode='never'\n", root.read("diplodocus.toml")));
        root.write(
            "disabled/index.qmd",
            "# Disabled\n\n```{python}\nraise RuntimeError('must not execute')\n```\n",
        );
        root.write(
            "guide/other.qmd",
            if kernel == "python3" {
                "# Other\n\n```{python}\nprint(7)\n```\n"
            } else {
                "# Other\n\n```{r}\ncat(7)\n```\n"
            },
        );
        root.write("guide/index.qmd", source);
        root.write("environment.txt", "first");
        let output = tempfile::tempdir().unwrap();
        let build = |destination| {
            commands::build(BuildOptions {
                config: root.path().join("diplodocus.toml"),
                output: destination,
            })
            .unwrap()
        };
        build(output.path().join("first"));
        let database = root.path().join(".diplodocus/documentation.sqlite");
        let first = Snapshot::load(&database).unwrap();
        assert_page_origins(&first, |title| {
            enabled_origin(title, ExecutionOrigin::Executed)
        });
        build(output.path().join("cached"));
        assert_eq!(root.read("guide/executions.txt"), "executed\n", "{kernel}");
        support::assert_output_tree(&output.path().join("first"), &output.path().join("cached"));
        let second = Snapshot::load(&database).unwrap();
        assert_page_origins(&second, |title| {
            enabled_origin(title, ExecutionOrigin::Cache)
        });
        assert_portable_results_equal(&first, &second);
        let relocated = support::TestWorkspace::new();
        for path in support::files_under(root.path()) {
            if path == std::path::Path::new("diplodocus.toml")
                || path.starts_with("guide") && path.extension().is_some_and(|e| e == "qmd")
                || path.starts_with("disabled")
                || path == std::path::Path::new("environment.txt")
            {
                relocated.write(&path, std::fs::read(root.path().join(&path)).unwrap());
            }
        }
        copy_cache(
            &root.path().join(".diplodocus/cache"),
            &relocated.path().join(".diplodocus/cache"),
        );
        commands::build(BuildOptions {
            config: relocated.path().join("diplodocus.toml"),
            output: output.path().join("relocated"),
        })
        .unwrap();
        assert!(
            !relocated.path().join("guide/executions.txt").exists(),
            "relocated {kernel} cache must skip execution"
        );
        support::assert_output_tree(
            &output.path().join("first"),
            &output.path().join("relocated"),
        );
        let moved =
            Snapshot::load(relocated.path().join(".diplodocus/documentation.sqlite")).unwrap();
        assert_page_origins(&moved, |title| {
            enabled_origin(title, ExecutionOrigin::Cache)
        });
        assert_portable_results_equal(&first, &moved);
        for snapshot in [&first, &second, &moved] {
            let serialized = serde_json::to_string(snapshot.workspace()).unwrap();
            assert!(!serialized.contains(root.path().to_str().unwrap()));
            assert!(!serialized.contains(relocated.path().to_str().unwrap()));
        }

        root.write(
            "guide/other.qmd",
            format!("{}\nChanged prose.\n", root.read("guide/other.qmd")),
        );
        build(output.path().join("mixed"));
        let mixed = Snapshot::load(&database).unwrap();
        assert_page_origins(&mixed, |title| match title {
            "Disabled" => None,
            "Other" => Some(ExecutionOrigin::Executed),
            "Cached page" => Some(ExecutionOrigin::Cache),
            title => panic!("unexpected page {title}"),
        });
        assert_eq!(root.read("guide/executions.txt"), "executed\n");
        root.write("environment.txt", "changed");
        build(output.path().join("changed"));
        assert_eq!(
            root.read("guide/executions.txt"),
            "executed\nexecuted\n",
            "{kernel}"
        );
        support::assert_output_tree(&output.path().join("mixed"), &output.path().join("changed"));
        let changed = Snapshot::load(&database).unwrap();
        assert_page_origins(&changed, |title| {
            enabled_origin(title, ExecutionOrigin::Executed)
        });
    }
}
