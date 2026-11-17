mod support;

use std::sync::{Arc, Mutex};

use diplodocus::commands::{CheckOptions, CommandRunner};
use diplodocus::reporting::{Event, Reporter};

#[derive(Default)]
struct Events(Mutex<Vec<(String, bool)>>);
impl Reporter for Events {
    fn report(&self, event: Event<'_>) {
        if let Event::Cache {
            stage: "static-python",
            owner,
            hit,
            ..
        } = event
        {
            self.0.lock().unwrap().push((owner.into(), hit));
        }
    }
}

#[test]
fn inferred_and_declared_dependencies_propagate_through_cycles() {
    let root = support::TestWorkspace::new();
    let mut configuration = "project:\n  name: Dependency invalidation\nrepositories:\n  - id: repo\n    path: .\npackages:\n".to_owned();
    for (name, extra) in [
        ("first", "    incremental:\n      depends-on: [second]\n"),
        ("second", "    incremental:\n      depends-on: [first]\n"),
        ("third", ""),
    ] {
        configuration.push_str(&format!("  - id: {name}\n    name: {name}\n    slug: {name}\n    ecosystem: python\n    repository: repo\n    path: {name}\n    metadata-path: pyproject.toml\n{extra}    targets:\n      - id: api\n        extractor: python\n        path: {name}\n        role: public-api\n"));
        root.write(
            format!("{name}/pyproject.toml"),
            format!(
                "[project]\nname = '{name}'\nversion = '1.0.0'\n{}",
                if name == "third" {
                    "dependencies = ['first>=1']\n"
                } else {
                    ""
                }
            ),
        );
        root.write(
            format!("{name}/{name}/__init__.py"),
            "def answer():\n    return 42\n",
        );
    }
    root.write("diplodocus.yaml", configuration);
    let events = Arc::new(Events::default());
    let check = || {
        events.0.lock().unwrap().clear();
        CommandRunner::with_reporter(events.clone())
            .check(CheckOptions {
                config: root.path().join("diplodocus.yaml"),
            })
            .unwrap();
        events.0.lock().unwrap().clone()
    };
    assert!(check().iter().all(|(_, hit)| !hit));
    assert!(check().iter().all(|(_, hit)| *hit));
    root.write(
        "second/second/__init__.py",
        "# A changed dependency.\ndef answer():\n    return 42\n",
    );
    let changed = check();
    for package in ["first", "second", "third"] {
        assert!(changed.contains(&(package.into(), false)), "{changed:?}");
    }

    root.write(
        "second/second/target/added.py",
        "def added():\n    return 1\n",
    );
    let added = check();
    for package in ["first", "second", "third"] {
        assert!(added.contains(&(package.into(), false)), "{added:?}");
    }
}

#[test]
fn declared_environment_values_change_persistent_identities() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Environment cache\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Demo\n    slug: demo\n    ecosystem: python\n    repository: repo\n    path: package\n    metadata-path: pyproject.toml\n    incremental:\n      environment-variables: [DIPLODOCUS_TEST_ENVIRONMENT]\n    targets:\n      - id: api\n        extractor: python\n        path: demo\n        role: public-api\n");
    root.write(
        "package/pyproject.toml",
        "[project]\nname = 'demo'\nversion = '1.0.0'\n",
    );
    root.write("package/demo/__init__.py", "def answer():\n    return 42\n");
    let check = |value| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("DIPLODOCUS_TEST_ENVIRONMENT", value)
            .arg("check")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read_dir(
            root.path()
                .join(".diplodocus/cache/incremental/static-python"),
        )
        .unwrap()
        .count()
    };
    assert_eq!(check("one"), 1);
    assert_eq!(check("one"), 1);
    assert_eq!(check("two"), 2);
    assert_eq!(check(""), 3);
}
