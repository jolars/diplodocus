# Execution origin and command authority

Page execution records and workspace document provenance record `executed` for
fresh results and `cache` for restored results. Disabled pages carry no
execution activity. The engine already implements these distinctions; this
validation adds direct evidence for the two Milestone 6 provenance and
command-authority gates.

## Portable provenance

`tests/execution_cache.rs` exercises the declared Python and R kernels with two
executable pages and one explicitly disabled page. It checks each page's origin
in both the loaded execution record and its workspace document. Repeated builds
restore both executable pages. Editing one page produces a mixed build: the
changed page records `executed`, the unchanged page records `cache`, and the
disabled page still has no execution activity. Changing a declared environment
input causes both eligible pages to execute again.

Relocation copies the source, declared environment, and complete cache tree,
including empty asset directories. The loaded snapshot has the same portable
workspace IR, public page execution records, and asset bytes as the original
execution after normalizing only execution activity origins. Snapshot loading
also validates the stored evidence and record hashes. Optional producer-slot
evidence on restored non-Markdown outputs follows the [shared-record
contract](execution-shared-records-validation.md); it is distinct from the
public portable page record. Rendered sites remain identical. A cell-side file
proves that restoration does not resubmit source.

The controllable protocol fixture in
`execution::jupyter::tests::engine::cache_hit_restores_assets_without_submitting_cells`
compares execution, restoration, and a second independent execution. It checks
provenance against observed connection paths, credentials, kernel process IDs,
ports, the executable path, checkout paths, and literal environment data. It
also rejects transient provenance fields such as timestamps and connection
identifiers. Configured deadlines remain valid portable numeric evidence.
Independent fresh executions produce identical records, while restoration
changes only the execution activity origin and retains identical asset bytes.

## Commands that cannot execute

The authority boundary has three layers:

- `commands::check` calls static assembly and resolution directly. It does not
  construct a runtime or dispatch workspace execution. CLI help and argument
  failures return before command dispatch.
- Authored preparation marks collections with omitted execution settings or
  `execution.mode = "never"` ineligible. `WorkspaceSources::execute` skips them
  before constructing an engine or creating workspace asset staging.
- The public Jupyter engine rejects requests without page execution authority
  before preparation, environment capture, kernelspec discovery, or cache
  access.

`tests/execution_isolation.rs` runs commands in child processes with isolated
Jupyter search roots, a startup-sentinel kernelspec, an empty executable search
path, and a private temporary directory. This avoids changing the environment of
parallel tests. Linux filesystem event monitors observe kernelspec access, cache
access and mutation, and writes to watched source or staging directories. Unlike
final file comparisons, these monitors retain events for files created and then
deleted. Positive controls prove that enabled execution reads the kernelspec and
starts its sentinel, and that transient create/write/delete operations are seen.

  | Path                                     | Cases and evidence                                                                                                                                                                                                                                                                                                                         |
  | ---                                      | ---                                                                                                                                                                                                                                                                                                                                        |
  | CLI and library `check`                  | Default-disabled, explicit `never`, and explicitly enabled collections; absent and populated caches; successful checks, warnings, missing or malformed configuration, invalid declarations, inactive execution settings, forbidden document selectors, missing content, malformed QMD metadata, unresolved references, and missing assets. |
  | CLI dispatch                             | Default and explicit configuration paths, `check --help`, unknown arguments, and a missing `--config` argument. Expected exit codes distinguish success, validation failure, and argument failure.                                                                                                                                         |
  | Check side effects                       | No kernelspec access, startup marker, cell marker, cache access, workspace mutations, or temporary-directory writes after each invocation.                                                                                                                                                                                                 |
  | Disabled workspace execution             | The library returns no executed pages and produces no filesystem events beneath its supplied asset-staging parent.                                                                                                                                                                                                                         |
  | Disabled `extract`, `build`, and `serve` | Both default and explicit `never`, with absent and populated caches. Initial and watched builds leave kernel and cache monitors quiet, write no cell marker, and publish snapshots with no execution records or generated assets.                                                                                                          |

Existing `tests/check_workspace.rs` also runs the complete acceptance workspace,
including Python and R extraction targets, with unavailable kernels and
unchanged source trees. `tests/workspace_assembly.rs` verifies disabled, vetoed,
and candidate-free pages with an unavailable staging parent. `tests/commands.rs`
checks disabled builds against an unusable cache path.

The command pipeline may create an empty temporary container for snapshot
assembly. This does not authorize a page or create execution assets. The direct
workspace test observes the staging boundary itself, and command snapshots
verify that no execution results or generated assets were published.

## Validation commands

```console
cargo test --locked --lib cache_hit_restores_assets_without_submitting_cells
cargo test --locked --test execution_cache --test execution_isolation
cargo test --locked --test check_workspace --test commands --test workspace_assembly
CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --all-targets -j 4
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

On September 25, 2026, the full suite passed 626 tests across 47 targets with no
ignored tests. Clippy passed with warnings denied, and Rust formatting passed.

The initial all-targets build exhausted local disk space while linking debug
binaries. Removing generated package artifacts and disabling debug symbols and
incremental compilation allowed validation to continue without changing the test
behavior.

The full Milestone 6 acceptance matrix remains a separate TODO. These checks
establish portable per-page origins and the disabled/check command boundary.
