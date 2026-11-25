use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(root: &Path, arguments: &[&str], detached: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    command.current_dir(root).args(arguments);
    if detached {
        command.env("PATH", "");
    }
    command.output().unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn failure(output: Output, message: &str) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
}

fn producer(root: &Path, id: &str, repo: &str, slug: &str, overview: &str) {
    let directory = root.join(id);
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("manifest"), "").unwrap();
    fs::write(directory.join("README.md"), overview).unwrap();
    fs::write(directory.join("asset.bin"), b"retained download").unwrap();
    fs::write(
        root.join(format!("{id}.yaml")),
        format!(
            "project: {{name: Test}}
repositories: [{{id: {repo}, path: {id}}}]
packages:
  - id: {id}
    name: {id}
    slug: {slug}
    ecosystem: cpp
    repository: {repo}
    path: .
    metadata-path: manifest
    targets: []
"
        ),
    )
    .unwrap();
}

fn export(root: &Path, id: &str) {
    success(cli(
        root,
        &[
            "bundle",
            "export",
            "--config",
            &format!("{id}.yaml"),
            "--output",
            &format!("{id}.sqlite"),
        ],
        true,
    ));
}

#[test]
fn documenter_execution_requires_local_package_owner() {
    use diplodocus::configuration::parse_configuration;
    use diplodocus::configuration_validation::validate_configuration;

    let yaml = "project: {name: Test}
repositories:
  - {id: source, path: .}
package-bundles:
  - {package: example, path: example.sqlite}
content:
  - id: manual
    owner: example
    repository: source
    path: docs
    mount: manual
    format: documenter
";
    assert!(validate_configuration(&parse_configuration(yaml).unwrap()).is_empty());
    let executable = format!(
        "{yaml}    execution: {{mode: execute, engine: julia, julia-project: Project.toml}}\n"
    );
    let diagnostics = validate_configuration(&parse_configuration(&executable).unwrap());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("require a local checkout"));
}

#[test]
fn bundle_storage_and_composition_work_without_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    producer(
        root,
        "example",
        "upstream",
        "example",
        "# Example\n\n[Download](asset.bin).\n",
    );
    producer(root, "conflict", "other", "example", "# Conflict\n");
    producer(
        root,
        "missing",
        "unresolved",
        "missing",
        "# Missing\n\n[`dependency::api`]\n",
    );
    for id in ["example", "conflict", "missing"] {
        export(root, id);
        fs::remove_dir_all(root.join(id)).unwrap();
    }

    let configuration = root.join("diplodocus.yaml");
    let valid =
        "project: {name: Test}\npackage-bundles:\n  - {package: example, path: example.sqlite}\n";
    fs::write(&configuration, valid).unwrap();
    let extract = ["extract", "--output", "documentation.sqlite"];
    success(cli(root, &extract, true));
    let completed = fs::read(root.join("documentation.sqlite")).unwrap();
    let generate = |input| {
        cli(
            root,
            &["generate", "--input", input, "--output", "site"],
            true,
        )
    };
    failure(
        generate("example.sqlite"),
        "expected a completed workspace snapshot",
    );

    let check = |extra: &str| {
        fs::write(&configuration, format!("{valid}{extra}")).unwrap();
        cli(root, &extract, true)
    };
    failure(
        check("  - {package: example, path: conflict.sqlite}\n"),
        "duplicate-package-id",
    );
    fs::write(&configuration, valid).unwrap();
    failure(
        cli(root, &["extract", "--output", "example.sqlite"], true),
        "overlaps",
    );
    failure(
        check("  - {package: conflict, path: conflict.sqlite}\n"),
        "duplicate package slug",
    );
    failure(
        check("  - {package: missing, path: missing.sqlite}\n"),
        "dependency::api",
    );
    failure(
        check("  - {package: bad, path: documentation.sqlite}\n"),
        "expected a package API bundle",
    );
    fs::write(root.join("bad.sqlite"), b"corrupt").unwrap();
    failure(
        check("  - {package: bad, path: bad.sqlite}\n"),
        "bad.sqlite",
    );
    fs::copy(root.join("example.sqlite"), root.join("future.sqlite")).unwrap();
    rusqlite::Connection::open(root.join("future.sqlite"))
        .unwrap()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    failure(
        check("  - {package: future, path: future.sqlite}\n"),
        "unsupported package API bundle version",
    );
    assert_eq!(
        completed,
        fs::read(root.join("documentation.sqlite")).unwrap()
    );

    fs::remove_file(root.join("example.sqlite")).unwrap();
    success(generate("documentation.sqlite"));
    let snapshot =
        diplodocus::snapshots::Snapshot::load(root.join("documentation.sqlite")).unwrap();
    assert!(
        snapshot
            .assets()
            .values()
            .any(|asset| asset.bytes == b"retained download")
    );
}

#[test]
fn pinned_release_cache_and_local_override_keep_the_mount() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let revision = "a".repeat(40);
    producer(
        root,
        "example",
        "upstream",
        "example",
        "# Example\n\nPublished bundle body.\n",
    );
    let producer_config = root.join("example.yaml");
    let local = fs::read_to_string(&producer_config).unwrap().replace(
        "path: example}",
        &format!("path: example, revision: {revision}}}"),
    );
    fs::write(&producer_config, &local).unwrap();
    export(root, "example");
    let bytes = fs::read(root.join("example.sqlite")).unwrap();
    let digest = diplodocus::provenance::fingerprint_bytes(&bytes).value;
    let cache = root.join(".diplodocus/cache/package-bundles/sha256");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(format!("{digest}.sqlite")), bytes).unwrap();

    let remote = format!(
        "package-bundles:\n  - package: example\n    mount: api/example\n    github-release: {{repository: example/source, tag: v1, asset: example.sqlite, revision: {revision}, sha256: '{digest}'}}\n"
    );
    let config = root.join("diplodocus.yaml");
    fs::write(&config, format!("project: {{name: Test}}\n{remote}")).unwrap();
    success(cli(
        root,
        &["extract", "--output", "published.sqlite"],
        true,
    ));
    producer(root, "other", "other-source", "other", "# Other package\n");
    let other = fs::read_to_string(root.join("other.yaml")).unwrap();
    fs::write(&config, format!("{other}{remote}")).unwrap();
    success(cli(root, &["build", "--output", "mixed"], true));
    let home = fs::read_to_string(root.join("mixed/index.html")).unwrap();
    assert!(home.contains("api/example/index.html"));
    assert!(home.contains("packages/other/index.html"));
    fs::write(
        &config,
        format!(
            "{}{}",
            other.replace("slug: other", "slug: collision"),
            remote.replace("api/example", "packages/collision")
        ),
    )
    .unwrap();
    failure(
        cli(root, &["check"], true),
        "site route is invalid or conflicts",
    );

    fs::remove_dir_all(&cache).unwrap();
    fs::write(
        root.join("example/README.md"),
        "# Example\n\nLocal checkout body.\n",
    )
    .unwrap();
    fs::write(
        &config,
        format!("{local}{remote}    local-override: true\n"),
    )
    .unwrap();
    success(cli(root, &["extract", "--output", "local.sqlite"], true));
    assert!(!cache.exists());
    fs::remove_file(config).unwrap();
    fs::remove_dir_all(root.join("example")).unwrap();
    for (name, title) in [
        ("published", "Published bundle body"),
        ("local", "Local checkout body"),
    ] {
        success(cli(
            root,
            &[
                "generate",
                "--input",
                &format!("{name}.sqlite"),
                "--output",
                name,
            ],
            true,
        ));
        let page = fs::read_to_string(root.join(name).join("api/example/index.html")).unwrap();
        assert!(page.contains(title), "missing {title} in {name}");
    }
}

#[test]
fn bundles_exported_from_implicit_checkouts_can_be_combined() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    for package in ["one", "two"] {
        fs::write(
            root.join(format!("{package}.yaml")),
            format!(
                "project: {{name: Test}}
packages:
  - id: {package}
    name: {package}
    slug: {package}
    ecosystem: cpp
    path: {package}
    metadata-path: manifest
    targets: []
"
            ),
        )
        .unwrap();
        fs::create_dir(root.join(package)).unwrap();
        fs::write(root.join(package).join("manifest"), "").unwrap();
        export(root, package);
    }
    fs::write(
        root.join("diplodocus.yaml"),
        "project: {name: Test}
package-bundles:
  - {package: one, path: one.sqlite}
  - {package: two, path: two.sqlite}
",
    )
    .unwrap();
    success(cli(root, &["build", "--output", "site"], true));
    for package in ["one", "two"] {
        assert!(
            root.join(format!("site/packages/{package}/index.html"))
                .is_file()
        );
    }
}
