use diplodocus::execution::identity::{CanonicalValue, content_digest, domain_digest};

#[path = "../src/execution/identity/build_metadata.rs"]
mod build_metadata;

#[test]
fn build_versions_follow_selected_edges_and_reject_ambiguity() {
    let lock = include_str!("../Cargo.lock");
    assert_eq!(
        build_metadata::selected_version(lock, env!("CARGO_PKG_VERSION"), "toml").unwrap(),
        "1.1.6+spec-1.1.0"
    );
    let ambiguous = lock.replace("\"toml 1.1.6+spec-1.1.0\"", "\"toml\"");
    assert!(
        build_metadata::selected_version(&ambiguous, env!("CARGO_PKG_VERSION"), "toml").is_err()
    );
    assert!(build_metadata::selected_version(lock, env!("CARGO_PKG_VERSION"), "missing").is_err());
    let components = build_metadata::components(lock, env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(components.len(), 18);
    assert!(components.windows(2).all(|pair| pair[0].0 < pair[1].0));
}

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../docs/spikes/fixtures/execution-cache-key-v1.json"
    ))
    .unwrap()
}

#[test]
fn immutable_encoding_and_key_vectors() {
    let fixture = fixture();
    let value = CanonicalValue::from_json(fixture["key_input"].clone()).unwrap();
    assert_eq!(
        value.encode().unwrap(),
        fixture["canonical_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        domain_digest("diplodocus/page-execution-key-v1", &value).unwrap(),
        fixture["page_key"]
    );
    let value = CanonicalValue::from_json(fixture["encoding_vector"]["input"].clone()).unwrap();
    let bytes = value.encode().unwrap();
    assert_eq!(
        bytes,
        fixture["encoding_vector"]["canonical_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(content_digest(&bytes), fixture["encoding_vector"]["sha256"]);
    assert_eq!(CanonicalValue::decode(&bytes).unwrap(), value);
    for (field, domain, expected) in [
        (
            "private_spec",
            "diplodocus/execution-kernelspec-v1",
            "spec_digest",
        ),
        (
            "private_launch",
            "diplodocus/execution-launch-v1",
            "launch_digest",
        ),
    ] {
        let value = CanonicalValue::from_json(fixture[field].clone()).unwrap();
        assert_eq!(
            domain_digest(domain, &value).unwrap(),
            fixture["key_input"]["kernel"][expected]
        );
    }
}

#[test]
fn rejects_invalid_or_noncanonical_encoding_at_every_depth() {
    for bytes in [
        b"{\"a\":1,\"a\":2}".as_slice(),
        b"{\"nested\":{\"a\":1,\"a\":2}}",
        b"-0",
        b"-1",
        b"1.0",
        b"1e0",
        b"01",
        b"18446744073709551616",
        b"NaN",
        b"\"\\ud800\"",
        b"\"\xff\"",
        b"\xef\xbb\xbfnull",
        b"{\"\\u00e9\":0}",
        b"{\"b\":0,\"a\":0}",
        b"null\n",
        b" true",
        b"\"\\n\"",
        b"\"\\u0041\"",
        b"\"\\/\"",
        b"[null,]",
    ] {
        assert!(CanonicalValue::decode(bytes).is_err(), "{bytes:?}");
    }
}

#[test]
fn closed_objects_reject_missing_and_unknown_fields() {
    let value = CanonicalValue::decode(b"{\"a\":null,\"b\":true}").unwrap();
    assert!(value.fields(&["a", "b"]).is_ok());
    assert!(value.fields(&["a"]).is_err());
    assert!(value.fields(&["a", "b", "c"]).is_err());
}

mod snapshots {
    use super::*;
    use diplodocus::configuration::ContentConfiguration;
    use diplodocus::documents::prepare_collection_document;
    use diplodocus::execution::identity::*;
    use diplodocus::execution::*;
    use diplodocus::ir::{InputFingerprint, SourceLocation};
    use diplodocus::provenance::fingerprint_bytes;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::Path;

    pub struct Setup {
        pub directory: tempfile::TempDir,
        pub source: Vec<u8>,
        pub request: PageExecutionRequest,
        pub inputs: IdentityInputs,
        pub runtime: RuntimeObservation,
    }
    pub async fn setup() -> Setup {
        use std::os::unix::fs::PermissionsExt;
        let fixture = fixture();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let source = fixture["source_utf8"].as_str().unwrap().as_bytes().to_vec();
        std::fs::write(root.join("docs/cache.qmd"), &source).unwrap();
        std::fs::write(
            root.join("pyproject.toml"),
            fixture["environment_input_utf8"].as_str().unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join(".venv/bin/python"),
            fixture["kernel_executable_utf8"].as_str().unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(
            root.join(".venv/bin/python"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(root.join("kernel.json"), serde_json::to_vec(&json!({"argv":["python","-m","ipykernel_launcher","-f","{connection_file}"],"language":"python","display_name":"Presentation"})).unwrap()).unwrap();
        let collection: ContentConfiguration = toml::from_str("id='guide'\nowner='project'\nrepository='python'\npath='docs'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
        let preparation =
            prepare_collection_document(std::str::from_utf8(&source).unwrap(), &collection)
                .unwrap()
                .preparation
                .unwrap();
        let request = PageExecutionRequest {
            page: ExecutionPage {
                source: SourceLocation {
                    repository: "python".into(),
                    path: "docs/cache.qmd".try_into().unwrap(),
                    span: None,
                },
                collection: "guide".into(),
                working_directory: Some("docs".try_into().unwrap()),
                source_fingerprint: fingerprint_bytes(&source),
                format: diplodocus::documents::AuthoredFormat::Qmd,
                mode: diplodocus::configuration::ExecutionMode::Execute,
                page_veto: false,
                parser_version: diplodocus::provenance::PANACHE_VERSION.into(),
                qmd_policy: "qmd-mvp-v1".into(),
            },
            kernel: "python3".into(),
            defaults: preparation.defaults,
            cells: preparation.cells,
            declared_environment_inputs: vec![InputFingerprint {
                source: SourceLocation {
                    repository: "python".into(),
                    path: "pyproject.toml".try_into().unwrap(),
                    span: None,
                },
                fingerprint: fingerprint_bytes(
                    fixture["environment_input_utf8"]
                        .as_str()
                        .unwrap()
                        .as_bytes(),
                ),
            }],
        };
        let repositories = BTreeMap::from([("python".into(), root.to_owned())]);
        let launch = LaunchIdentityInput::resolve(LaunchResolverInput {
            repositories: repositories.clone(),
            spec_path: root.join("kernel.json"),
            selector: "python3".into(),
            search: vec![KernelSearchLocation {
                class: KernelSearchClass::JupyterPath,
                ordinal: 0,
                selected: true,
            }],
            executable_search_path: root.join(".venv/bin").into_os_string(),
            repository: "python".into(),
            working_directory: "docs".into(),
        })
        .await
        .unwrap();
        let mut build = BuildObservation::observe().await.unwrap();
        build.executable_digest = fingerprint_bytes(
            fixture["engine_executable_utf8"]
                .as_str()
                .unwrap()
                .as_bytes(),
        );
        let inputs = IdentityInputs {
            repositories,
            page: RepositoryFile::new("python", Path::new("docs/cache.qmd")).unwrap(),
            declared_files: vec![
                RepositoryFile::new("python", Path::new("pyproject.toml")).unwrap(),
            ],
            build,
            launch,
        };
        let runtime = RuntimeObservation {
            implementation: "ipython".into(),
            implementation_version: "7.1.0".into(),
            language: "python".into(),
            language_version: "3.14.7".into(),
            protocol_version: "5.3".into(),
        };
        Setup {
            directory,
            source,
            request,
            inputs,
            runtime,
        }
    }

    #[tokio::test]
    async fn snapshots_bind_preparation_and_detect_changes() {
        let setup = setup().await;
        let snapshot = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        let identity = snapshot
            .identity(
                &setup.runtime,
                &setup.request,
                &ExecutionDeadlines::default(),
            )
            .unwrap();
        assert_eq!(
            identity
                .key_input()
                .fields(&[
                    "schema",
                    "schemas",
                    "page",
                    "options",
                    "engine",
                    "policies",
                    "components",
                    "kernel",
                    "platform",
                    "deadlines_ms",
                    "environment_inputs"
                ])
                .unwrap()
                .len(),
            11
        );
        snapshot.revalidate(&setup.inputs.launch).await.unwrap();
        let mut changed = setup.request.clone();
        changed.cells[0].options.execution.eval.value = false;
        assert!(
            snapshot_inputs(setup.inputs.clone(), &changed, &setup.source)
                .await
                .is_err()
        );
        assert!(
            snapshot
                .identity(&setup.runtime, &changed, &ExecutionDeadlines::default())
                .is_err()
        );
        changed = setup.request.clone();
        changed.cells[0].cell.source.push_str("unsafe_mutation()\n");
        assert!(
            snapshot_inputs(setup.inputs.clone(), &changed, &setup.source)
                .await
                .is_err()
        );
        std::fs::write(setup.directory.path().join("pyproject.toml"), "changed").unwrap();
        assert_eq!(
            snapshot
                .revalidate(&setup.inputs.launch)
                .await
                .unwrap_err()
                .kind,
            ExecutionFailureKind::InputChanged
        );
    }

    #[tokio::test]
    async fn relocated_checkouts_have_identical_identity_and_private_debug() {
        let left = setup().await;
        let right = setup().await;
        let a = snapshot_inputs(left.inputs.clone(), &left.request, &left.source)
            .await
            .unwrap();
        let b = snapshot_inputs(right.inputs.clone(), &right.request, &right.source)
            .await
            .unwrap();
        let a = a
            .identity(&left.runtime, &left.request, &ExecutionDeadlines::default())
            .unwrap();
        let b = b
            .identity(
                &right.runtime,
                &right.request,
                &ExecutionDeadlines::default(),
            )
            .unwrap();
        assert_eq!(a.key(), b.key());
        assert_eq!(
            left.inputs.launch.spec_digest(),
            fixture()["key_input"]["kernel"]["spec_digest"]
        );
        assert_eq!(
            left.inputs.launch.launch_digest(),
            fixture()["key_input"]["kernel"]["launch_digest"]
        );
        for text in [
            format!("{:?}", left.inputs),
            String::from_utf8(a.key_input().encode().unwrap()).unwrap(),
        ] {
            assert!(!text.contains(left.directory.path().to_str().unwrap()));
            assert!(!text.contains("ipykernel_launcher"));
        }
    }
}

#[test]
fn strict_key_schema_rejects_unknown_missing_and_wrong_types_at_each_depth() {
    use diplodocus::execution::identity::validate_key_input;
    let input = fixture()["key_input"].clone();
    validate_key_input(&CanonicalValue::from_json(input.clone()).unwrap()).unwrap();
    fn records(value: &serde_json::Value, path: String, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                output.push(path.clone());
                for (name, field) in fields {
                    records(field, format!("{path}/{name}"), output);
                }
            }
            serde_json::Value::Array(values) => {
                for (index, field) in values.iter().enumerate() {
                    records(field, format!("{path}/{index}"), output);
                }
            }
            _ => {}
        }
    }
    let mut paths = Vec::new();
    records(&input, String::new(), &mut paths);
    for path in paths {
        let mut invalid = input.clone();
        let object = invalid.pointer_mut(&path).unwrap().as_object_mut().unwrap();
        object.insert("unknown".into(), serde_json::Value::Null);
        assert!(
            validate_key_input(&CanonicalValue::from_json(invalid).unwrap()).is_err(),
            "unknown at {path}"
        );
        let mut invalid = input.clone();
        let object = invalid.pointer_mut(&path).unwrap().as_object_mut().unwrap();
        let key = object.keys().next().unwrap().clone();
        object.remove(&key);
        assert!(
            validate_key_input(&CanonicalValue::from_json(invalid).unwrap()).is_err(),
            "missing at {path}"
        );
        let mut invalid = input.clone();
        *invalid.pointer_mut(&path).unwrap() = serde_json::Value::Bool(true);
        assert!(
            validate_key_input(&CanonicalValue::from_json(invalid).unwrap()).is_err(),
            "type at {path}"
        );
    }
}

#[test]
fn immutable_artifact_digest_and_each_key_dimension() {
    use diplodocus::execution::identity::validate_key_input;
    let bytes = include_bytes!("../docs/spikes/fixtures/execution-artifact-v1/manifest.json");
    let manifest = CanonicalValue::decode(bytes).unwrap();
    let fields = manifest
        .fields(&["schema", "key", "key_input", "result_digest", "result"])
        .unwrap();
    validate_key_input(&fields["key_input"]).unwrap();
    assert_eq!(
        CanonicalValue::String(
            domain_digest("diplodocus/page-execution-key-v1", &fields["key_input"]).unwrap()
        ),
        fields["key"]
    );
    assert_eq!(
        CanonicalValue::String(
            domain_digest("diplodocus/page-execution-result-v1", &fields["result"]).unwrap()
        ),
        fields["result_digest"]
    );
    let key = fixture()["key_input"].clone();
    let original = domain_digest(
        "diplodocus/page-execution-key-v1",
        &CanonicalValue::from_json(key.clone()).unwrap(),
    )
    .unwrap();
    fn leaves(value: &serde_json::Value, path: String, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                for (k, v) in fields {
                    leaves(v, format!("{path}/{k}"), output)
                }
            }
            serde_json::Value::Array(values) if !values.is_empty() => {
                for (i, v) in values.iter().enumerate() {
                    leaves(v, format!("{path}/{i}"), output)
                }
            }
            _ => output.push(path),
        }
    }
    let mut paths = Vec::new();
    leaves(&key, String::new(), &mut paths);
    assert!(paths.len() > 90);
    for path in paths {
        let mut changed = key.clone();
        let leaf = changed.pointer_mut(&path).unwrap();
        *leaf = match leaf {
            serde_json::Value::String(s) => serde_json::Value::String(format!("{s}-changed")),
            serde_json::Value::Bool(b) => (!*b).into(),
            serde_json::Value::Number(n) => (n.as_u64().unwrap() + 1).into(),
            _ => "changed".into(),
        };
        let hash = domain_digest(
            "diplodocus/page-execution-key-v1",
            &CanonicalValue::from_json(changed).unwrap(),
        )
        .unwrap();
        assert_ne!(original, hash, "{path}");
    }
}

mod acceptance {
    use super::snapshots::*;
    use diplodocus::execution::identity::*;
    use diplodocus::execution::*;
    use diplodocus::provenance::fingerprint_bytes;
    use serde_json::json;
    use std::path::Path;

    fn resolver(setup: &Setup) -> LaunchResolverInput {
        LaunchResolverInput {
            repositories: setup.inputs.repositories.clone(),
            spec_path: setup.directory.path().join("kernel.json"),
            selector: setup.request.kernel.clone(),
            search: setup.inputs.launch.search().to_vec(),
            executable_search_path: setup.directory.path().join(".venv/bin").into_os_string(),
            repository: "python".into(),
            working_directory: "docs".into(),
        }
    }
    #[tokio::test]
    async fn post_cleanup_detects_source_executable_spec_and_containment_changes() {
        use std::os::unix::fs::symlink;
        let setup = setup().await;
        let snapshot = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        let root = setup.directory.path();
        for file in [
            "docs/cache.qmd",
            "pyproject.toml",
            ".venv/bin/python",
            "kernel.json",
        ] {
            let path = root.join(file);
            let before = std::fs::read(&path).unwrap();
            std::fs::write(&path, b"different input").unwrap();
            let failure = snapshot.revalidate(&setup.inputs.launch).await.unwrap_err();
            assert_eq!(failure.kind, ExecutionFailureKind::InputChanged, "{file}");
            assert!(!format!("{failure:?}").contains(root.to_str().unwrap()));
            std::fs::write(&path, before).unwrap();
            snapshot.revalidate(&setup.inputs.launch).await.unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        let path = root.join("pyproject.toml");
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(outside.path().join("same"), bytes).unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(outside.path().join("same"), &path).unwrap();
        assert!(snapshot.revalidate(&setup.inputs.launch).await.is_err());
    }
    #[tokio::test]
    async fn spec_presentation_order_defaults_and_connection_filename_do_not_change_identity() {
        let setup = setup().await;
        let old = &setup.inputs.launch;
        let spec = json!({"env":{},"interrupt_mode":"signal","language":"PYTHON3","argv":["python","-m","ipykernel_launcher","-f","{connection_file}"],"display_name":"Changed presentation","metadata":{"unrelated":1.5}});
        std::fs::write(
            setup.directory.path().join("kernel.json"),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();
        let changed = LaunchIdentityInput::resolve(resolver(&setup))
            .await
            .unwrap();
        assert_eq!(old.spec_digest(), changed.spec_digest());
        assert_eq!(old.launch_digest(), changed.launch_digest());
        old.revalidate().await.unwrap();
        let a = old.arguments(Path::new("/tmp/connection-a.json")).unwrap();
        let b = old.arguments(Path::new("/tmp/connection-b.json")).unwrap();
        assert_ne!(a, b);
        assert_eq!(a[0], old.executable().to_str().unwrap());
        assert_eq!(a[4], "/tmp/connection-a.json");
    }
    #[tokio::test]
    async fn rejects_bad_specs_without_leaking_explicit_environment_values() {
        let setup = setup().await;
        let base = json!({"argv":["python","-f","{connection_file}"],"language":"python","env":{"SECRET":"private-value"}});
        let mut invalid = Vec::new();
        for (field, value) in [
            ("unsupported_behavior", json!(true)),
            ("interrupt_mode", json!("other")),
            ("argv", json!(["python"])),
            ("env", json!({"SECRET":"${HOME}"})),
            ("metadata", json!({"kernel_provisioner":{}})),
        ] {
            let mut spec = base.clone();
            spec[field] = value;
            invalid.push(serde_json::to_string(&spec).unwrap());
        }
        invalid.push("{\"argv\":[\"python\",\"{connection_file}\"],\"language\":\"python\",\"env\":{\"SECRET\":\"private-value\",\"SECRET\":\"second\"}}".into());
        for spec in invalid {
            std::fs::write(setup.directory.path().join("kernel.json"), spec).unwrap();
            let error = LaunchIdentityInput::resolve(resolver(&setup))
                .await
                .unwrap_err();
            assert!(!format!("{error:?} {error}").contains("private-value"));
        }
        std::fs::write(
            setup.directory.path().join("kernel.json"),
            serde_json::to_vec(&base).unwrap(),
        )
        .unwrap();
        let launch = LaunchIdentityInput::resolve(resolver(&setup))
            .await
            .unwrap();
        assert!(!format!("{launch:?}").contains("private-value"));
        assert_eq!(launch.environment()["SECRET"], "private-value");
    }
    #[tokio::test]
    async fn declared_inputs_are_normalized_sorted_regular_and_explicit() {
        let mut setup = setup().await;
        std::fs::write(setup.directory.path().join("second.lock"), b"second").unwrap();
        setup
            .inputs
            .declared_files
            .push(RepositoryFile::new("python", Path::new("docs/.././second.lock")).unwrap());
        setup
            .request
            .declared_environment_inputs
            .push(diplodocus::ir::InputFingerprint {
                source: diplodocus::ir::SourceLocation {
                    repository: "python".into(),
                    path: "second.lock".try_into().unwrap(),
                    span: None,
                },
                fingerprint: fingerprint_bytes(b"second"),
            });
        let a = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        setup.inputs.declared_files.reverse();
        setup.request.declared_environment_inputs.reverse();
        let b = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        assert_eq!(
            a.identity(
                &setup.runtime,
                &setup.request,
                &ExecutionDeadlines::default()
            )
            .unwrap(),
            b.identity(
                &setup.runtime,
                &setup.request,
                &ExecutionDeadlines::default()
            )
            .unwrap()
        );
        setup
            .inputs
            .declared_files
            .push(setup.inputs.declared_files[0].clone());
        assert!(
            snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
                .await
                .is_err()
        );
        setup.inputs.declared_files.pop();
        setup.request.declared_environment_inputs[0].fingerprint = fingerprint_bytes(b"forged");
        assert!(
            snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
                .await
                .is_err()
        );
        for path in ["../escape", "/absolute", "*.lock", "missing", "docs"] {
            let declaration = RepositoryFile::new("python", Path::new(path));
            if let Ok(declaration) = declaration {
                setup.inputs.declared_files = vec![declaration];
                assert!(
                    snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
                        .await
                        .is_err(),
                    "{path}"
                );
            }
        }
    }
    #[tokio::test]
    async fn identity_projection_observes_runtime_limits_build_and_all_prepared_fields() {
        let setup = setup().await;
        let snapshot = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        let deadlines = ExecutionDeadlines::default();
        let original = snapshot
            .identity(&setup.runtime, &setup.request, &deadlines)
            .unwrap();
        for index in 0..4 {
            let mut runtime = setup.runtime.clone();
            match index {
                0 => runtime.implementation.push_str("-new"),
                1 => runtime.implementation_version.push_str("-new"),
                2 => runtime.language_version.push_str("-new"),
                _ => runtime.protocol_version = "5.4".into(),
            }
            assert_ne!(
                original,
                snapshot
                    .identity(&runtime, &setup.request, &deadlines)
                    .unwrap()
            );
        }
        let mut aliases = setup.runtime.clone();
        aliases.language = "PYTHON3".into();
        assert_eq!(
            original,
            snapshot
                .identity(&aliases, &setup.request, &deadlines)
                .unwrap()
        );
        for index in 0..7 {
            let mut limits = deadlines;
            let fields = [
                &mut limits.startup,
                &mut limits.cell,
                &mut limits.terminal_sync,
                &mut limits.interrupt,
                &mut limits.shutdown,
                &mut limits.termination,
                &mut limits.forced_exit,
            ];
            *fields.into_iter().nth(index).unwrap() += 1;
            assert_ne!(
                original,
                snapshot
                    .identity(&setup.runtime, &setup.request, &limits)
                    .unwrap()
            );
        }
        let mut changed = setup.inputs.clone();
        changed.build.executable_digest = fingerprint_bytes(b"new build");
        let snapshot2 = snapshot_inputs(changed, &setup.request, &setup.source)
            .await
            .unwrap();
        assert_ne!(
            original,
            snapshot2
                .identity(&setup.runtime, &setup.request, &deadlines)
                .unwrap()
        );
        for index in 0..7 {
            let mut request = setup.request.clone();
            match index {
                0 => request.defaults.eval.value = false,
                1 => request.cells[0].ordinal = 8,
                2 => request.cells[0].cell.source_segments[0].text.push('x'),
                3 => request.cells[0].cell.span.end += 1,
                4 => {
                    request.cells[0].options.execution.echo.origin = OptionOrigin::Inline {
                        span: diplodocus::ir::SourceSpan { start: 0, end: 1 },
                    }
                }
                5 => request.page.working_directory = None,
                _ => request.cells.clear(),
            }
            assert!(
                snapshot_inputs(setup.inputs.clone(), &request, &setup.source)
                    .await
                    .is_err(),
                "mutation {index}"
            );
        }
        assert!(
            snapshot_inputs(
                setup.inputs.clone(),
                &setup.request,
                b"different prepared source"
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn revalidation_detects_executable_symlink_path_resolution_and_cwd_retargeting() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let setup = setup().await;
        let root = setup.directory.path();
        let snapshot = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
            .await
            .unwrap();
        std::fs::create_dir(root.join("alternate")).unwrap();
        std::fs::copy(root.join(".venv/bin/python"), root.join("alternate/python")).unwrap();
        std::fs::set_permissions(
            root.join("alternate/python"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut input = resolver(&setup);
        input.executable_search_path = root.join("alternate").into_os_string();
        let alternate = LaunchIdentityInput::resolve(input).await.unwrap();
        assert!(snapshot.revalidate(&alternate).await.is_err());
        std::fs::remove_file(root.join(".venv/bin/python")).unwrap();
        symlink(root.join("alternate/python"), root.join(".venv/bin/python")).unwrap();
        assert!(snapshot.revalidate(&setup.inputs.launch).await.is_err());
    }
}

#[tokio::test]
async fn public_launch_preserves_separators_in_argv_and_explicit_environment() {
    use diplodocus::execution::identity::*;
    use serde_json::json;
    use std::path::Path;
    let setup = snapshots::setup().await;
    let root = setup.directory.path();
    let resolve = || LaunchResolverInput {
        repositories: setup.inputs.repositories.clone(),
        spec_path: root.join("kernel.json"),
        selector: "python3".into(),
        search: setup.inputs.launch.search().to_vec(),
        executable_search_path: root.join(".venv/bin").into_os_string(),
        repository: "python".into(),
        working_directory: "docs".into(),
    };
    let mut observations = Vec::new();
    for suffix in [
        "sub/{connection_file}",
        "sub{connection_file}",
        "sub/",
        "sub",
        "a/../sub",
        "sub//",
    ] {
        let argument = format!("--config={}/{suffix}", root.display());
        std::fs::write(root.join("kernel.json"),serde_json::to_vec(&json!({"argv":["python","{connection_file}",argument],"language":"python","env":{"DIRECTORY":format!("{}/{suffix}",root.display())}})).unwrap()).unwrap();
        let launch = LaunchIdentityInput::resolve(resolve()).await.unwrap();
        assert_eq!(
            launch.arguments(Path::new("/tmp/file.json")).unwrap()[2],
            argument.replace("{connection_file}", "/tmp/file.json")
        );
        for previous in &observations {
            assert_ne!(launch.launch_digest(), previous);
        }
        observations.push(launch.launch_digest().to_owned());
    }
}

#[tokio::test]
async fn snapshot_projection_matches_the_complete_immutable_key_vector() {
    use diplodocus::execution::identity::*;
    use diplodocus::execution::{ExecutionComponent, ExecutionDeadlines};
    let mut setup = snapshots::setup().await;
    let fixture = fixture();
    setup.inputs.build.components = fixture["key_input"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["role"].as_str().unwrap().into(),
                ExecutionComponent {
                    name: v["name"].as_str().unwrap().into(),
                    version: v["version"].as_str().unwrap().into(),
                },
            )
        })
        .collect();
    let snapshot = snapshot_inputs(setup.inputs, &setup.request, &setup.source)
        .await
        .unwrap();
    let identity = snapshot
        .identity(
            &setup.runtime,
            &setup.request,
            &ExecutionDeadlines::default(),
        )
        .unwrap();
    assert_eq!(identity.key(), fixture["page_key"]);
    assert_eq!(
        identity.key_input().encode().unwrap(),
        fixture["canonical_utf8"].as_str().unwrap().as_bytes()
    );
}

#[tokio::test]
async fn current_build_observation_hashes_the_running_executable() {
    use diplodocus::execution::identity::BuildObservation;
    let observation = BuildObservation::observe().await.unwrap();
    let bytes = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    assert_eq!(
        observation.executable_digest,
        diplodocus::provenance::fingerprint_bytes(&bytes)
    );
    assert_eq!(observation.engine_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(observation.platform.os, std::env::consts::OS);
    assert_eq!(observation.platform.architecture, std::env::consts::ARCH);
    assert!(
        observation
            .platform
            .target
            .starts_with(std::env::consts::ARCH)
    );
}

#[tokio::test]
async fn post_cleanup_rejects_changed_cwd_even_with_identical_source() {
    use diplodocus::execution::identity::snapshot_inputs;
    use std::os::unix::fs::symlink;
    let setup = snapshots::setup().await;
    let root = setup.directory.path();
    let snapshot = snapshot_inputs(setup.inputs.clone(), &setup.request, &setup.source)
        .await
        .unwrap();
    std::fs::rename(root.join("docs"), root.join("moved-docs")).unwrap();
    symlink(root.join("moved-docs"), root.join("docs")).unwrap();
    assert!(snapshot.revalidate(&setup.inputs.launch).await.is_err());
}

#[test]
fn all_artifact_representation_vectors_match_production_hashes() {
    let manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../docs/spikes/fixtures/execution-artifact-v1/manifest.json"
    ))
    .unwrap();
    for cell in manifest["result"]["cells"].as_array().unwrap() {
        for output in cell["outputs"].as_array().unwrap() {
            for representation in output["representations"].as_array().unwrap() {
                let kind = representation["kind"].as_str().unwrap();
                let content = &representation["content"];
                let digest=match kind {
                    "text" if content["type"]=="literal"=>content_digest(content["text"].as_str().unwrap().as_bytes()),
                    "html-candidate"=>content_digest(content["markup"].as_str().unwrap().as_bytes()),
                    "asset"=>content["asset"]["digest"].as_str().unwrap().to_owned(),
                    _=>domain_digest("diplodocus/execution-representation-v1",&CanonicalValue::from_json(serde_json::json!({"kind":if kind=="text"{"error"}else{kind},"content":content})).unwrap()).unwrap()
                };
                assert_eq!(digest, representation["content_digest"]);
            }
        }
    }
}

#[test]
fn missing_build_record_fields_are_errors_without_panics() {
    assert!(
        build_metadata::selected_version("[[package]]\nname='diplodocus'\n", "0.1.0", "tokio")
            .is_err()
    );
}

#[test]
fn key_decoder_rejects_ranges_inexact_digests_and_cross_record_inconsistency() {
    use diplodocus::execution::identity::validate_key_input;
    for (path, value) in [
        ("/engine/version", serde_json::json!("^0.1")),
        ("/components/0/version", serde_json::json!("1.*")),
        ("/page/source_digest", serde_json::json!("sha256:UPPERCASE")),
        (
            "/options/cells/0/submitted_source_digest",
            serde_json::Value::Null,
        ),
        ("/options/cells/0/eligible", serde_json::json!(false)),
        ("/kernel/search/0/selected", serde_json::json!(false)),
        ("/kernel/search/0/ordinal", serde_json::json!(1)),
        ("/deadlines_ms/cell", serde_json::json!(0)),
        (
            "/environment_inputs/0/path",
            serde_json::json!("../outside"),
        ),
    ] {
        let mut input = fixture()["key_input"].clone();
        *input.pointer_mut(path).unwrap() = value;
        assert!(
            validate_key_input(&CanonicalValue::from_json(input).unwrap()).is_err(),
            "{path}"
        );
    }
}

#[tokio::test]
async fn key_projection_retains_nested_skipped_cells_and_all_effective_options() {
    use diplodocus::configuration::ContentConfiguration;
    use diplodocus::documents::prepare_collection_document;
    use diplodocus::execution::ExecutionDeadlines;
    use diplodocus::execution::identity::snapshot_inputs;
    let mut setup = snapshots::setup().await;
    let source = "---\nexecute: {echo: false, output: asis, include: false, error: true}\n---\n\n```{python}\n#| label: figure\n#| fig-alt: Alt\n#| fig-cap: Caption\n#| fig-subcap: [Second, First]\nprint('run')\n```\n\n> ```{r}\n> stop('skip language')\n> ```\n\n```{python}\n#| eval: false\nraise Exception('skip eval')\n```\n";
    let collection:ContentConfiguration=toml::from_str("id='guide'\nowner='project'\nrepository='python'\npath='docs'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
    let parsed = prepare_collection_document(source, &collection).unwrap();
    let prepared = parsed.preparation.expect("valid mixed cells");
    setup.source = source.as_bytes().to_vec();
    setup.request.defaults = prepared.defaults;
    setup.request.cells = prepared.cells;
    setup.request.page.source_fingerprint =
        diplodocus::provenance::fingerprint_bytes(&setup.source);
    std::fs::write(setup.directory.path().join("docs/cache.qmd"), &setup.source).unwrap();
    let snapshot = snapshot_inputs(setup.inputs, &setup.request, &setup.source)
        .await
        .unwrap();
    let identity = snapshot
        .identity(
            &setup.runtime,
            &setup.request,
            &ExecutionDeadlines::default(),
        )
        .unwrap();
    let key: serde_json::Value =
        serde_json::from_slice(&identity.key_input().encode().unwrap()).unwrap();
    let cells = key["options"]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 3);
    for (ordinal, cell) in cells.iter().enumerate() {
        assert_eq!(cell["ordinal"], ordinal);
        assert_eq!(cell["eligible"], ordinal == 0);
    }
    assert_eq!(cells[1]["language"], "r");
    assert!(cells[1]["submitted_source_digest"].is_null());
    assert!(cells[2]["submitted_source_digest"].is_null());
    assert_eq!(
        cells[0]["effective"],
        serde_json::json!({"eval":true,"echo":false,"output":"asis","include":false,"error":true,"label":"figure","fig-alt":"Alt","fig-cap":"Caption","fig-subcap":["Second","First"]})
    );
    diplodocus::execution::identity::validate_key_input(identity.key_input()).unwrap();
}

#[tokio::test]
async fn root_slash_preserves_argv_environment_and_revalidation_identity() {
    use diplodocus::execution::identity::{LaunchIdentityInput, LaunchResolverInput};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    let setup = snapshots::setup().await;
    let root = setup.directory.path();
    let resolver = || LaunchResolverInput {
        repositories: BTreeMap::from([("all".into(), PathBuf::from("/"))]),
        spec_path: root.join("kernel.json"),
        selector: "python3".into(),
        search: setup.inputs.launch.search().to_vec(),
        executable_search_path: root.join(".venv/bin").into_os_string(),
        repository: "all".into(),
        working_directory: root
            .join("docs")
            .strip_prefix("/")
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
    };
    for (left, right) in [
        ("/tmp/input", "//tmp/input"),
        ("/./tmp/input", "//./tmp/input"),
    ] {
        for in_environment in [false, true] {
            let spec = |value: &str| {
                json!({
                    "argv": [setup.inputs.launch.executable(), "{connection_file}", if in_environment { "unchanged" } else { value }],
                    "language": "python", "env": {"VALUE": if in_environment { value } else { "unchanged" }}
                })
            };
            std::fs::write(
                root.join("kernel.json"),
                serde_json::to_vec(&spec(left)).unwrap(),
            )
            .unwrap();
            let old = LaunchIdentityInput::resolve(resolver()).await.unwrap();
            std::fs::write(
                root.join("kernel.json"),
                serde_json::to_vec(&spec(right)).unwrap(),
            )
            .unwrap();
            let new = LaunchIdentityInput::resolve(resolver()).await.unwrap();
            assert_ne!(
                old.spec_digest(),
                new.spec_digest(),
                "{left} {right} env={in_environment}"
            );
            assert_ne!(old.launch_digest(), new.launch_digest());
            if in_environment {
                assert_ne!(old.environment(), new.environment());
            } else {
                assert_ne!(
                    old.arguments(Path::new("/tmp/connection.json")).unwrap(),
                    new.arguments(Path::new("/tmp/connection.json")).unwrap()
                );
            }
            assert!(old.revalidate().await.is_err());
        }
    }
}

#[tokio::test]
async fn active_connection_markers_take_precedence_over_repository_roots() {
    use diplodocus::execution::identity::{LaunchIdentityInput, LaunchResolverInput};
    use diplodocus::execution::{KernelSearchClass, KernelSearchLocation};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    let directory = tempfile::tempdir().unwrap();
    let roots = [
        directory.path().join("checkout-{connection_file}"),
        directory.path().join("relocated"),
    ];
    for root in &roots {
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/python"), b"same executable bytes").unwrap();
        std::fs::set_permissions(
            root.join("bin/python"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    for (active_argument, prefix) in [(true, ""), (true, "--config="), (false, "")] {
        let mut launches = Vec::new();
        for root in &roots {
            let rooted_value = format!("{}/data", root.display());
            let argument_value = format!("{prefix}{rooted_value}");
            let spec = json!({"argv":["python","{connection_file}",if active_argument {argument_value.as_str()} else {"constant"}],"language":"python","env":{"DIRECTORY":rooted_value}});
            std::fs::write(root.join("kernel.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
            let launch = LaunchIdentityInput::resolve(LaunchResolverInput {
                repositories: BTreeMap::from([("repo".into(), root.clone())]),
                spec_path: root.join("kernel.json"),
                selector: "python3".into(),
                search: vec![KernelSearchLocation {
                    class: KernelSearchClass::JupyterPath,
                    ordinal: 0,
                    selected: true,
                }],
                executable_search_path: root.join("bin").into_os_string(),
                repository: "repo".into(),
                working_directory: ".".into(),
            })
            .await
            .unwrap();
            let arguments = launch
                .arguments(Path::new("/tmp/actual-connection.json"))
                .unwrap();
            assert_eq!(arguments[0], root.join("bin/python").to_str().unwrap());
            assert_eq!(arguments[1], "/tmp/actual-connection.json");
            assert_eq!(launch.environment()["DIRECTORY"], rooted_value);
            if active_argument {
                assert_eq!(
                    arguments[2],
                    argument_value.replace("{connection_file}", "/tmp/actual-connection.json")
                );
            }
            launches.push(launch);
        }
        if active_argument {
            assert_ne!(launches[0].spec_digest(), launches[1].spec_digest());
            assert_ne!(launches[0].launch_digest(), launches[1].launch_digest());
        } else {
            assert_eq!(launches[0].spec_digest(), launches[1].spec_digest());
            assert_eq!(launches[0].launch_digest(), launches[1].launch_digest());
        }
    }
}
