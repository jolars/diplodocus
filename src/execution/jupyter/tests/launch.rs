use super::super::launch::ResolvedKernel;
use super::super::session::{start_resolved_before, start_resolved_session};
use super::*;
use std::collections::BTreeMap;

async fn shadowed_fixture_kernel(root: &Path) -> super::super::discovery::SelectedKernel {
    let kernel = fixture_kernel(root, "normal").await;
    let value: Value =
        serde_json::from_slice(&std::fs::read(kernel.directory.join("kernel.json")).unwrap())
            .unwrap();
    install(&root.join("second"), "fixture", &value);
    let kernel = discover_kernel("fixture", &environment(root), &source())
        .await
        .unwrap();
    assert_eq!(kernel.diagnostics.len(), 1);
    kernel
}

fn assert_one_discovery_warning(failure: &crate::execution::ExecutionFailure) {
    assert_eq!(
        failure.diagnostics[0].code,
        DiagnosticCode::ShadowedKernelspec
    );
    assert_eq!(
        failure
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ShadowedKernelspec)
            .count(),
        1
    );
}

#[tokio::test]
async fn startup_rejects_a_spec_changed_after_discovery() {
    let root = TempDir::new().unwrap();
    let kernel = shadowed_fixture_kernel(root.path()).await;
    let spec = kernel.directory.join("kernel.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&spec).unwrap()).unwrap();
    value["env"]["DIPLODOCUS_LITERAL"] = json!("changed after discovery");
    std::fs::write(&spec, serde_json::to_vec(&value).unwrap()).unwrap();
    let result = start_session(kernel, &mut context(root.path()), source()).await;
    match result {
        Ok(session) => {
            session.shutdown().await.unwrap();
            panic!("A changed discovery observation must not launch a kernel.");
        }
        Err(failure) => {
            assert_eq!(failure.kind, ExecutionFailureKind::InputChanged);
            assert_one_discovery_warning(&failure);
        }
    }
    assert!(!root.path().join("observations.json").exists());
}

#[tokio::test]
async fn resolved_launch_rejects_changed_inputs_before_spawning() {
    for mutation in ["spec", "executable", "path"] {
        let root = TempDir::new().unwrap();
        let kernel = shadowed_fixture_kernel(root.path()).await;
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let executable = bin.join("first");
        std::fs::copy(std::fs::canonicalize("/bin/sh").unwrap(), &executable).unwrap();
        std::fs::copy(&executable, bin.join("second")).unwrap();
        std::os::unix::fs::symlink(&executable, bin.join("runtime")).unwrap();
        let kernel = edit_kernel(root.path(), &kernel, |spec| {
            spec["argv"][0] = json!("runtime");
        })
        .await;
        let mut context = context(root.path());
        let resolved = ResolvedKernel::resolve(
            kernel.clone(),
            BTreeMap::from([("docs".into(), root.path().to_owned())]),
            &context.repository_root,
            &context.page_path,
            &source(),
        )
        .await
        .unwrap();
        assert_eq!(resolved.identity().executable(), executable);
        match mutation {
            "spec" => {
                edit_kernel(root.path(), &kernel, |spec| {
                    spec["env"]["DIPLODOCUS_LITERAL"] = json!("changed");
                })
                .await;
            }
            "executable" => {
                use std::io::Write;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&executable)
                    .unwrap()
                    .write_all(b"changed bytes")
                    .unwrap();
            }
            "path" => {
                std::fs::remove_file(bin.join("runtime")).unwrap();
                std::os::unix::fs::symlink(bin.join("second"), bin.join("runtime")).unwrap();
            }
            _ => unreachable!(),
        }
        let failure = start_resolved_session(resolved, &mut context, source())
            .await
            .err()
            .expect("Changed launch facts cannot start a session.");
        assert_eq!(
            failure.kind,
            ExecutionFailureKind::InputChanged,
            "{mutation}"
        );
        assert_one_discovery_warning(&failure);
        assert!(failure.cleanup_diagnostics.is_empty(), "{mutation}");
        assert!(
            !root.path().join("observations.json").exists(),
            "{mutation}"
        );
    }
}

#[tokio::test]
async fn resolved_launch_identity_survives_execution_and_cleanup() {
    let root = TempDir::new().unwrap();
    let kernel = shadowed_fixture_kernel(root.path()).await;
    let mut context = context(root.path());
    let repositories = BTreeMap::from([
        ("docs".into(), root.path().to_owned()),
        ("alias".into(), root.path().to_owned()),
    ]);
    let resolved = ResolvedKernel::resolve(
        kernel,
        repositories,
        &context.repository_root,
        &context.page_path,
        &source(),
    )
    .await
    .unwrap();
    let identity = resolved.identity().clone();
    let session = start_resolved_session(resolved, &mut context, source())
        .await
        .unwrap();
    assert_eq!(session.runtime.language, identity.language());
    assert_eq!(session.kernel.diagnostics.len(), 1);
    assert_eq!(
        observation(root.path())["literal"],
        "space ; $(not-a-command)"
    );
    session.shutdown().await.unwrap();
    identity.revalidate().await.unwrap();
    assert_cleaned(root.path()).await;
}

#[tokio::test]
async fn resolved_launch_honors_cancellation_before_spawning() {
    let root = TempDir::new().unwrap();
    let kernel = shadowed_fixture_kernel(root.path()).await;
    let mut context = context(root.path());
    let resolved = ResolvedKernel::resolve(
        kernel,
        BTreeMap::from([("docs".into(), root.path().to_owned())]),
        &context.repository_root,
        &context.page_path,
        &source(),
    )
    .await
    .unwrap();
    context.cancellation = Box::pin(ready(()));
    let failure = start_resolved_session(resolved, &mut context, source())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.kind, ExecutionFailureKind::Cancelled);
    assert_one_discovery_warning(&failure);
    assert!(failure.cleanup_diagnostics.is_empty());
    assert!(!root.path().join("observations.json").exists());
}

#[tokio::test]
async fn preflight_failure_preserves_discovery_warnings_without_launching() {
    for cancel in [false, true] {
        let root = TempDir::new().unwrap();
        let kernel = shadowed_fixture_kernel(root.path()).await;
        let mut context = context(root.path());
        if cancel {
            context.cancellation = Box::pin(ready(()));
        } else {
            context.deadlines.startup = 0;
        }
        let failure = start_session(kernel, &mut context, source())
            .await
            .err()
            .unwrap();
        assert_eq!(
            failure.kind,
            if cancel {
                ExecutionFailureKind::Cancelled
            } else {
                ExecutionFailureKind::Startup
            }
        );
        assert_one_discovery_warning(&failure);
        assert!(!root.path().join("observations.json").exists());
    }
}

#[tokio::test]
async fn startup_keeps_the_original_deadline_after_resolution() {
    let root = TempDir::new().unwrap();
    let kernel = shadowed_fixture_kernel(root.path()).await;
    let mut context = context(root.path());
    let deadline = tokio::time::Instant::now();
    let resolved = ResolvedKernel::resolve(
        kernel,
        BTreeMap::from([("docs".into(), root.path().to_owned())]),
        &context.repository_root,
        &context.page_path,
        &source(),
    )
    .await
    .unwrap();
    match start_resolved_before(resolved, &mut context, source(), deadline).await {
        Ok(session) => {
            session.shutdown().await.unwrap();
            panic!("Resolution must not restart an expired startup budget.");
        }
        Err(failure) => {
            assert_eq!(
                failure.kind,
                ExecutionFailureKind::Timeout {
                    phase: ExecutionPhase::Startup
                }
            );
            assert_one_discovery_warning(&failure);
            assert!(failure.cleanup_diagnostics.is_empty());
        }
    }
    assert!(!root.path().join("observations.json").exists());
}
