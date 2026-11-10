mod support;

use std::collections::BTreeSet;
use std::process::{Command, Output};

use diplodocus::ir::{CppDeclarationKind, CppProtection, ItemLanguageData};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_item;

fn command(root: &support::TestWorkspace, operation: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args([operation, "--config"])
        .arg(root.path().join("diplodocus.yaml"))
        .output()
        .unwrap()
}

fn successful(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
}

#[test]
fn extracts_cpp_templates_overloads_visibility_and_portable_generation() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    successful(command(&root, "check"));
    successful(command(&root, "extract"));
    let snapshot = snapshot(&root);
    let workspace = snapshot.workspace();
    let package = &workspace.packages["demo"];
    assert_eq!(package.version.as_deref(), Some("1.2.3"));
    for name in [
        "demo::Builder",
        "demo::Builder::setWhitePoint",
        "demo::Matrix",
        "demo::Space",
        "demo::Space::RGB",
        "demo::Palette",
        "demo::count",
    ] {
        assert!(
            resolve_item(workspace, Some("demo"), name).is_ok(),
            "missing {name}"
        );
    }
    assert!(
        package
            .items
            .values()
            .all(|item| !item.qualified_name.contains("private_value")
                && !item.qualified_name.contains("::Mode")
                && !item.qualified_name.contains("::detail"))
    );
    let family = resolve_item(workspace, None, "demo::Builder::setWhitePoint").unwrap();
    let Some(ItemLanguageData::Cpp(data)) = &package.items[&family.item].language_data else {
        panic!("C++ family");
    };
    assert_eq!(data.overloads.len(), 2);
    for overload in &data.overloads {
        let item = &package.items[&overload.item];
        assert_eq!(item.signatures.len(), 1);
        assert!(
            item.documentation
                .as_ref()
                .unwrap()
                .raw_source
                .as_ref()
                .unwrap()
                .contains("chaining")
        );
    }
    assert!(package.items.values().any(|item| matches!(&item.language_data, Some(ItemLanguageData::Cpp(data)) if data.protection == CppProtection::Protected)));
    assert!(package.items.values().any(|item| matches!(&item.language_data, Some(ItemLanguageData::Cpp(data)) if data.declaration == CppDeclarationKind::Constructor)));
    let json = serde_json::to_string(workspace).unwrap();
    assert!(!json.contains(root.path().to_str().unwrap()));
    assert!(!json.contains("/nix/store"));
    assert!(!json.contains("classdemo_1_1Builder"));

    let database = root.path().join(".diplodocus/documentation.sqlite");
    let copy = support::TestWorkspace::new();
    copy.write("documentation.sqlite", std::fs::read(&database).unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["generate", "--input"])
        .arg(copy.path().join("documentation.sqlite"))
        .arg("--output")
        .arg(copy.path().join("site"))
        .env("PATH", copy.path().join("no-tools"))
        .output()
        .unwrap();
    successful(output);
    let builder = resolve_item(workspace, Some("demo"), "demo::Builder").unwrap();
    let digest = diplodocus::provenance::fingerprint_bytes(builder.item.as_bytes()).value;
    let html = copy.read(format!("site/packages/cpp/reference/{digest}.html"));
    let markdown = copy.read(format!("site/packages/cpp/reference/{digest}.md"));
    assert!(html.contains("setWhitePoint"));
    assert!(html.contains("Standard white point index"));
    assert!(
        markdown.contains("Builder & setWhitePoint(int point)"),
        "{markdown}"
    );
    assert!(
        markdown.contains("Custom white point coordinates")
            || markdown.contains("custom white point coordinates")
    );
    assert!(markdown.contains("Throws"));
    assert!(
        markdown.contains("static constexpr int limit = 256"),
        "{markdown}"
    );
    assert!(markdown.contains("```cpp"));
    assert!(!html.contains("private_value"));
    assert!(
        copy.read("site/assets/search.json")
            .contains("demo::Builder::setWhitePoint")
    );
    let detached = Snapshot::load(copy.path().join("documentation.sqlite")).unwrap();
    let fragments =
        diplodocus::rendering::render_fragments(&diplodocus::site::Site::new(&detached).unwrap())
            .unwrap();
    let index: diplodocus::rendering::FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let builder = index
        .documents
        .iter()
        .find(|document| document.qualified_name.as_deref() == Some("demo::Builder"))
        .unwrap();
    assert!(builder.identities.iter().any(|identity| matches!(identity, diplodocus::validation::DocumentIdentity::Item { item } if item == &family)));
    let fragment = std::str::from_utf8(&fragments.files()[&builder.path].bytes).unwrap();
    assert!(fragment.contains("Standard white point index"));
    assert!(!fragment.contains("<!doctype"));
}

#[test]
fn cpp_ids_survive_source_shifts_parameter_names_defaults_and_order() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    successful(command(&root, "extract"));
    let before = snapshot(&root).workspace().packages["demo"]
        .items
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let default = "  /** @brief Construct a default builder. */\n  Builder() = default;";
    let sized = "  /** @brief Construct a builder with a size. @param size Palette size. */\n  explicit Builder(int size);";
    let original = root.read("include/demo.hpp");
    let reordered = original.replace(
        &format!("{default}\n{sized}"),
        &format!("{sized}\n{default}"),
    );
    assert_ne!(original, reordered);
    root.write(
        "include/demo.hpp",
        format!(
            "\n\n{}",
            reordered
                .replace("int point", "int white_index = 1")
                .replace("@param point", "@param white_index")
        ),
    );
    successful(command(&root, "extract"));
    let after = snapshot(&root).workspace().packages["demo"]
        .items
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(before, after);
}

#[cfg(unix)]
#[test]
fn generated_doxyfile_accepts_dot_pages_and_preserves_native_diagnostics() {
    use std::os::unix::fs::PermissionsExt;

    let root = support::TestWorkspace::from_fixture("doxygen-api");
    root.write(
        "build/Doxyfile.docs",
        format!(
            "{}\nINPUT += docs\nHAVE_DOT = YES\nDOT_PATH = tools\n\
             GENERATE_HTML = YES\nGENERATE_LATEX = YES\n\
             OUTPUT_DIRECTORY = build/docs\nWARN_LOGFILE = build/warnings.log\n",
            root.read("Doxyfile")
        ),
    );
    root.remove("Doxyfile");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml")
            .replace("Doxyfile", "build/Doxyfile.docs"),
    );
    root.write(
        "docs/colors.md",
        "# Colors {#colors}\n\nColor conversions.\n\n\
         @dot\ndigraph ColorSpaces { RGB -> XYZ; }\n@enddot\n",
    );
    root.write(
        "tools/dot",
        "#!/bin/sh\necho invoked > dot-invoked\nexit 1\n",
    );
    std::fs::set_permissions(
        root.path().join("tools/dot"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    successful(command(&root, "check"));
    successful(command(&root, "extract"));
    assert!(resolve_item(snapshot(&root).workspace(), Some("demo"), "demo::Builder").is_ok());
    assert!(!root.path().join("dot-invoked").exists());
    assert!(!root.path().join("build/docs").exists());
    assert!(!root.path().join("build/warnings.log").exists());

    let database = root.path().join(".diplodocus/documentation.sqlite");
    let previous = std::fs::read(&database).unwrap();
    root.write(
        "docs/colors.md",
        root.read("docs/colors.md").replace(
            "Color conversions.",
            "Color conversions. See @ref MissingType.",
        ),
    );
    for operation in ["check", "extract"] {
        let failed = command(&root, operation);
        assert!(!failed.status.success());
        let errors = String::from_utf8_lossy(&failed.stderr);
        assert!(errors.contains("Doxygen failed"), "{errors}");
        assert!(errors.contains("MissingType"), "{errors}");
        assert_eq!(std::fs::read(&database).unwrap(), previous);
    }

    root.write(
        "build/Doxyfile.docs",
        root.read("build/Doxyfile.docs")
            .replace("WARN_AS_ERROR = YES", "WARN_AS_ERROR = NO"),
    );
    successful(command(&root, "extract"));
    assert!(
        snapshot(&root)
            .workspace()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == diplodocus::diagnostics::DiagnosticCode::DoxygenUnsupported
                    && diagnostic.severity == diplodocus::diagnostics::Severity::Warning
                    && diagnostic.message.contains("MissingType")
                    && diagnostic.message.contains("<repository>/docs/colors.md")
            })
    );
    assert!(!root.path().join("dot-invoked").exists());
}

#[test]
fn native_failure_and_missing_doxygen_preserve_previous_snapshot() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    successful(command(&root, "extract"));
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let previous = std::fs::read(&database).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["extract", "--config"])
        .arg(root.path().join("diplodocus.yaml"))
        .env("PATH", root.path().join("missing-tools"))
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Doxygen could not start"));
    root.write(
        "include/demo.hpp",
        root.read("include/demo.hpp").replace(
            "Available palette spaces.",
            "Available palette spaces. See @ref MissingType.",
        ),
    );
    let failed = command(&root, "extract");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Doxygen failed"));
    assert_eq!(std::fs::read(&database).unwrap(), previous);
    root.write(
        "Doxyfile",
        format!("{}\nINPUT = missing-headers\n", root.read("Doxyfile")),
    );
    assert!(!command(&root, "extract").status.success());
    assert_eq!(std::fs::read(database).unwrap(), previous);
}

#[test]
fn revalidation_observes_added_sources_and_included_doxyfiles() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    let sources =
        diplodocus::assembly::assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.write(
        "include/added.hpp",
        "/** Another declaration. */\nstruct Added {};\n",
    );
    assert!(sources.revalidate().is_err());
    let sources =
        diplodocus::assembly::assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    root.write(
        "config/project.doxy",
        root.read("config/project.doxy").replace("1.2.3", "1.2.4"),
    );
    assert!(sources.revalidate().is_err());
}

#[test]
fn private_extraction_requires_an_explicit_native_setting() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    root.write(
        "Doxyfile",
        root.read("Doxyfile")
            .replace("EXTRACT_PRIVATE = NO", "EXTRACT_PRIVATE = YES"),
    );
    successful(command(&root, "extract"));
    assert!(
        snapshot(&root).workspace().packages["demo"]
            .items
            .values()
            .any(|item| item.name == "private_value")
    );
}

#[test]
fn malformed_xml_reports_an_extraction_diagnostic() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    let path = root.path().join("diplodocus.yaml");
    let configuration = diplodocus::configuration::load_configuration(&path).unwrap();
    let paths = diplodocus::paths::resolve_workspace_paths(&path, &configuration).unwrap();
    for index in [
        "<broken",
        "<unrelated/>",
        "<doxygenindex><compound kind=\"class\" refid=\"missing\"/></doxygenindex>",
    ] {
        let result = diplodocus::extractors::doxygen::extract_doxygen(
            &paths.repositories[0],
            &paths.packages[0],
            &paths.packages[0].targets[0],
            &std::collections::BTreeMap::from([("index.xml".into(), index.into())]),
            &Default::default(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == diplodocus::diagnostics::Severity::Error)
        );
    }
}

#[test]
fn included_doxyfiles_inherit_the_native_include_search_path() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    root.write("config/outer.doxy", "@INCLUDE = project.doxy\n");
    root.write(
        "Doxyfile",
        format!(
            "@INCLUDE_PATH = config\r\n@INCLUDE = outer.doxy\r\n{}",
            root.read("Doxyfile")
        ),
    );
    successful(command(&root, "extract"));
    assert_eq!(
        snapshot(&root).workspace().packages["demo"]
            .version
            .as_deref(),
        Some("1.2.3")
    );
}
