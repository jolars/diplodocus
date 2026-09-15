use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{DiagnosticPath, DiagnosticSource};
use crate::ir::{
    ExecutionEngine, ExecutionMode, ExecutionOrigin, ExtractionInput, ExtractionMode,
    InputFingerprint, KernelProvenance, ParserProvenance, Provenance, ProvenanceActivity,
    SourceSpan, TargetReference,
};
use crate::paths::ResolvedRepositoryPaths;

/// Independent declaration and local version-control observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryObservation {
    /// Caller-supplied revision, preserved even when Git reports a different HEAD.
    pub declared: Option<String>,
    /// Exact Git HEAD object ID, if this supplied root is a Git worktree.
    pub observed: Option<String>,
    /// Tracked, staged, or untracked changes, or unknown when Git cannot report.
    pub dirty: Option<bool>,
}

/// Read this explicitly supplied root's Git HEAD and working-tree state.
///
/// An enclosing checkout is not evidence for a nested non-Git source root.
/// Git is invoked with optional index writes, filesystem-monitor hooks, and
/// lazy fetching disabled. Nothing is fetched, checked out, or executed from
/// documented code.
/// Missing Git, missing metadata, unborn HEADs, and failed commands leave the
/// corresponding observations unknown; a declared revision remains available.
/// Dirty state also remains unknown for configured content filters or indexed
/// submodules, since inspecting them can run repository-provided commands.
pub fn observe_repository(
    repository: &ResolvedRepositoryPaths,
    declared: Option<&str>,
) -> RepositoryObservation {
    let mut result = RepositoryObservation {
        declared: declared.map(str::to_owned),
        observed: None,
        dirty: None,
    };
    if !repository.path.join(".git").exists() {
        return result;
    }
    let Some(root) = git(repository, &["rev-parse", "--show-toplevel"]) else {
        return result;
    };
    let Ok(root) = String::from_utf8(root) else {
        return result;
    };
    if fs::canonicalize(root.trim_end_matches('\n')).ok().as_ref() != Some(&repository.path) {
        return result;
    }
    result.observed = git(repository, &["rev-parse", "--verify", "HEAD^{commit}"])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|value| value.trim_end().to_owned())
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if !can_observe_dirty_state(repository) {
        return result;
    }
    result.dirty = git(
        repository,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )
    .map(|bytes| !bytes.is_empty());
    result
}

fn git(repository: &ResolvedRepositoryPaths, args: &[&str]) -> Option<Vec<u8>> {
    git_command(repository, args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
}

fn can_observe_dirty_state(repository: &ResolvedRepositoryPaths) -> bool {
    // Status may execute clean/process filters to compare working files with
    // the index. Inspect only configuration names, and treat failed inspection
    // as unknown rather than running a potentially active comparison.
    let Ok(filters) = git_command(
        repository,
        &[
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ],
    )
    .output() else {
        return false;
    };
    if filters.status.code() != Some(1) || !filters.stdout.is_empty() || !filters.stderr.is_empty()
    {
        return false;
    }
    // A submodule can have its own filters and hooks. Skipping its contents in
    // status would hide real changes, so retain an explicitly unknown state.
    let Some(index) = git(repository, &["ls-files", "--stage", "-z"]) else {
        return false;
    };
    !index
        .split(|byte| *byte == 0)
        .any(|entry| entry.starts_with(b"160000 "))
}

fn git_command(repository: &ResolvedRepositoryPaths, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(&repository.path)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
        ])
        .args(args)
        // Even revision lookup can fetch a missing object from a promisor remote.
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(name);
    }
    command
}

/// Built-in extractors versioned with the Diplodocus semantic adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinExtractor {
    /// The static Python producer, implemented in Milestone 4.
    Python,
    /// The static R producer, implemented in Milestone 5.
    R,
}

impl BuiltinExtractor {
    /// Configured extractor identity.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::R => "r",
        }
    }
}

/// Evidence supplied by a completed static extractor, not inferred by collection.
///
/// Milestones 4 and 5 own parser settings, capability claims, full contributing
/// input enumeration, and fact-level [`crate::ir::SourceEvidence`]. Constructing
/// this record must follow successful extraction; dependency availability alone
/// is not evidence that an extractor or parser ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionObservation {
    /// Configured target that produced the facts.
    pub target: TargetReference,
    /// Built-in extractor identity, `python` or `r`.
    pub extractor: BuiltinExtractor,
    /// Complete capabilities actually implemented by this producer.
    pub capabilities: BTreeSet<String>,
    /// Exact parser versions, semantic roles, and output-affecting settings.
    pub parsers: BTreeMap<String, ParserProvenance>,
    /// All contributing content hashes and parser associations.
    pub inputs: BTreeMap<String, BTreeMap<DiagnosticPath, ExtractionInput>>,
}

impl ExtractionObservation {
    /// Encode a completed built-in extraction, retaining explicit source evidence.
    pub fn into_provenance(
        self,
        source: Option<DiagnosticSource>,
        span: Option<SourceSpan>,
    ) -> Provenance {
        let mut tools = BTreeMap::from([
            ("diplodocus".into(), env!("CARGO_PKG_VERSION").into()),
            (
                self.extractor.as_str().into(),
                env!("CARGO_PKG_VERSION").into(),
            ),
        ]);
        tools.extend(
            self.parsers
                .iter()
                .map(|(name, parser)| (name.clone(), parser.version.clone())),
        );
        Provenance {
            source,
            span,
            tools,
            activity: ProvenanceActivity::Extraction {
                target: self.target,
                mode: ExtractionMode::Static,
                capabilities: self.capabilities,
                parsers: self.parsers,
                inputs: self.inputs,
            },
        }
    }
}

/// Evidence supplied by an execution engine after an authorized result exists.
///
/// Milestone 6 owns kernel/toolchain observations and execution modes. Unknown
/// kernel versions stay `None`; no default implementation launches a kernel.
/// This interface accepts portable IR only, without environment values or launch
/// paths. Cache restoration and live execution identify their origin explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionObservation {
    /// Engine that produced or restored the result.
    pub engine: ExecutionEngine,
    /// Selected kernel and only the versions it actually reported.
    pub kernel: KernelProvenance,
    /// Fresh execution or restoration of a prior execution.
    pub origin: ExecutionOrigin,
    /// Exact observed execution component versions, keyed by identity.
    pub tools: BTreeMap<String, String>,
    /// Collected declared environment inputs, never environment values.
    pub declared_environment_inputs: Vec<InputFingerprint>,
}

impl ExecutionObservation {
    /// Encode an executed result with its producer-supplied page/cell context.
    pub fn into_provenance(
        mut self,
        source: Option<DiagnosticSource>,
        span: Option<SourceSpan>,
    ) -> Provenance {
        self.declared_environment_inputs.sort_by(|left, right| {
            (&left.source.repository, &left.source.path)
                .cmp(&(&right.source.repository, &right.source.path))
        });
        Provenance {
            source,
            span,
            tools: self.tools,
            activity: ProvenanceActivity::Execution {
                mode: ExecutionMode::Execute,
                engine: self.engine,
                kernel: self.kernel,
                origin: self.origin,
                declared_environment_inputs: self.declared_environment_inputs,
            },
        }
    }
}
