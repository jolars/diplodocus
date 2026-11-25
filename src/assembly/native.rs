use super::*;
use std::collections::BTreeSet;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Extractor {
    Julia,
    Rust,
    Doxygen,
    R,
    Python,
}

const CHECK_ORDER: [Extractor; 5] = [
    Extractor::Rust,
    Extractor::Doxygen,
    Extractor::R,
    Extractor::Python,
    Extractor::Julia,
];
const EXTRACTION_ORDER: [Extractor; 5] = [
    Extractor::Julia,
    Extractor::Rust,
    Extractor::Doxygen,
    Extractor::R,
    Extractor::Python,
];

impl Extractor {
    fn name(self) -> &'static str {
        match self {
            Self::Julia => "julia",
            Self::Rust => "rust",
            Self::Doxygen => "doxygen",
            Self::R => "r",
            Self::Python => "python",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Default)]
struct Status {
    completed: u8,
    stopped: bool,
    failed: Option<Extractor>,
}

async fn run<T>(
    extractor: Extractor,
    prerequisites: u8,
    status: &watch::Sender<Status>,
    operation: impl AsyncFnOnce(&mut ExecutionCancellation<'_>) -> Result<T, AssemblyError>,
) -> Result<T, AssemblyError> {
    let mut updates = status.subscribe();
    let stopped = updates
        .wait_for(|state| state.stopped || state.completed & prerequisites == prerequisites)
        .await
        .expect("the coordinator retains the sender")
        .stopped;
    if stopped {
        return Err(AssemblyError::Cancelled);
    }
    let mut cancellation: ExecutionCancellation<'_> = Box::pin(async move {
        let _ = updates.wait_for(|state| state.stopped).await;
    });
    let result = operation(&mut cancellation).await;
    status.send_modify(|state| {
        state.completed |= extractor.bit();
        if result.is_err() && !state.stopped {
            state.stopped = true;
            state.failed = Some(extractor);
        }
    });
    result
}

fn prerequisite_masks(
    configuration: &WorkspaceConfiguration,
    paths: &ResolvedWorkspacePaths,
    order: [Extractor; 5],
) -> [u8; 5] {
    let packages = &configuration.packages;
    let owners = order.map(|extractor| {
        packages
            .iter()
            .enumerate()
            .filter(|(_, package)| {
                package
                    .targets
                    .iter()
                    .any(|target| target.extractor == extractor.name())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    });
    let dependencies: Vec<_> = packages
        .iter()
        .map(|package| {
            let mut pending = package
                .incremental
                .depends_on
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if seen.insert(id)
                    && let Some(dependency) = packages.iter().find(|package| package.id == id)
                {
                    pending.extend(dependency.incremental.depends_on.iter().map(String::as_str));
                }
            }
            seen
        })
        .collect();

    let mut masks = [0; 5];
    for (later, &extractor) in order.iter().enumerate() {
        for (earlier, &predecessor) in order[..later].iter().enumerate() {
            let conflict = owners[later].iter().any(|&a| {
                owners[earlier].iter().any(|&b| {
                    let left = &paths.repositories[paths.packages[a].repository_index].path;
                    let right = &paths.repositories[paths.packages[b].repository_index].path;
                    left.starts_with(right)
                        || right.starts_with(left)
                        || dependencies[a].contains(packages[b].id.as_str())
                        || dependencies[b].contains(packages[a].id.as_str())
                })
            });
            if conflict {
                masks[extractor as usize] |= predecessor.bit();
            }
        }
    }
    masks
}

pub(crate) async fn assemble_for_check(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
) -> Result<WorkspaceSources, AssemblyError> {
    assemble(
        path,
        deadlines,
        cancellation,
        reporting,
        &mut Default::default(),
        false,
        false,
    )
    .await
}

pub(crate) async fn assemble_for_extraction(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    journal: &mut r_native::GeneratedWrites,
    bundle_export: bool,
) -> Result<WorkspaceSources, AssemblyError> {
    assemble(
        path,
        deadlines,
        cancellation,
        reporting,
        journal,
        bundle_export,
        true,
    )
    .await
}

async fn assemble(
    path: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
    reporting: &Reporting,
    journal: &mut r_native::GeneratedWrites,
    bundle_export: bool,
    run_examples: bool,
) -> Result<WorkspaceSources, AssemblyError> {
    let loading = reporting.stage("Loading workspace", None);
    let (text, mut configuration) = if bundle_export {
        let text = fs::read_to_string(path).map_err(|source| ConfigurationError::Read {
            path: path.into(),
            source,
        })?;
        let configuration =
            parse_configuration(&text).map_err(|source| ConfigurationError::Parse {
                path: path.into(),
                source,
            })?;
        (Some(text), configuration)
    } else {
        crate::configuration::resolve_configuration(
            path,
            path == Path::new(crate::configuration::DEFAULT_WORKSPACE),
        )?
    };
    let config_name = portable(Path::new(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("diplodocus.yaml"),
    ))?;
    reject_errors(validate_configuration_with_source(
        &configuration,
        config_name,
    ))?;
    if bundle_export {
        bundles::prepare_export(&mut configuration)?;
    }
    let paths = resolve_workspace_paths(path, &configuration)?;
    let order = if run_examples {
        EXTRACTION_ORDER
    } else {
        CHECK_ORDER
    };
    let masks = prerequisite_masks(&configuration, &paths, order);
    loading.finish();
    let (status, _) = watch::channel(Status::default());
    let configuration_ref = &configuration;
    let paths_ref = &paths;
    let prerequisites = |extractor: Extractor| masks[extractor as usize];

    use Extractor::*;
    // Keeping these futures in the caller's task preserves cache and output scopes.
    let jobs = async {
        tokio::join!(
            Box::pin(run(Julia, prerequisites(Julia), &status, async |cancel| {
                julia_runtime::results(
                    path,
                    configuration_ref,
                    paths_ref,
                    deadlines,
                    cancel,
                    reporting,
                )
                .await
            })),
            Box::pin(run(Rust, prerequisites(Rust), &status, async |cancel| {
                rust_results(
                    configuration_ref,
                    paths_ref,
                    run_examples,
                    deadlines,
                    cancel,
                    reporting,
                )
                .await
            })),
            Box::pin(run(
                Doxygen,
                prerequisites(Doxygen),
                &status,
                async |cancel| {
                    doxygen_results(configuration_ref, paths_ref, deadlines, cancel, reporting)
                        .await
                }
            )),
            Box::pin(run(R, prerequisites(R), &status, async |cancel| {
                r_results(
                    path,
                    configuration_ref,
                    paths_ref,
                    deadlines,
                    cancel,
                    reporting,
                    journal,
                )
                .await
            })),
            Box::pin(run(
                Python,
                prerequisites(Python),
                &status,
                async |cancel| {
                    python_native::results(
                        configuration_ref,
                        paths_ref,
                        deadlines,
                        cancel,
                        reporting,
                    )
                    .await
                }
            )),
        )
    };
    let (julia, rust, doxygen, r, python) = {
        tokio::pin!(jobs);
        tokio::select! {
            biased;
            () = cancellation.as_mut() => {
                status.send_modify(|state| state.stopped = true);
                jobs.await
            }
            results = &mut jobs => results,
        }
    };
    let state = *status.borrow();
    if state.stopped {
        let mut primary = None;
        let mut secondary = Vec::new();
        let mut errors = [
            julia.err(),
            rust.err(),
            doxygen.err(),
            r.err(),
            python.err(),
        ];
        for extractor in order {
            if let Some(error) = errors[extractor as usize].take() {
                if state.failed == Some(extractor) {
                    primary = Some(error);
                } else if !matches!(error, AssemblyError::Cancelled) {
                    secondary.push(error);
                }
            }
        }
        if primary.is_none() && !secondary.is_empty() {
            primary = Some(secondary.remove(0));
        }
        for error in secondary {
            reporting.error(&error.into());
        }
        return Err(primary.unwrap_or(AssemblyError::Cancelled));
    }

    let current = match fs::read_to_string(path) {
        Ok(current) => Some(current),
        Err(error) if text.is_none() && error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if current != text {
        return Err(AssemblyError::inputs_changed(
            "native extraction configuration verification",
            path.display().to_string(),
        ));
    }
    if resolve_workspace_paths(path, &configuration)? != paths {
        return Err(AssemblyError::inputs_changed(
            "native extraction path verification",
            path.display().to_string(),
        ));
    }
    let sources = assemble_with_native(
        path,
        text,
        configuration,
        NativeExtractions {
            julia: julia?,
            rust: rust?,
            doxygen: doxygen?,
            r: r?,
            python: python?,
        },
        bundle_export,
        reporting,
        cancellation,
    )
    .await?;
    if sources.paths.repositories != paths.repositories || sources.paths.packages != paths.packages
    {
        return Err(AssemblyError::inputs_changed(
            "assembled and native extraction path comparison",
            path.display().to_string(),
        ));
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[test]
    fn nested_cpp_repository_precedes_independent_r_and_python() {
        let root = tempfile::tempdir().unwrap();
        for file in [
            "Doxyfile",
            "r/DESCRIPTION",
            "python/pyproject.toml",
            "python/demo.py",
        ] {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let yaml = "
project: {name: Demo}
repositories:
  - {id: cpp, path: .}
  - {id: r, path: r}
  - {id: python, path: python}
packages:
  - id: cpp
    slug: cpp
    name: C++
    ecosystem: cpp
    repository: cpp
    path: .
    metadata-path: Doxyfile
    targets: [{id: api, extractor: doxygen, path: Doxyfile, role: public-api}]
  - id: r
    slug: r
    name: R
    ecosystem: r
    repository: r
    path: .
    metadata-path: DESCRIPTION
    targets: [{id: api, extractor: r, path: ., role: public-api}]
  - id: python
    slug: python
    name: Python
    ecosystem: python
    repository: python
    path: .
    metadata-path: pyproject.toml
    targets: [{id: api, extractor: python, path: demo.py, role: public-api}]
";
        let path = root.path().join("diplodocus.yaml");
        fs::write(&path, yaml).unwrap();
        let mut configuration = parse_configuration(yaml).unwrap();
        let paths = resolve_workspace_paths(&path, &configuration).unwrap();

        for order in [CHECK_ORDER, EXTRACTION_ORDER] {
            let masks = prerequisite_masks(&configuration, &paths, order);
            assert_eq!(masks[Extractor::R as usize], Extractor::Doxygen.bit());
            assert_eq!(masks[Extractor::Python as usize], Extractor::Doxygen.bit());
        }

        configuration.packages[2]
            .incremental
            .depends_on
            .push("r".into());
        let masks = prerequisite_masks(&configuration, &paths, EXTRACTION_ORDER);
        assert_eq!(
            masks[Extractor::Python as usize],
            Extractor::Doxygen.bit() | Extractor::R.bit()
        );
    }

    #[tokio::test]
    async fn failure_cancels_running_work_and_skips_waiting_work() {
        let (status, _) = watch::channel(Status::default());
        let (started, running) = oneshot::channel();
        let (r, python, waiting): (_, _, Result<(), _>) =
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::join!(
                    run(Extractor::R, 0, &status, async |cancel| {
                        started.send(()).unwrap();
                        cancel.as_mut().await;
                        // The coordinator must poll this work after cancellation.
                        tokio::task::yield_now().await;
                        Ok("cleaned up")
                    }),
                    run(Extractor::Python, 0, &status, async |_| {
                        running.await.unwrap();
                        Err::<(), _>(AssemblyError::inputs_changed(
                            "test input verification",
                            "simulated Python input change",
                        ))
                    }),
                    run(Extractor::Julia, Extractor::R.bit(), &status, async |_| {
                        unreachable!("dependent extractor started after failure");
                    }),
                )
            })
            .await
            .expect("independent extractors must overlap and finish cleanup");
        assert_eq!(r.unwrap(), "cleaned up");
        assert!(matches!(python, Err(AssemblyError::InputsChanged { .. })));
        assert!(matches!(waiting, Err(AssemblyError::Cancelled)));
        assert_eq!(status.borrow().failed, Some(Extractor::Python));
    }

    #[tokio::test]
    async fn dependent_work_starts_after_success() {
        let (status, _) = watch::channel(Status::default());
        let (finished, done) = oneshot::channel();
        let (python, r) = tokio::join!(
            run(Extractor::Python, Extractor::R.bit(), &status, async |_| {
                let mut done = done;
                assert_eq!(done.try_recv(), Ok(()));
                Ok(())
            }),
            run(Extractor::R, 0, &status, async |_| {
                tokio::task::yield_now().await;
                finished.send(()).unwrap();
                Ok(())
            }),
        );
        python.unwrap();
        r.unwrap();
    }
}
