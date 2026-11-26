mod support;

use std::fs;
use std::process::{Command, Output};

const GUIDE: &str = r#"# Shared guide

```python run package=demo
from generated_demo import answer
from pathlib import Path
Path("executed").touch()
value = answer()
print("Python result:", value)
```

```r run
value <- answer()
cat("R result:", value, "\n")
plot(1:3, c(3, 1, 2))
```

```python run
print("Python continued:", value + 1)
```

```r run
cat("R continued:", value + 1L, "\n")
```

```python
raise AssertionError("illustrative fence ran")
```
"#;

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

#[test]
#[ignore = "native Python, Sphinx, and R guide acceptance"]
fn shared_guide_runs_against_both_checkouts_and_generates_offline() {
    let root = support::TestWorkspace::from_fixture("generated-python");
    root.write(
        "generated_demo/__init__.py",
        "def answer():\n    return 42\n",
    );
    root.write("r/DESCRIPTION", "Package: guidedemo\nVersion: 0.1.0\nTitle: Guide Demo\nDescription: A guide fixture.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <author@example.com>\nEncoding: UTF-8\n");
    root.write("r/NAMESPACE", "export(answer)\n");
    root.write("r/R/api.R", "answer <- function() 27L\n");
    root.write(
        "r/man/answer.Rd",
        "\\name{answer}\\alias{answer}\\title{Answer}\\usage{answer()}\\description{Answer.}\n",
    );
    root.write("diplodocus.yaml", format!("{}  - id: r\n    name: R demo\n    slug: r\n    ecosystem: r\n    repository: repo\n    path: r\n    metadata-path: DESCRIPTION\n    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\ncontent:\n  - path: docs\n    mount: ''\n    execution: {{ mode: execute, engine: native }}\n", root.read("diplodocus.yaml")));
    root.write("docs/guide.md", GUIDE);
    success(command(&root, &["check"]));
    assert!(!root.path().join("docs/executed").exists());

    success(command(&root, &["build"]));
    assert!(root.path().join("docs/executed").exists());
    let html = root.read("site/guide.html");
    let mut previous = 0;
    for expected in [
        "Python result: 42",
        "R result: 27",
        "Python continued: 43",
        "R continued: 28",
    ] {
        let index = html
            .find(expected)
            .unwrap_or_else(|| panic!("missing {expected}"));
        assert!(index > previous);
        previous = index;
    }
    let plot = html[html.find("R result: 27").unwrap()..html.find("Python continued: 43").unwrap()]
        .split_once("<img ")
        .and_then(|(_, image)| image.split_once("src=\""))
        .and_then(|(_, source)| source.split_once('"'))
        .map(|(source, _)| source)
        .expect("R plot between the two output blocks");
    let figure = fs::read(root.path().join("site").join(plot)).unwrap();
    assert!(figure.starts_with(b"\x89PNG\r\n\x1a\n"));
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = fs::read(&database).unwrap();

    root.write(
        "docs/guide.md",
        format!("{GUIDE}\n```r run\nstop('broken guide')\n```\n"),
    );
    let output = command(&root, &["build"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("broken guide"));
    assert_eq!(fs::read(&database).unwrap(), snapshot);
    assert_eq!(root.read("site/guide.html"), html);

    for directory in ["generated_demo", "r", "docs"] {
        fs::remove_dir_all(root.path().join(directory)).unwrap();
    }
    success(
        Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", "")
            .args([
                "generate",
                "--input",
                ".diplodocus/documentation.sqlite",
                "--output",
                "portable",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(root.read("portable/guide.html"), html);
    assert_eq!(
        fs::read(root.path().join("portable").join(plot)).unwrap(),
        figure
    );
    assert_eq!(root.read("portable/guide.md"), root.read("site/guide.md"));
}
