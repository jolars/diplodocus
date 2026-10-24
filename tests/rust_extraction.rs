mod support;

use std::collections::BTreeSet;
use std::process::{Command, Output};

use diplodocus::configuration::{RustTargetConfiguration, parse_configuration};
use diplodocus::configuration_validation::validate_configuration;
use diplodocus::ir::{
    ItemLanguageData, ProvenanceActivity, RustDeclarationKind, RustDoctestStatus,
};
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

#[test]
fn rust_settings_default_to_cargo_defaults_and_reject_misplaced_settings() {
    assert_eq!(
        RustTargetConfiguration::default().features,
        Vec::<String>::new()
    );
    assert!(RustTargetConfiguration::default().default_features);
    let root = support::TestWorkspace::from_fixture("rust-api");
    let text = root.read("diplodocus.yaml");
    let mut configuration = parse_configuration(&text).unwrap();
    assert!(validate_configuration(&configuration).is_empty());
    configuration.packages[0].targets[0].rust = Some(RustTargetConfiguration::default());
    configuration.packages[0].targets[0].extractor = "python".into();
    assert!(!validate_configuration(&configuration).is_empty());
    configuration.packages[0].targets[0].extractor = "rust".into();
    configuration.packages[0].targets[0]
        .rust
        .as_mut()
        .unwrap()
        .features
        .push("extra,other".into());
    assert!(!validate_configuration(&configuration).is_empty());
}

#[test]
fn extracts_compiler_public_surface_and_native_example_outcomes() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    successful(command(&root, "extract"));
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let workspace = snapshot.workspace();
    let package = &workspace.packages["rustdemo"];
    assert_eq!(package.version.as_deref(), Some("0.1.0"));
    let Some(diplodocus::ir::PackageMetadata::Rust(metadata)) = &package.metadata else {
        panic!("Cargo metadata");
    };
    assert_eq!(metadata.name, "rust_demo");
    assert_eq!(metadata.version, "0.1.0");
    assert_eq!(metadata.license.as_deref(), Some("MIT"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("A compiler-backed documentation fixture")
    );
    for name in [
        "rust_demo::Counter",
        "rust_demo::Counter::new",
        "rust_demo::Named",
        "rust_demo::Mode::Fast",
        "rust_demo::Mode::Custom::width",
        "rust_demo::generated",
    ] {
        assert!(
            resolve_item(workspace, Some("rustdemo"), name).is_ok(),
            "missing {name}"
        );
    }
    let counter = resolve_item(workspace, None, "rustdemo::Counter").unwrap();
    assert_eq!(
        resolve_item(workspace, None, "rustdemo::rust_demo::Counter").unwrap(),
        counter
    );
    let counter = &package.items[&counter.item];
    let Some(ItemLanguageData::Rust(data)) = &counter.language_data else {
        panic!("Rust semantics");
    };
    assert_eq!(data.declaration, RustDeclarationKind::Struct);
    assert_eq!(data.public_path, "rust_demo::Counter");
    assert!(data.implementations.iter().any(|implementation| {
        implementation
            .trait_name
            .as_deref()
            .is_some_and(|name| name.ends_with("Named"))
    }));
    assert!(
        data.implementations
            .iter()
            .any(|implementation| implementation.documentation_item.is_some())
    );
    assert!(
        package
            .items
            .values()
            .all(|item| !matches!(item.name.as_str(), "Invisible" | "hidden" | "extra"))
    );
    assert!(
        counter
            .source_location
            .as_ref()
            .is_some_and(|source| source.path.as_str() == "src/model.rs" && source.span.is_some())
    );

    let outcomes = workspace
        .provenance
        .iter()
        .find_map(|record| match &record.activity {
            ProvenanceActivity::RustDoctests { examples, .. } => Some(examples),
            _ => None,
        })
        .unwrap();
    for expected in [
        RustDoctestStatus::Executed,
        RustDoctestStatus::CompileOnly,
        RustDoctestStatus::ExpectedError,
        RustDoctestStatus::ExplicitSkip,
    ] {
        assert!(
            outcomes.iter().any(|example| example.outcome == expected),
            "missing {expected:?}"
        );
    }
    assert!(
        outcomes
            .iter()
            .any(|example| example.name.contains("private_examples"))
    );
    assert!(
        !outcomes
            .iter()
            .any(|example| example.code.contains("illustrative prose"))
    );
    let json = serde_json::to_string(workspace).unwrap();
    assert!(!json.contains(root.path().to_str().unwrap()));
    assert!(!json.contains("/nix/store"));
}

#[test]
fn explicit_features_select_optional_api_and_ids_survive_source_shifts() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    successful(command(&root, "extract"));
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let before = Snapshot::load(&database).unwrap();
    let before_ids: BTreeSet<_> = before.workspace().packages["rustdemo"]
        .items
        .keys()
        .cloned()
        .collect();
    root.write("src/model.rs", format!("\n\n{}", root.read("src/model.rs")));
    successful(command(&root, "extract"));
    let after = Snapshot::load(&database).unwrap();
    assert_eq!(
        before_ids,
        after.workspace().packages["rustdemo"]
            .items
            .keys()
            .cloned()
            .collect()
    );

    let mut configuration: serde_json::Value =
        serde_saphyr::from_str(&root.read("diplodocus.yaml")).unwrap();
    configuration["packages"][0]["targets"][0]
        .as_object_mut()
        .unwrap()
        .insert(
            "rust".into(),
            serde_json::Value::Object(serde_json::Map::from_iter([(
                "features".into(),
                serde_json::Value::Array(vec!["extra".into()]),
            )])),
        );
    root.write(
        "diplodocus.yaml",
        serde_saphyr::to_string(&configuration).unwrap(),
    );
    successful(command(&root, "extract"));
    let extra = Snapshot::load(&database).unwrap();
    assert!(resolve_item(extra.workspace(), Some("rustdemo"), "rust_demo::extra").is_ok());
}

#[test]
fn check_does_not_execute_examples_and_failure_preserves_previous_snapshot() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    successful(command(&root, "extract"));
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let previous = std::fs::read(&database).unwrap();
    root.write(
        "src/lib.rs",
        root.read("src/lib.rs").replace(
            "assert_eq!(counter.value, 3)",
            "assert_eq!(counter.value, 999)",
        ),
    );
    successful(command(&root, "check"));
    let failed = command(&root, "extract");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("doctest"));
    assert_eq!(std::fs::read(&database).unwrap(), previous);
}

#[test]
fn missing_toolchain_is_an_actionable_failure() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    let failed = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["extract", "--config"])
        .arg(root.path().join("diplodocus.yaml"))
        .env(
            "DIPLODOCUS_RUST_TOOLCHAIN",
            root.path().join("missing-toolchain"),
        )
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let diagnostic = String::from_utf8_lossy(&failed.stderr);
    assert!(diagnostic.contains("rust-toolchain"), "{diagnostic}");
    assert!(
        !root
            .path()
            .join(".diplodocus/documentation.sqlite")
            .exists()
    );
}

#[test]
fn snapshot_publication_cannot_replace_rust_compilation_storage() {
    let root = support::TestWorkspace::from_fixture("rust-api");
    let output = root
        .path()
        .join(".diplodocus/cache/rust/documentation.sqlite");
    let failed = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .args(["extract", "--config"])
        .arg(root.path().join("diplodocus.yaml"))
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(!output.exists());
}
