mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use diplodocus::documents::{AuthoredFormat, parse_authored_document};
use support::{AcceptanceRegistry, TestWorkspace};

#[test]
fn baseline_authored_pages_have_no_parser_diagnostics() {
    let workspace = support::acceptance_workspace();
    let declared = support::authored_sources(&workspace, "workspace/diplodocus.toml")
        .into_iter()
        .map(|(path, _)| path)
        .collect::<BTreeSet<_>>();
    for path in support::fixture_files("acceptance") {
        if path == std::path::Path::new("MATRIX.md") {
            continue;
        }
        let format = match path.extension().and_then(|extension| extension.to_str()) {
            Some("md") => AuthoredFormat::Gfm,
            Some("qmd") => AuthoredFormat::Qmd,
            _ => continue,
        };
        assert!(
            declared.contains(&path),
            "unmounted authored fixture: {}",
            path.display()
        );
        let source = support::load_fixture(std::path::Path::new("acceptance").join(&path));
        let parsed = parse_authored_document(&source, format);
        assert!(
            parsed.diagnostics.is_empty(),
            "{}: {:?}",
            path.display(),
            parsed.diagnostics
        );
    }
}

#[test]
fn acceptance_registry_covers_all_mvp_criteria() {
    validate_registry(&support::acceptance_registry());
}

fn validate_registry(registry: &AcceptanceRegistry) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roadmap = fs::read_to_string(root.join("TODO.md")).unwrap();
    let matrix = support::load_fixture("acceptance/MATRIX.md");
    assert_eq!(registry.criteria.len(), 11);
    let cases = registry
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(cases.len(), registry.cases.len());
    let scenarios = registry
        .scenarios
        .iter()
        .map(|scenario| scenario.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(scenarios.len(), registry.scenarios.len());
    let mut covered_scenarios = BTreeSet::new();
    for number in 1..=11 {
        let id = format!("MVP-{number:02}");
        assert!(roadmap.contains(&format!("**{id}:**")));
        assert!(matrix.contains(&format!("| `{id}`:")));
        let ids = &registry.criteria[&id];
        assert!(!ids.is_empty(), "uncovered {id}");
        for scenario in ids {
            assert!(
                scenarios.contains(scenario.as_str()),
                "unknown scenario {scenario}"
            );
            covered_scenarios.insert(scenario.as_str());
        }
    }
    assert_eq!(covered_scenarios, scenarios);
    let mut covered_cases = BTreeSet::new();
    for scenario in &registry.scenarios {
        assert!(!scenario.inputs.is_empty());
        assert!(!scenario.action.is_empty());
        assert!(!scenario.expected.is_empty());
        assert!((1..=11).contains(&scenario.milestone));
        assert!(matrix.contains(&format!("| `{}` |", scenario.id)));
        assert!(matrix.contains(&scenario.action));
        assert!(matrix.contains(&scenario.expected));
        for input in &scenario.inputs {
            assert!(support::is_relative_input(input));
            assert!(
                root.join(input).is_file(),
                "missing scenario input: {input}"
            );
        }
        for case in &scenario.cases {
            assert!(cases.contains(case.as_str()));
            covered_cases.insert(case.as_str());
        }
    }
    assert_eq!(covered_cases, cases);
    let mut overlay_files = BTreeSet::new();
    for case in &registry.cases {
        assert!(support::is_relative_input(&case.config));
        assert!(matrix.contains(&format!("| `{}` |", case.id)));
        assert!(case.check.len() <= 1 && case.build.len() <= 1);
        for diagnostic in case.check.iter().chain(&case.build) {
            assert!(matches!(diagnostic.severity.as_str(), "warning" | "error"));
            assert!(support::is_relative_input(&diagnostic.source));
            assert!(matrix.contains(&format!("`{}`", diagnostic.code)));
        }
        if let Some(overlay) = &case.overlay {
            assert_eq!(case.changes.len(), 1);
            assert_eq!(case.build.len(), 1);
            assert_eq!(case.build[0].source, case.changes[0]);
            for path in &case.changes {
                let relative = Path::new(overlay).join(path);
                assert!(matrix.contains(&format!("`../acceptance-cases/{}`", relative.display())));
                overlay_files.insert(relative);
            }
        }
    }
    assert_eq!(
        overlay_files,
        support::fixture_files("acceptance-cases")
            .into_iter()
            .collect()
    );
}

#[test]
fn registry_rejects_missing_coverage_and_unreachable_inputs() {
    let mut registry = support::acceptance_registry();
    registry.criteria.remove("MVP-10");
    assert!(std::panic::catch_unwind(|| validate_registry(&registry)).is_err());

    let mut registry = support::acceptance_registry();
    registry.scenarios[0]
        .inputs
        .push("missing-fixture.toml".into());
    assert!(std::panic::catch_unwind(|| validate_registry(&registry)).is_err());
}

fn file_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    support::files_under(root)
        .into_iter()
        .map(|path| {
            let bytes = fs::read(root.join(&path)).unwrap();
            (path, bytes)
        })
        .collect()
}

#[test]
fn cases_change_only_their_declared_inputs() {
    let baseline = file_bytes(&support::fixture_path("acceptance"));
    let overlays = file_bytes(&support::fixture_path("acceptance-cases"));
    for case in support::acceptance_registry().cases {
        let workspace = support::materialize_case(&case);
        let materialized = file_bytes(workspace.path());
        let paths = baseline
            .keys()
            .chain(materialized.keys())
            .collect::<BTreeSet<_>>();
        let changed = paths
            .into_iter()
            .filter(|path| baseline.get(*path) != materialized.get(*path))
            .map(|path| path.to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(changed, case.changes, "unexpected changes for {}", case.id);
        for diagnostic in case.check.iter().chain(&case.build) {
            assert!(workspace.path().join(&diagnostic.source).is_file());
        }
        validate_configuration(&workspace, &case.config);
        let authored = support::authored_sources(&workspace, &case.config);
        for path in &case.changes {
            let path = Path::new(path);
            if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("md" | "qmd")
            ) {
                assert!(
                    authored.iter().any(|(source, _)| source == path),
                    "unmounted diagnostic case: {}",
                    case.id
                );
            }
        }
    }
    assert_eq!(baseline, file_bytes(&support::fixture_path("acceptance")));
    assert_eq!(
        overlays,
        file_bytes(&support::fixture_path("acceptance-cases"))
    );
}

#[test]
fn overlays_reject_traversal_and_symlink_escapes() {
    let workspace = TestWorkspace::new();
    let overlay = TestWorkspace::new();
    overlay.write("safe.txt", "input");
    for relative in ["../safe.txt", "/safe.txt"] {
        assert!(
            std::panic::catch_unwind(|| support::copy_overlay_file(
                &workspace,
                overlay.path(),
                Path::new(relative)
            ))
            .is_err()
        );
    }
    #[cfg(unix)]
    {
        let outside = TestWorkspace::new();
        outside.write("safe.txt", "outside");
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape")).unwrap();
        overlay.write("escape/safe.txt", "replacement");
        assert!(
            std::panic::catch_unwind(|| support::copy_overlay_file(
                &workspace,
                overlay.path(),
                Path::new("escape/safe.txt")
            ))
            .is_err()
        );
        std::os::unix::fs::symlink(
            outside.path().join("safe.txt"),
            overlay.path().join("link.txt"),
        )
        .unwrap();
        assert!(
            std::panic::catch_unwind(|| support::copy_overlay_file(
                &workspace,
                overlay.path(),
                Path::new("link.txt")
            ))
            .is_err()
        );
        assert_eq!(outside.read("safe.txt"), "outside");
        std::os::unix::fs::symlink(
            outside.path().join("missing.txt"),
            workspace.path().join("safe.txt"),
        )
        .unwrap();
        assert!(
            std::panic::catch_unwind(|| support::copy_overlay_file(
                &workspace,
                overlay.path(),
                Path::new("safe.txt")
            ))
            .is_err()
        );
        assert!(!outside.path().join("missing.txt").exists());
    }
}

fn validate_configuration(workspace: &TestWorkspace, config: &str) {
    let value = support::fixture_configuration(workspace, config);
    let root = workspace.path().canonicalize().unwrap();
    let config_dir = workspace.path().join(config).parent().unwrap().to_owned();
    let mut repositories = BTreeMap::new();
    for repository in value["repository"].as_array().unwrap() {
        let path = config_dir
            .join(repository["path"].as_str().unwrap())
            .canonicalize()
            .unwrap();
        assert!(path.starts_with(&root));
        assert!(
            repositories
                .insert(repository["id"].as_str().unwrap(), path)
                .is_none()
        );
        assert!(repository["url"].as_str().unwrap().starts_with("https://"));
    }
    let mut owners = BTreeSet::from(["project"]);
    if let Some(packages) = value.get("package").and_then(toml::Value::as_array) {
        for package in packages {
            assert!(owners.insert(package["id"].as_str().unwrap()));
            let repository = &repositories[package["repository"].as_str().unwrap()];
            let path = repository
                .join(package["path"].as_str().unwrap())
                .canonicalize()
                .unwrap();
            assert!(path.starts_with(repository));
            let metadata = path
                .join(package["metadata_path"].as_str().unwrap())
                .canonicalize()
                .unwrap();
            assert!(metadata.is_file() && metadata.starts_with(repository));
            for target in package["targets"].as_array().unwrap() {
                assert!(
                    path.join(target["path"].as_str().unwrap())
                        .canonicalize()
                        .unwrap()
                        .starts_with(&path)
                );
            }
        }
    }
    if let Some(collections) = value.get("content").and_then(toml::Value::as_array) {
        let mut ids = BTreeSet::new();
        for collection in collections {
            assert!(ids.insert(collection["id"].as_str().unwrap()));
            assert!(owners.contains(collection["owner"].as_str().unwrap()));
            assert!(support::is_relative_input(
                collection["mount"].as_str().unwrap()
            ));
            let repository = &repositories[collection["repository"].as_str().unwrap()];
            let path = repository
                .join(collection["path"].as_str().unwrap())
                .canonicalize()
                .unwrap();
            assert!(path.is_dir() && path.starts_with(repository));
            assert!(!root.join("site").starts_with(&path));
            assert!(matches!(
                collection["format"].as_str().unwrap(),
                "gfm" | "qmd"
            ));
            if let Some(execution) = collection.get("execution") {
                match execution["mode"].as_str().unwrap() {
                    "never" => {
                        assert!(execution.get("engine").is_none());
                        assert!(execution.get("kernel").is_none());
                    }
                    "execute" => {
                        assert_eq!(collection["format"].as_str(), Some("qmd"));
                        assert_eq!(execution["engine"].as_str(), Some("jupyter"));
                        assert!(matches!(
                            execution["kernel"].as_str(),
                            Some("python3" | "ir")
                        ));
                        for input in execution["declared_environment_inputs"].as_array().unwrap() {
                            let path = repository
                                .join(input.as_str().unwrap())
                                .canonicalize()
                                .unwrap();
                            assert!(path.is_file() && path.starts_with(repository));
                        }
                    }
                    mode => panic!("unsupported fixture execution mode {mode}"),
                }
            }
        }
    }
}

#[test]
fn own_documentation_has_a_content_only_configuration() {
    let workspace = support::own_documentation_workspace();
    validate_configuration(&workspace, "diplodocus.toml");
    let config = support::fixture_configuration(&workspace, "diplodocus.toml");
    assert!(config.get("package").is_none());
    assert_eq!(config["repository"].as_array().unwrap().len(), 1);
    assert_eq!(config["content"].as_array().unwrap().len(), 2);
    assert!(
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".gitignore"))
            .unwrap()
            .lines()
            .any(|line| line == "/site/")
    );
}

#[test]
fn cases_have_exactly_the_expected_authored_parser_diagnostics() {
    for case in support::acceptance_registry().cases {
        let workspace = support::materialize_case(&case);
        let mut observed = Vec::new();
        for (path, profile) in support::authored_sources(&workspace, &case.config) {
            let parsed = parse_authored_document(&workspace.read(&path), format(&profile));
            for diagnostic in parsed.diagnostics {
                observed.push(support::ExpectedDiagnostic {
                    code: serde_json::to_value(diagnostic.code)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .into(),
                    severity: serde_json::to_value(diagnostic.severity)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .into(),
                    source: path.to_str().unwrap().replace('\\', "/"),
                });
            }
        }
        // Semantic extraction and execution policy are verified in later milestones.
        let expected = case
            .check
            .into_iter()
            .filter(|diagnostic| diagnostic.code == "unsupported-authored-syntax")
            .collect::<Vec<_>>();
        assert_eq!(
            observed, expected,
            "unexpected parser diagnostic for {}",
            case.id
        );
    }
}

fn format(profile: &str) -> AuthoredFormat {
    match profile {
        "gfm" => AuthoredFormat::Gfm,
        "qmd" => AuthoredFormat::Qmd,
        _ => panic!("unknown authored profile"),
    }
}

#[test]
fn real_documentation_matches_authored_goldens() {
    let workspace = support::own_documentation_workspace();
    for (path, profile) in support::authored_sources(&workspace, "diplodocus.toml") {
        let parsed = parse_authored_document(&workspace.read(&path), format(&profile));
        assert!(
            parsed.diagnostics.is_empty(),
            "{}: {:?}",
            path.display(),
            parsed.diagnostics
        );
        support::assert_json_golden(&parsed, format!("dogfood/{}.json", path.display()));
    }
}

fn link_targets(value: &serde_json::Value) -> Vec<&str> {
    let mut targets = Vec::new();
    match value {
        serde_json::Value::Object(fields) => {
            if matches!(
                fields.get("type").and_then(serde_json::Value::as_str),
                Some("link" | "image")
            ) {
                targets.push(fields["target"].as_str().unwrap());
            }
            for child in fields.values() {
                targets.extend(link_targets(child));
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                targets.extend(link_targets(child));
            }
        }
        _ => {}
    }
    targets
}

fn validate_local_links(workspace: &TestWorkspace, config: &str) {
    for (path, profile) in support::authored_sources(workspace, config) {
        let parsed = parse_authored_document(&workspace.read(&path), format(&profile));
        let value = serde_json::to_value(parsed.document).unwrap();
        for target in link_targets(&value) {
            if target.contains(':') {
                continue;
            }
            let target = target.split('#').next().unwrap();
            let resolved = workspace
                .path()
                .join(path.parent().unwrap())
                .join(target)
                .canonicalize()
                .unwrap_or_else(|error| panic!("{} -> {target}: {error}", path.display()));
            assert!(
                resolved.is_file()
                    && resolved.starts_with(workspace.path().canonicalize().unwrap())
            );
        }
    }
}

#[test]
fn baseline_and_real_documentation_have_resolvable_local_links_and_assets() {
    validate_local_links(
        &support::acceptance_workspace(),
        "workspace/diplodocus.toml",
    );
    validate_local_links(&support::own_documentation_workspace(), "diplodocus.toml");
}

#[test]
fn missing_authored_assets_fail_the_corpus_check() {
    let workspace = support::own_documentation_workspace();
    workspace.remove("docs/guide/assets/workspace.svg");
    assert!(
        std::panic::catch_unwind(|| validate_local_links(&workspace, "diplodocus.toml")).is_err()
    );
}
