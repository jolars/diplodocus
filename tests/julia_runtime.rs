mod support;

use std::fs;
use std::process::Command;

use diplodocus::ir::{ItemKind, ProvenanceActivity};
use diplodocus::snapshots::Snapshot;

fn workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        r#"project:
  name: "Generated Julia exceptions"
repositories:
  - id: "repo"
    path: "."
packages:
  - id: "demo"
    name: "Demo"
    slug: "demo"
    ecosystem: "julia"
    repository: "repo"
    path: "package"
    metadata-path: "Project.toml"
    targets:
      - id: "api"
        extractor: "julia"
        path: "src/Demo.jl"
        role: "public-api"
"#,
    );
    root.write(
        "package/Project.toml",
        "name = \"Demo\"\nuuid = \"12345678-1234-1234-1234-123456789abc\"\nversion = \"1.2.3\"\n",
    );
    let mut source = String::from(
        "module Demo\nabstract type PackageError <: Exception end\nmacro define_error(name)\nquote\nstruct $(esc(name)) <: PackageError\nmessage::String\nend\nend\nend\n",
    );
    for i in 1..=15 {
        source.push_str(&format!("@define_error Error{i}\nexport Error{i}\n"));
    }
    source.push_str("write(\"loaded\", \"yes\")\nend\n");
    root.write("package/src/Demo.jl", source);
    root
}

fn command(root: &support::TestWorkspace, name: &str, no_runtime: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    cmd.current_dir(root.path())
        .args([name, "--config", "diplodocus.yaml"]);
    if no_runtime {
        cmd.env("PATH", "");
    }
    cmd.output().unwrap()
}

#[test]
fn generated_types_complete_extraction_while_check_stays_static_and_generation_is_portable() {
    let root = workspace();
    let checked = command(&root, "check", true);
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr).contains("julia-unresolved-definition"));
    assert!(!root.path().join("package/loaded").exists());
    let output = command(&root, "extract", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.path().join("package/loaded").exists());
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let package = &snapshot.workspace().packages["demo"];
    for i in 1..=15 {
        let item = package
            .items
            .values()
            .find(|v| v.qualified_name == format!("Demo.Error{i}"))
            .unwrap();
        assert_eq!(item.kind, ItemKind::Type);
        let module = package
            .items
            .values()
            .find(|v| v.qualified_name == "Demo")
            .unwrap();
        let id = package
            .items
            .iter()
            .find(|(_, v)| v.qualified_name == item.qualified_name)
            .unwrap()
            .0;
        assert!(module.children.contains(id));
        assert!(item.source_location.is_none());
        assert_eq!(item.children.len(), 1);
        assert_eq!(item.signatures.len(), 1);
        assert!(item.signatures[0].sources.is_empty());
        assert!(
            item.provenance
                .iter()
                .all(|p| p.source.is_none() && p.span.is_none())
        );
        assert!(item.provenance.iter().any(|p| matches!(&p.activity, ProvenanceActivity::JuliaRuntime { package_version, .. } if package_version == "1.2.3")));
    }
    fs::remove_dir_all(root.path().join("package")).unwrap();
    let generated = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("PATH", "")
        .args([
            "generate",
            "--input",
            ".diplodocus/documentation.sqlite",
            "--output",
            "site",
        ])
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
}

#[test]
fn missing_runtime_and_package_load_failure_preserve_the_previous_snapshot() {
    let root = workspace();
    root.write(".diplodocus/documentation.sqlite", "previous snapshot");
    let missing = command(&root, "extract", true);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("requires Julia"));
    root.write(
        "package/src/Demo.jl",
        fs::read_to_string(root.path().join("package/src/Demo.jl"))
            .unwrap()
            .replace("write(\"loaded\", \"yes\")", "error(\"load failed\")"),
    );
    let failed = command(&root, "extract", false);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("load failed"));
    assert_eq!(
        fs::read_to_string(root.path().join(".diplodocus/documentation.sqlite")).unwrap(),
        "previous snapshot"
    );
}

#[test]
fn plain_checkouts_are_validated_before_publication() {
    for broken in ["runtime", "dependency", "checkout", "load"] {
        let root = workspace();
        root.write("package/src/Demo.jl", "module Demo\nvalue = 42\nend\n");
        let built = command(&root, "build", false);
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let snapshot = root.path().join(".diplodocus/documentation.sqlite");
        let previous = fs::read(&snapshot).unwrap();
        let site = root.path().join("site/index.html");
        let previous_site = fs::read(&site).unwrap();
        let (source, expected) = match broken {
            "dependency" => (
                "module Demo\nusing UnavailableDocumentationDependency\nend\n",
                "UnavailableDocumentationDependency",
            ),
            "load" => (
                "module Demo\nerror(\"checkout load failed\")\nend\n",
                "checkout load failed",
            ),
            _ => ("module Demo\nvalue = 42\nend\n", ""),
        };
        root.write("package/src/Demo.jl", source);
        if broken == "checkout" {
            root.write("package/src/Other.jl", source);
            let config = fs::read_to_string(root.path().join("diplodocus.yaml")).unwrap();
            root.write(
                "diplodocus.yaml",
                config.replace("src/Demo.jl", "src/Other.jl"),
            );
        }
        let checked = command(&root, "check", true);
        assert!(
            checked.status.success(),
            "{broken}: {}",
            String::from_utf8_lossy(&checked.stderr)
        );
        for operation in ["extract", "build"] {
            let failed = command(&root, operation, broken == "runtime");
            assert!(!failed.status.success(), "{broken}: {operation}");
            let expected = match broken {
                "runtime" => "requires Julia",
                "checkout" => "does not match the supplied checkout",
                _ => expected,
            };
            assert!(
                String::from_utf8_lossy(&failed.stderr).contains(expected),
                "{}",
                String::from_utf8_lossy(&failed.stderr)
            );
            assert_eq!(previous, fs::read(&snapshot).unwrap());
            assert_eq!(previous_site, fs::read(&site).unwrap());
        }
    }
}
