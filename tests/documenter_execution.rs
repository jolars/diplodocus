mod support;

use diplodocus::ir::{DocumenterOutput, ProvenanceActivity};
use diplodocus::rendering::{FragmentIndex, render_fragments, render_site};
use diplodocus::site::{Site, UrlMap};
use diplodocus::snapshots::Snapshot;
use std::fs;
use std::process::{Command, Output};

fn workspace(body: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::from_fixture("julia-documenter");
    root.write("docs/index.md", "# Demo guide\n");
    root.write("diplodocus.yaml", root.read("diplodocus.yaml") + "    execution:\n      mode: execute\n      engine: julia\n      julia-project: docs/Project.toml\n      declared-environment-inputs: [figure.png]\n");
    root.write("docs/Project.toml", "[deps]\nDemo = '12345678-1234-1234-1234-123456789abc'\nPrepared = '12345678-1234-1234-1234-123456789abd'\n");
    root.write("docs/Manifest.toml", "manifest_format = '2.0'\n[[deps.Demo]]\npath = '..'\nuuid = '12345678-1234-1234-1234-123456789abc'\nversion = '1.0.0'\n[[deps.Prepared]]\npath = '../dependency'\nuuid = '12345678-1234-1234-1234-123456789abd'\nversion = '0.1.0'\n");
    root.write(
        "dependency/Project.toml",
        "name = 'Prepared'\nuuid = '12345678-1234-1234-1234-123456789abd'\nversion = '0.1.0'\n",
    );
    root.write(
        "dependency/src/Prepared.jl",
        "module Prepared\nconst OFFSET = 40\nend\n",
    );
    root.write("docs/guide/gallery.md", body);
    image::RgbaImage::new(2, 2)
        .save(root.path().join("figure.png"))
        .unwrap();
    root
}

fn command(root: &support::TestWorkspace, operation: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args([operation, "--config", "diplodocus.yaml"])
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

const EXAMPLES: &str = r##"# Gallery

```@example gallery
using Demo, Prepared # hide
shared = Prepared.OFFSET # hide
struct NativeFigure end # hide
Base.show(io::IO, ::MIME"image/png", ::NativeFigure) = write(io, read("../../figure.png")) # hide
nothing # hide
```

```@example gallery
shared += 1
println("discarded native stream")
shared + 1;
```

```@example gallery
@assert ans == 42
nothing # hide
```

```@example
@assert !isdefined(@__MODULE__, :shared)
shared = 99
nothing # hide
```

```@example
@assert !isdefined(@__MODULE__, :shared)
literal = "# hide"
println("native stdout")
print(stderr, "native stderr")
nothing # hide
```

```@example gallery
NativeFigure()
```

- Nested output:

  ```@example gallery
  @assert shared == 41
  println("nested native output")
  ```
"##;

#[test]
fn native_documenter_examples_retain_sessions_figures_and_portable_evidence() {
    let root = workspace(EXAMPLES);
    root.write("docs/second.md", "# Another page\n\n```@example gallery\n@assert !isdefined(@__MODULE__, :shared)\nprintln(\"fresh page\")\n```\n");
    let figure = fs::read(root.path().join("figure.png")).unwrap();
    success(command(&root, "check"));
    success(command(&root, "build"));
    let original = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = Snapshot::load(&original).unwrap();
    let mut example_count = 0;
    let mut images = 0;
    for page in snapshot.workspace().pages.values() {
        for record in &page.document.provenance {
            if let ProvenanceActivity::DocumenterExamples {
                julia_version,
                dependencies,
                environment_inputs,
                examples,
                ..
            } = &record.activity
            {
                assert!(!julia_version.is_empty());
                assert!(dependencies.contains_key("12345678-1234-1234-1234-123456789abc"));
                if page.title == "Gallery" {
                    assert!(dependencies.contains_key("12345678-1234-1234-1234-123456789abd"));
                }
                assert_eq!(
                    environment_inputs
                        .iter()
                        .map(|input| input.source.path.as_str())
                        .collect::<Vec<_>>(),
                    ["docs/Manifest.toml", "docs/Project.toml", "figure.png"]
                );
                example_count += examples.len();
                images += examples
                    .iter()
                    .flat_map(|example| &example.outputs)
                    .filter(|output| matches!(output, DocumenterOutput::Image { .. }))
                    .count();
            }
        }
    }
    assert_eq!(example_count, 8);
    assert_eq!(images, 1);
    assert!(
        !snapshot
            .canonical_export()
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
    let artifacts = tempfile::tempdir().unwrap();
    let detached = artifacts.path().join("relocated.sqlite");
    fs::rename(original, &detached).unwrap();
    drop(snapshot);
    drop(root);
    let snapshot = Snapshot::load(&detached).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let gallery = index
        .documents
        .iter()
        .find(|document| document.title == "Gallery")
        .unwrap();
    let markdown = std::str::from_utf8(&fragments.files()[&gallery.markdown_path].bytes).unwrap();
    let html = std::str::from_utf8(&fragments.files()[&gallery.path].bytes).unwrap();
    let full = std::str::from_utf8(&standalone.files()[&gallery.path].bytes).unwrap();
    for body in [markdown, html, full] {
        for text in [
            "42",
            "native stdout",
            "native stderr",
            "nested native output",
            "# hide",
        ] {
            assert!(body.contains(text), "{body}");
        }
        for text in [
            "Prepared.OFFSET",
            "struct NativeFigure",
            "nothing # hide",
            "discarded native stream\n42",
        ] {
            assert!(!body.contains(text), "{body}");
        }
    }
    let figure_path = fragments
        .files()
        .iter()
        .find(|(_, file)| file.bytes == figure)
        .map(|(path, _)| path.clone())
        .unwrap();
    assert_eq!(standalone.files()[&figure_path].bytes, figure);
    let asset = format!("/retained/{}", figure_path.rsplit('/').next().unwrap());
    let mapped = render_fragments(
        &Site::new(&snapshot)
            .unwrap()
            .with_url_map(UrlMap {
                format_version: 1,
                documents: index
                    .documents
                    .iter()
                    .enumerate()
                    .map(|(i, doc)| (doc.path.clone(), format!("/docs/{i}/")))
                    .collect(),
                assets: [(figure_path.clone(), asset.clone())].into(),
            })
            .unwrap(),
    )
    .unwrap();
    for path in [&gallery.path, &gallery.markdown_path] {
        assert!(
            std::str::from_utf8(&mapped.files()[path].bytes)
                .unwrap()
                .contains(&asset)
        );
    }
    assert_eq!(mapped.files()[&figure_path].bytes, figure);
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .env("PATH", "")
        .args([
            "generate",
            "--input",
            detached.to_str().unwrap(),
            "--output",
            artifacts.path().join("site").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    success(output);
}

#[test]
fn checks_disabled_pages_and_display_fences_need_no_julia() {
    for (body, disabled) in [
        ("# Plain\n\n```julia\nerror(\"illustrative\")\n```\n", false),
        (
            "# Disabled\n\n```@example\nerror(\"disabled\")\n```\n",
            true,
        ),
    ] {
        let root = project_workspace(body);
        if disabled {
            root.write(
                "diplodocus.yaml",
                root.read("diplodocus.yaml")
                    .split("    execution:")
                    .next()
                    .unwrap(),
            );
        }
        for operation in ["check", "extract"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_diplodocus"))
                    .current_dir(root.path())
                    .env("PATH", "")
                    .args([operation, "--config", "diplodocus.yaml"])
                    .output()
                    .unwrap(),
            );
        }
    }
    let root = workspace("# Check\n\n```@example\nerror(\"must remain inert\")\n```\n");
    success(
        Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", "")
            .args(["check", "--config", "diplodocus.yaml"])
            .output()
            .unwrap(),
    );
}

fn project_workspace(body: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Native Documenter\nrepositories:\n  - id: repo\n    path: .\ncontent:\n  - id: manual\n    owner: project\n    repository: repo\n    path: docs\n    mount: manual\n    format: documenter\n    execution:\n      mode: execute\n      engine: julia\n      julia-project: Project.toml\n");
    root.write("Project.toml", "[deps]\n");
    root.write("docs/index.md", body);
    root
}

#[test]
fn native_documenter_preserves_ordered_displays_and_literal_text() {
    let root = project_workspace(
        r##"# Displays

```@example
struct NativeSVG end
Base.show(io::IO, ::MIME"image/svg+xml", ::NativeSVG) = print(io, "<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><circle cx='0' cy='0' r='1'/></svg>")
print("before")
display(NativeSVG())
print("after")
display("**literal text**")
print("<script>literal output</script>")
nothing # hide
```
"##,
    );
    success(command(&root, "extract"));
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let outputs = snapshot
        .workspace()
        .pages
        .values()
        .flat_map(|page| &page.document.provenance)
        .find_map(|record| match &record.activity {
            ProvenanceActivity::DocumenterExamples { examples, .. } => Some(&examples[0].outputs),
            _ => None,
        })
        .unwrap();
    assert!(matches!(&outputs[0], DocumenterOutput::Text { text } if text == "before"));
    assert!(
        matches!(&outputs[1], DocumenterOutput::Image { media_type, .. } if media_type == "image/svg+xml")
    );
    assert!(matches!(&outputs[2], DocumenterOutput::Text { text } if text == "after"));
    assert!(
        matches!(&outputs[3], DocumenterOutput::Text { text } if text == "\"**literal text**\"")
    );
    assert!(
        matches!(&outputs[4], DocumenterOutput::Text { text } if text == "<script>literal output</script>")
    );
    let fragments = render_fragments(&Site::new(&snapshot).unwrap()).unwrap();
    let html = std::str::from_utf8(&fragments.files()["manual/index.html"].bytes).unwrap();
    assert!(!html.contains("<script>literal output</script>"));
    assert!(html.contains("&lt;script&gt;literal output&lt;/script&gt;"));
}

#[test]
fn native_documenter_rejects_changed_environment_inputs_and_missing_julia() {
    let root = project_workspace(
        "# Changes\n\n```@example\nwrite(\"../Project.toml\", \"[deps]\\n# changed\\n\")\nnothing\n```\n",
    );
    assert!(!command(&root, "extract").status.success());
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
    let root = project_workspace("# Missing Julia\n\n```@example\n42\n```\n");
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("PATH", "")
        .args(["extract", "--config", "diplodocus.yaml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("require Julia"));
}

#[tokio::test]
async fn native_documenter_timeouts_and_cancellation_prevent_publication() {
    use diplodocus::commands::{ExtractOptions, extract_with};
    use diplodocus::execution::ExecutionDeadlines;
    for cancel in [false, true] {
        let root =
            project_workspace("# Long example\n\n```@example\nwhile true; sleep(0.1); end\n```\n");
        let snapshot = root.path().join("result.sqlite");
        let result = extract_with(
            ExtractOptions {
                config: root.path().join("diplodocus.yaml"),
                output: Some(snapshot.clone()),
            },
            ExecutionDeadlines {
                startup: 10_000,
                cell: 100,
                termination: 100,
                forced_exit: 100,
                ..ExecutionDeadlines::default()
            },
            if cancel {
                Box::pin(async { tokio::time::sleep(std::time::Duration::from_millis(100)).await })
            } else {
                Box::pin(std::future::pending())
            },
        )
        .await;
        assert!(result.is_err());
        assert!(!snapshot.exists());
    }
}

#[test]
fn native_documenter_errors_preserve_previous_publications() {
    let root = workspace("# Gallery\n\n```@example\nprintln(\"valid result\")\n```\n");
    success(command(&root, "build"));
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = fs::read(&database).unwrap();
    let index = fs::read(root.path().join("site/index.html")).unwrap();
    for bad in [
        "error(\"unexpected native failure\")",
        "using MissingPreparedDependency",
        "struct InvalidImage end\nBase.show(io::IO, ::MIME\"image/png\", ::InvalidImage) = print(io, \"not a PNG\")\nInvalidImage()",
        "struct UnsafeImage end\nBase.show(io::IO, ::MIME\"image/svg+xml\", ::UnsafeImage) = print(io, \"<svg xmlns='http://www.w3.org/2000/svg'><script>bad()</script></svg>\")\nUnsafeImage()",
    ] {
        root.write(
            "docs/guide/gallery.md",
            format!("# Gallery\n\n```@example\n{bad}\n```\n"),
        );
        success(command(&root, "check"));
        let output = command(&root, "build");
        assert!(!output.status.success(), "{bad}");
        assert_eq!(fs::read(&database).unwrap(), snapshot);
        assert_eq!(
            fs::read(root.path().join("site/index.html")).unwrap(),
            index
        );
    }
}

#[test]
fn unsupported_documenter_execution_syntax_is_attributed_before_launch() {
    for header in [
        "@example shared; continued = true",
        "@repl shared",
        "@setup shared",
        "@eval",
        "@meta",
    ] {
        let root = workspace(&format!(
            "# Gallery\n\n```{header}\nerror(\"must remain inert\")\n```\n"
        ));
        let error = diplodocus::assembly::assemble_workspace(root.path().join("diplodocus.yaml"))
            .unwrap_err();
        let diplodocus::assembly::AssemblyError::Diagnostics(diagnostics) = error else {
            panic!("{error}")
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.span.is_some() && diagnostic.source.is_some())
        );
    }
}
