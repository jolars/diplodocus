use super::*;

fn included_request(root: &Path) -> (ExecutionContext<'static>, PageExecutionRequest) {
    let (context, request) = request(root, SOURCE);
    write(root.join("parts/cells.qmd"), SOURCE);
    write(&context.page_path, "{{< include ../parts/cells.qmd >}}");
    (context, request)
}

#[tokio::test]
async fn expanded_requests_are_verified_against_current_original_inputs_before_startup() {
    for change in ["cell", "edit", "missing", "escape", "frontmatter", "cycle"] {
        let root = TempDir::new().unwrap();
        fixture_kernel(root.path(), "execute-images").await;
        let (context, mut request) = included_request(root.path());
        let partial = root.path().join("parts/cells.qmd");
        let outside = TempDir::new().unwrap();
        match change {
            "cell" => request.cells[0].cell.source = "forged code".into(),
            "edit" => write(&partial, SOURCE.replace("define", "changed")),
            "missing" => std::fs::remove_file(&partial).unwrap(),
            "escape" => {
                write(outside.path().join("cells.qmd"), SOURCE);
                std::fs::remove_file(&partial).unwrap();
                std::os::unix::fs::symlink(outside.path().join("cells.qmd"), &partial).unwrap();
            }
            "frontmatter" => write(&partial, format!("---\nexecute: true\n---\n\n{SOURCE}")),
            "cycle" => write(&partial, "{{< include cells.qmd >}}"),
            _ => unreachable!(),
        }
        let failure = engine(root.path())
            .with_source_root(root.path().join("guide"))
            .execute_page(context, &request)
            .await
            .unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::InputChanged, "{change}");
        assert!(!root.path().join("observations.json").exists(), "{change}");
        assert!(!root.path().join("requests").exists(), "{change}");
    }
}

#[tokio::test]
async fn execution_and_cache_hits_recheck_includes_after_kernel_cleanup() {
    for cached in [false, true] {
        for change in ["edit", "delete", "retarget"] {
            let root = TempDir::new().unwrap();
            fixture_kernel(root.path(), "execute-images").await;
            let (context, request) = included_request(root.path());
            let deadlines = context.deadlines;
            if change == "retarget" {
                std::fs::rename(
                    root.path().join("parts/cells.qmd"),
                    root.path().join("parts/original.qmd"),
                )
                .unwrap();
                write(root.path().join("parts/replacement.qmd"), SOURCE);
                std::os::unix::fs::symlink("original.qmd", root.path().join("parts/cells.qmd"))
                    .unwrap();
            }
            let engine = engine(root.path())
                .with_source_root(root.path().join("guide"))
                .with_cache_root(root.path().join("cache"));
            let manifest = if cached {
                engine.execute_page(context, &request).await.unwrap();
                let manifest = cache_manifest(root.path());
                std::fs::remove_dir_all(root.path().join("assets")).unwrap();
                Some((manifest.clone(), std::fs::read(manifest).unwrap()))
            } else {
                None
            };
            write(root.path().join("mutate-include-on-shutdown"), change);
            // Recreate only the runtime context; the original inputs must stay intact.
            let context = ExecutionContext {
                repository_root: root.path().to_owned(),
                page_path: root.path().join("guide/example.qmd"),
                asset_staging_directory: root.path().join("assets"),
                deadlines,
                cancellation: Box::pin(pending()),
            };
            let failure = engine.execute_page(context, &request).await.unwrap_err();
            assert_eq!(
                failure.kind,
                ExecutionFailureKind::InputChanged,
                "cached={cached}, {change}"
            );
            assert!(failure.cleanup_diagnostics.is_empty());
            let assets = root.path().join("assets");
            if assets.exists() {
                assert_eq!(std::fs::read_dir(assets).unwrap().count(), 0);
            }
            assert_eq!(
                std::fs::read_to_string(root.path().join("requests"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
            if let Some((path, bytes)) = manifest {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            } else {
                assert!(!root.path().join("cache").exists());
            }
            assert_reaped(root.path()).await;
        }
    }
}
