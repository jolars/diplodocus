use super::*;

fn fixture() -> (
    PreparedExecution,
    CanonicalValue,
    Vec<u8>,
    BTreeMap<String, Vec<u8>>,
) {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/spikes/fixtures/execution-artifact-v1");
    let source = std::fs::read(root.join("source.qmd")).unwrap();
    let manifest = std::fs::read(root.join("manifest.json")).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    // Cache entries are build-specific, while the checked-in encoding vector is immutable.
    for pointer in ["/key_input", "/result/provenance"] {
        let identity = value.pointer_mut(pointer).unwrap();
        identity["engine"]["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
        for component in identity["components"].as_array_mut().unwrap() {
            bind_build_version(component);
        }
    }
    for cell in value["result"]["cells"].as_array_mut().unwrap() {
        for output in cell["outputs"].as_array_mut().unwrap() {
            for representation in output["representations"].as_array_mut().unwrap() {
                bind_build_version(&mut representation["producer"]);
            }
        }
    }
    let key = CanonicalValue::from_json(value["key_input"].clone()).unwrap();
    value["key"] = serde_json::json!(
        identity::domain_digest("diplodocus/page-execution-key-v1", &key).unwrap()
    );
    rehash(&mut value);
    let manifest = canonical(value);
    let config = toml::from_str("id='guide'\nowner='project'\nrepository='python'\npath='docs'\nmount='guide'\nformat='qmd'\n[execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n").unwrap();
    let preparation = crate::documents::prepare_collection_document(
        std::str::from_utf8(&source).unwrap(),
        &config,
    )
    .unwrap()
    .preparation
    .unwrap();
    let request = PageExecutionRequest {
        page: ExecutionPage {
            source: crate::ir::SourceLocation {
                repository: "python".into(),
                path: "docs/artifact.qmd".try_into().unwrap(),
                span: None,
            },
            collection: "guide".into(),
            working_directory: Some("docs".try_into().unwrap()),
            source_fingerprint: crate::provenance::fingerprint_bytes(&source),
            format: crate::documents::AuthoredFormat::Qmd,
            mode: crate::configuration::ExecutionMode::Execute,
            page_veto: false,
            parser_version: crate::provenance::PANACHE_VERSION.into(),
            qmd_policy: "qmd-mvp-v1".into(),
        },
        kernel: "python3".into(),
        defaults: preparation.defaults,
        cells: preparation.cells,
        declared_environment_inputs: vec![],
    };
    let context = crate::execution::output_safety::AuthoredOutputContext::new(
        request.page.source.clone(),
        "guide".into(),
        preparation.authored_anchors,
    );
    let prepared = PreparedExecution::checked(request, source, context).unwrap();
    let assets = std::fs::read_dir(root.join("assets/sha256"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    (prepared, key, manifest, assets)
}

fn bind_build_version(component: &mut serde_json::Value) {
    if matches!(
        component["name"].as_str(),
        Some(
            "diplodocus-html-sanitizer"
                | "diplodocus-svg-validator"
                | "diplodocus-raster-validator"
        )
    ) {
        component["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn reference_artifact_with_build_versions_restores_every_alternative_and_round_trips() {
    let (prepared, key, bytes, assets) = fixture();
    let page = codec::restore(&bytes, &key, &prepared, &assets).unwrap();
    assert_eq!(codec::encode(&prepared, &key, &page).unwrap(), bytes);
    assert_eq!(page.record().assets.len(), 2);
    assert_eq!(page.record().cells[0].outputs[0].slot, 1);
    assert_eq!(page.record().cells[0].outputs[1].updating_cell, Some(1));
    assert_eq!(page.diagnostics().len(), 3);
}

fn canonical(value: serde_json::Value) -> Vec<u8> {
    CanonicalValue::from_json(value).unwrap().encode().unwrap()
}
fn rehash(value: &mut serde_json::Value) {
    value["result_digest"] = serde_json::json!(
        identity::domain_digest(
            "diplodocus/page-execution-result-v1",
            &CanonicalValue::from_json(value["result"].clone()).unwrap()
        )
        .unwrap()
    );
}

#[test]
fn forged_results_fail_even_with_recomputed_digests() {
    use serde_json::json;
    let (prepared, key, bytes, assets) = fixture();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(codec::restore(&bytes, &key, &prepared, &assets).is_ok());
    let mutations: Vec<(&str, serde_json::Value)> = vec![
        ("/result/cells/0/ordinal", json!(99)),
        ("/result/cells/0/span/start", json!(1)),
        ("/result/cells/0/source_segments/0/text", json!("forged")),
        (
            "/result/cells/0/option_origins/echo",
            json!({"kind":"inline","span":{"start":0,"end":1}}),
        ),
        ("/result/cells/0/outputs/1/updating_cell", json!(0)),
        (
            "/result/cells/0/outputs/1/representations/0/content_digest",
            json!(format!("sha256:{}", "0".repeat(64))),
        ),
        (
            "/result/cells/0/outputs/1/representations/0/producer/version",
            json!(format!("{}-forged", env!("CARGO_PKG_VERSION"))),
        ),
        (
            "/result/cells/0/outputs/1/representations/2/producer/version",
            json!(format!("{}-forged", env!("CARGO_PKG_VERSION"))),
        ),
        (
            "/result/cells/0/outputs/1/representations/2/content/markup",
            json!("<script>bad()</script>"),
        ),
        (
            "/result/cells/0/outputs/1/representations/1/fragment/byte_length",
            json!(1),
        ),
        (
            "/result/cells/1/outputs/0/representations/0/content/blocks/0/inlines/0/asset/digest",
            json!(format!("sha256:{}", "0".repeat(64))),
        ),
        ("/result/cells/2/outputs/0/diagnostic_indices", json!([999])),
        ("/result/cells/3/outcome", json!("ok")),
        ("/result/diagnostics/0/severity", json!("error")),
        ("/result/assets/0/byte_size", json!(1)),
        ("/result/provenance/origin", json!("cache")),
        (
            "/result/provenance/preparation/declarations/0/raw",
            json!("forged"),
        ),
        (
            "/result/provenance/kernel/runtime/language_version",
            json!("different"),
        ),
    ];
    for (pointer, replacement) in mutations {
        let mut changed = original.clone();
        let target = changed
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer}"));
        assert_ne!(*target, replacement, "mutation must change {pointer}");
        *target = replacement;
        rehash(&mut changed);
        assert!(
            codec::restore(&canonical(changed), &key, &prepared, &assets).is_err(),
            "{pointer}"
        );
    }
    let mut corrupt = assets.clone();
    corrupt.values_mut().next().unwrap().push(0);
    assert!(codec::restore(&bytes, &key, &prepared, &corrupt).is_err());
    assert!(codec::restore(&bytes, &key, &prepared, &BTreeMap::new()).is_err());
    let mut whitespace = bytes.clone();
    whitespace.push(b'\n');
    assert!(codec::restore(&whitespace, &key, &prepared, &assets).is_err());
    assert!(codec::restore(&bytes[..bytes.len() - 1], &key, &prepared, &assets).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replacen(
        "{",
        "{\"schema\":\"page-execution-artifact-v1\",",
        1,
    );
    assert!(codec::restore(duplicate.as_bytes(), &key, &prepared, &assets).is_err());
}

#[test]
fn every_record_rejects_unknown_fields_and_omitted_nulls() {
    let (prepared, key, bytes, assets) = fixture();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    fn objects(value: &serde_json::Value, path: String, found: &mut Vec<(String, Option<String>)>) {
        match value {
            serde_json::Value::Object(fields) => {
                found.push((path.clone(), None));
                for (name, child) in fields {
                    if child.is_null() {
                        found.push((path.clone(), Some(name.clone())));
                    }
                    objects(child, format!("{path}/{name}"), found);
                }
            }
            serde_json::Value::Array(values) => {
                for (i, child) in values.iter().enumerate() {
                    objects(child, format!("{path}/{i}"), found);
                }
            }
            _ => {}
        }
    }
    let mut paths = Vec::new();
    objects(&original, String::new(), &mut paths);
    for (path, null) in paths {
        let mut value = original.clone();
        let fields = value.pointer_mut(&path).unwrap().as_object_mut().unwrap();
        if let Some(name) = &null {
            fields.remove(name);
        } else {
            fields.insert("unknown".into(), serde_json::json!(true));
        }
        rehash(&mut value);
        assert!(
            codec::restore(&canonical(value), &key, &prepared, &assets).is_err(),
            "{path}: {null:?}"
        );
    }
}

fn install(
    root: &Path,
    key: &CanonicalValue,
    bytes: &[u8],
    assets: &BTreeMap<String, Vec<u8>>,
) -> PathBuf {
    let digest = identity::domain_digest("diplodocus/page-execution-key-v1", key).unwrap();
    let entry = root.join("v1/sha256").join(&digest[7..]);
    std::fs::create_dir_all(entry.join("assets/sha256")).unwrap();
    std::fs::write(entry.join("manifest.json"), bytes).unwrap();
    for (name, bytes) in assets {
        std::fs::write(entry.join("assets/sha256").join(name), bytes).unwrap();
    }
    entry
}

#[test]
fn storage_rejects_missing_extra_and_symlinked_files_without_writes() {
    for case in [
        "manifest-missing",
        "asset-missing",
        "extra-file",
        "extra-asset",
        "asset-symlink",
        "manifest-symlink",
        "directory-symlink",
        "fifo",
        "truncated",
    ] {
        let (prepared, key, bytes, assets) = fixture();
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        assert!(matches!(
            storage::lookup(&cache, &key, &prepared),
            Lookup::Miss
        ));
        assert!(!cache.exists());
        let entry = install(&cache, &key, &bytes, &assets);
        let first = entry
            .join("assets/sha256")
            .join(assets.keys().next().unwrap());
        match case {
            "manifest-missing" => std::fs::remove_file(entry.join("manifest.json")).unwrap(),
            "asset-missing" => std::fs::remove_file(&first).unwrap(),
            "extra-file" => std::fs::write(entry.join("extra"), b"extra").unwrap(),
            "extra-asset" => std::fs::write(entry.join("assets/sha256/extra"), b"extra").unwrap(),
            "asset-symlink" | "manifest-symlink" => {
                let target = if case == "asset-symlink" {
                    first
                } else {
                    entry.join("manifest.json")
                };
                let outside = root.path().join("outside");
                std::fs::rename(&target, &outside).unwrap();
                std::os::unix::fs::symlink(&outside, &target).unwrap();
            }
            "directory-symlink" => {
                let outside = root.path().join("outside");
                std::fs::rename(entry.join("assets"), &outside).unwrap();
                std::os::unix::fs::symlink(&outside, entry.join("assets")).unwrap();
            }
            "fifo" => {
                std::fs::remove_file(&first).unwrap();
                rustix::fs::mknodat(
                    rustix::fs::CWD,
                    &first,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::RUSR,
                    0,
                )
                .unwrap();
            }
            "truncated" => std::fs::write(entry.join("manifest.json"), b"{").unwrap(),
            _ => unreachable!(),
        }
        assert!(
            matches!(storage::lookup(&cache, &key, &prepared), Lookup::Rejected),
            "{case}"
        );
        assert!(!cache.join("staging").exists());
        assert!(!cache.join("locks").exists());
    }
}

#[test]
fn publication_is_atomic_immutable_and_nonblocking() {
    use rustix::fs::{FlockOperation, flock};
    let (prepared, key, bytes, assets) = fixture();
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    let staged: BTreeMap<_, _> = assets
        .iter()
        .map(|(name, bytes)| {
            let path = root.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            (name.clone(), path)
        })
        .collect();
    assert_eq!(
        storage::publish(&cache, &key, &prepared, &bytes, staged.clone()),
        None
    );
    assert!(matches!(
        storage::lookup(&cache, &key, &prepared),
        Lookup::Hit(_)
    ));
    assert_eq!(std::fs::read_dir(cache.join("staging")).unwrap().count(), 0);
    let digest = identity::domain_digest("diplodocus/page-execution-key-v1", &key).unwrap();
    let lock = std::fs::File::open(cache.join("locks/sha256").join(&digest[7..])).unwrap();
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
    assert_eq!(
        storage::publish(&cache, &key, &prepared, b"invalid", BTreeMap::new()),
        None
    );
    drop(lock);
    let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let rep = &mut changed["result"]["cells"][0]["outputs"][0]["representations"][0];
    rep["content"]["text"] = serde_json::json!("different");
    rep["content_digest"] = serde_json::json!(identity::content_digest(b"different"));
    rehash(&mut changed);
    let changed = canonical(changed);
    assert!(codec::restore(&changed, &key, &prepared, &assets).is_ok());
    assert_eq!(
        storage::publish(&cache, &key, &prepared, &changed, staged.clone()),
        Some(DiagnosticCode::NonDeterministicExecution)
    );
    let entry = cache.join("v1/sha256").join(&digest[7..]);
    assert_eq!(std::fs::read(entry.join("manifest.json")).unwrap(), bytes);
    std::fs::write(entry.join("manifest.json"), b"invalid").unwrap();
    assert_eq!(
        storage::publish(&cache, &key, &prepared, &bytes, staged.clone()),
        None
    );
    assert!(matches!(
        storage::lookup(&cache, &key, &prepared),
        Lookup::Hit(_)
    ));
    let broken = root.path().join("not-a-directory");
    std::fs::write(&broken, "sentinel").unwrap();
    assert_eq!(
        storage::publish(&broken, &key, &prepared, &bytes, staged),
        Some(DiagnosticCode::ExecutionCacheUnavailable)
    );
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "sentinel");
}

#[test]
fn unsupported_schemas_miss_and_interrupted_publications_leave_no_entry() {
    let (prepared, key, bytes, assets) = fixture();
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    let mut future: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    future["schema"] = serde_json::json!("future");
    install(&cache, &key, &canonical(future), &assets);
    assert!(matches!(
        storage::lookup(&cache, &key, &prepared),
        Lookup::Miss
    ));
    std::fs::remove_dir_all(&cache).unwrap();
    let missing = BTreeMap::from([(
        assets.keys().next().unwrap().clone(),
        root.path().join("missing"),
    )]);
    assert_eq!(
        storage::publish(&cache, &key, &prepared, &bytes, missing),
        Some(DiagnosticCode::ExecutionCacheUnavailable)
    );
    assert!(matches!(
        storage::lookup(&cache, &key, &prepared),
        Lookup::Miss
    ));
    assert_eq!(std::fs::read_dir(cache.join("staging")).unwrap().count(), 0);
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(matches!(
        storage::lookup(&link, &key, &prepared),
        Lookup::Rejected
    ));
    assert_eq!(
        storage::publish(&link, &key, &prepared, &bytes, BTreeMap::new()),
        Some(DiagnosticCode::ExecutionCacheUnavailable)
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn concurrent_writers_publish_one_complete_entry() {
    let (prepared, key, bytes, assets) = fixture();
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    let staged: BTreeMap<_, _> = assets
        .iter()
        .map(|(name, bytes)| {
            let path = root.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            (name.clone(), path)
        })
        .collect();
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    storage::publish(&cache, &key, &prepared, &bytes, staged.clone())
                })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.join().unwrap(), None);
        }
    });
    assert!(matches!(
        storage::lookup(&cache, &key, &prepared),
        Lookup::Hit(_)
    ));
    assert_eq!(std::fs::read_dir(cache.join("staging")).unwrap().count(), 0);
}
