#![cfg(target_os = "linux")]
mod support;

use diplodocus::commands::{self, BuildOptions};
use diplodocus::snapshots::Snapshot;
use serde_json::Value;

fn producing_origin(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("origin").is_some_and(|v| v == "cache") {
                fields.insert("origin".into(), Value::String("executed".into()));
            }
            fields.values_mut().for_each(producing_origin);
        }
        Value::Array(values) => values.iter_mut().for_each(producing_origin),
        _ => {}
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
        build(output.path().join("cached"));
        assert_eq!(root.read("guide/executions.txt"), "executed\n", "{kernel}");
        support::assert_output_tree(&output.path().join("first"), &output.path().join("cached"));
        let second = Snapshot::load(&database).unwrap();
        let mut workspace = serde_json::to_value(second.workspace()).unwrap();
        assert!(workspace.to_string().contains("\"origin\":\"cache\""));
        producing_origin(&mut workspace);
        assert_eq!(
            workspace,
            serde_json::to_value(first.workspace()).unwrap(),
            "{kernel}"
        );
        assert_eq!(first.assets(), second.assets());
        for id in first.workspace().pages.keys() {
            if let Some(first) = first.executed_page(id) {
                let second = second.executed_page(id).unwrap();
                let mut record = serde_json::to_value(second.record()).unwrap();
                producing_origin(&mut record);
                assert_eq!(record, serde_json::to_value(first.record()).unwrap());
            }
        }
        let relocated = support::TestWorkspace::new();
        for path in support::files_under(root.path()) {
            if path == std::path::Path::new("diplodocus.toml")
                || path == std::path::Path::new("guide/index.qmd")
                || path == std::path::Path::new("environment.txt")
                || path.starts_with(".diplodocus/cache")
            {
                relocated.write(&path, std::fs::read(root.path().join(&path)).unwrap());
            }
        }
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
        root.write("environment.txt", "changed");
        build(output.path().join("changed"));
        assert_eq!(
            root.read("guide/executions.txt"),
            "executed\nexecuted\n",
            "{kernel}"
        );
        support::assert_output_tree(&output.path().join("first"), &output.path().join("changed"));
    }
}
