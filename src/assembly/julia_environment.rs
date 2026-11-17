//! Current prepared Julia environments for private native result reuse.

use super::*;
use crate::incremental::{BuildIdentity, Manifest, Store};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::process::Stdio;
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Environment {
    identity: Fingerprint,
    inputs: Manifest,
}

#[derive(Deserialize)]
struct Observation {
    eligible: bool,
    identity: String,
    roots: Vec<PathBuf>,
}

pub(super) struct Context {
    store: Store,
    stage: &'static str,
    owner: String,
    identity: Fingerprint,
    inputs: Vec<Manifest>,
}

impl Context {
    pub(super) fn new(
        base: Option<(Store, String, Manifest)>,
        environment: &Environment,
        stage: &'static str,
        owner: &str,
        semantic: &impl Serialize,
        deadlines: ExecutionDeadlines,
    ) -> Option<Self> {
        let (store, build, inputs) = base?;
        let identity = crate::incremental::key(&(
            stage,
            build,
            owner,
            semantic,
            &inputs,
            environment,
            [
                deadlines.startup,
                deadlines.cell,
                deadlines.shutdown,
                deadlines.forced_exit,
            ],
        ));
        Some(Self {
            store,
            stage,
            owner: owner.into(),
            identity,
            inputs: vec![inputs, environment.inputs.clone()],
        })
    }

    pub(super) fn load<T: DeserializeOwned>(&self, reporting: &Reporting) -> Option<T> {
        let candidate = crate::incremental::verification_scope(|| {
            self.inputs
                .iter()
                .all(Manifest::verify)
                .then(|| self.store.load(self.stage, &self.identity))
                .flatten()
        });
        reporting.cache(
            self.stage,
            &self.owner,
            candidate.is_some(),
            candidate
                .is_none()
                .then_some("Package, document, or prepared Julia environment inputs changed."),
        );
        candidate
    }

    pub(super) fn manifests(&self) -> &[Manifest] {
        &self.inputs
    }

    pub(super) fn accepts_completion(&self, staging: &Path, reporting: &Reporting) -> bool {
        #[derive(Deserialize)]
        struct Completion {
            eligible: bool,
            roots: Vec<PathBuf>,
        }
        let completion = fs::read_to_string(staging.join("completion.toml"))
            .ok()
            .and_then(|text| toml::from_str::<Completion>(&text).ok());
        let covered: std::collections::BTreeSet<_> = self
            .inputs
            .iter()
            .flat_map(Manifest::paths)
            .filter_map(|path| fs::canonicalize(path).ok())
            .collect();
        let valid = completion.is_some_and(|completion| {
            completion.eligible
                && completion
                    .roots
                    .iter()
                    .all(|path| fs::canonicalize(path).is_ok_and(|path| covered.contains(&path)))
        });
        if !valid {
            reporting.cache(self.stage, &self.owner, false, Some("Loaded Julia source or native library dependencies extend beyond tracked inputs."));
        }
        valid
    }

    pub(super) fn save<T: Serialize>(&self, value: &T) {
        if crate::incremental::verification_scope(|| self.inputs.iter().all(Manifest::verify)) {
            let _ = self.store.save(self.stage, &self.identity, value);
        }
    }
}

pub(super) fn completed_source(source: &str, staging: &Path) -> std::io::Result<String> {
    fs::write(
        staging.join("cache_environment.jl"),
        include_str!("julia_environment.jl"),
    )?;
    Ok(format!(
        "{source}\ninclude(joinpath(@__DIR__, \"cache_environment.jl\"))\nwrite_completion(joinpath(@__DIR__, \"completion.toml\"))\n"
    ))
}

pub(super) async fn observe(
    configuration_directory: &Path,
    project: &Path,
    package_root: &Path,
    working: &Path,
    deadlines: ExecutionDeadlines,
    cancellation: &mut ExecutionCancellation<'_>,
) -> Result<Option<Environment>, AssemblyError> {
    let Some(store) = Store::new(configuration_directory) else {
        return Ok(None);
    };
    let Some(executable) = executable(working) else {
        return Ok(None);
    };
    let Some(build) = BuildIdentity::observe().ok() else {
        return Ok(None);
    };
    let project_file = if project.is_dir() {
        project.join("Project.toml")
    } else {
        project.to_owned()
    };
    let mut selectors = vec![executable.clone(), project_file];
    if let Some(search) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&search) {
            let candidate = working.join(directory).join("julia");
            let selected = fs::canonicalize(&candidate).ok().as_ref() == Some(&executable);
            selectors.push(candidate);
            if selected {
                break;
            }
        }
    }
    let environment_names = [
        "PATH",
        "LANG",
        "LC_ALL",
        "TZ",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "JULIA_PROJECT",
        "JULIA_LOAD_PATH",
        "JULIA_DEPOT_PATH",
        "JULIA_CPU_TARGET",
        "JULIA_NUM_THREADS",
        "JULIA_NUM_GC_THREADS",
        "JULIA_PKG_SERVER",
        "JULIA_PKG_OFFLINE",
        "JULIA_PKG_PRECOMPILE_AUTO",
    ]
    .map(str::to_owned);
    let Ok(selected) = Manifest::capture(&selectors, &environment_names) else {
        return Ok(None);
    };
    let key = crate::incremental::key(&(
        "julia-environment-v1",
        build,
        working,
        package_root,
        &selected,
    ));
    if let Some(candidate) = store.load::<Environment>("julia-environment", &key)
        && candidate.inputs.verify()
    {
        return Ok(Some(candidate));
    }
    let _ = store.discard("julia-environment", &key);
    let staging = tempfile::tempdir()?;
    let helper = staging.path().join("environment.jl");
    let output = staging.path().join("environment.toml");
    fs::write(&helper, include_str!("julia_environment.jl"))?;
    let mut command = tokio::process::Command::new(&executable);
    command
        .args(["--startup-file=no", "--history-file=no", "--color=no"])
        .arg(format!("--project={}", project.display()))
        .arg(&helper)
        .arg(&output)
        .arg(package_root)
        .current_dir(working)
        .env("JULIA_PKG_PRECOMPILE_AUTO", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    command.process_group(0);
    let Ok(child) = command.spawn() else {
        return Ok(None);
    };
    #[cfg(target_os = "linux")]
    let group = child
        .id()
        .and_then(|pid| rustix::process::Pid::from_raw(pid as i32));
    let mut process = super::rd_examples::Process {
        child,
        #[cfg(target_os = "linux")]
        group,
    };
    let status = tokio::select! {
        result = tokio::time::timeout(Duration::from_millis(deadlines.cell.max(1)), process.child.wait()) => match result {
            Ok(status) => status.ok(),
            Err(_) => { super::rd_examples::terminate(&mut process, deadlines).await?; None }
        },
        () = cancellation.as_mut() => { super::rd_examples::terminate(&mut process, deadlines).await?; return Err(AssemblyError::Cancelled); }
    };
    process.kill();
    #[cfg(target_os = "linux")]
    {
        process.group = None;
    }
    let observation: Option<Observation> = status
        .filter(|status| status.success())
        .and_then(|_| fs::read_to_string(&output).ok())
        .and_then(|text| toml::from_str(&text).ok());
    staging.close()?;
    let Some(observation) = observation.filter(|value| value.eligible) else {
        return Ok(None);
    };
    selectors.extend(observation.roots);
    let Ok(inputs) = Manifest::capture(&selectors, &environment_names) else {
        return Ok(None);
    };
    if !selected.verify() {
        return Err(AssemblyError::InputsChanged);
    }
    let environment = Environment {
        identity: crate::incremental::key(&observation.identity),
        inputs,
    };
    let _ = store.save("julia-environment", &key, &environment);
    Ok(Some(environment))
}

fn executable(working: &Path) -> Option<PathBuf> {
    let search = std::env::var_os("PATH")?;
    std::env::split_paths(&search).find_map(|directory| {
        let path = fs::canonicalize(working.join(directory).join("julia")).ok()?;
        let metadata = fs::metadata(&path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        metadata.is_file().then_some(path)
    })
}
