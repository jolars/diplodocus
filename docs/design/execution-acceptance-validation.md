# Execution acceptance

The final Milestone 6 test item combines the controllable Jupyter subprocess
fixture with the Python and R kernels declared in `devenv.nix`. These tests run
unconditionally in the normal Linux test suite. A missing declared runtime fails
the suite rather than skipping execution coverage. CI runs the suite through
`devenv shell -- cargo test --locked --all-targets`.

## Coverage

| Requirement | Controllable fixture and unit evidence | Real Python and R evidence |
| --- | --- | --- |
| Success and source order | `tests::pages::a_page_submits_exact_source_sequentially_in_one_fresh_session` checks exact requests and both terminal-message orders. | `tests/milestone_six.rs` builds QMD through the CLI, loads the published SQLite snapshot, and compares every cell, structured output, diagnostic, and asset with the checked-in Python/R snapshots. |
| State retention | The protocol fixture requires `define` before `use` in one session. | Both acceptance pages compute a total, use it in later cells, raise an allowed error, and use it again. The existing `declared_python_and_r_retain_definitions_and_imports_only_within_a_page` test also checks fresh page sessions. |
| Rich output | Reducer and public-engine tests validate alternatives, display updates, clearing, safe HTML/Markdown, and figures. | Both CLI fixtures publish stdout, stderr, HTML, Markdown, and SVG. Tests inspect the rendered site and compare fresh and cached portable results, asset bytes, and complete site trees. |
| Timeouts | Startup, cell, and both terminal-message orders have independent deadline tests, including continuous output and missing terminal events. | Both real kernels hang after an earlier cell has staged a figure. The engine must report a cell timeout at the active cell and stop before the next cell. |
| Interruption | Lifecycle tests observe message and signal interrupts; shutdown tests cover escalation and descendants. | Both real kernels catch the interrupt and write a sentinel. Tests require it after timeout, cooperative cancellation, and dropping execution. |
| Missing kernels | The public-engine test uses an isolated empty discovery environment and verifies no launch, cache, or asset writes. | CLI builds of Python and R pages with an absent selector must fail without publishing a site or snapshot or executing the first cell. |
| Unsupported MIME types | The controllable fixture sends JavaScript-only and JavaScript/plain-text bundles. The engine and cache must retain a diagnosed, payload-free placeholder or the safe fallback, respectively. | Both real kernels publish the same bundles. Structured snapshots retain MIME names and diagnostic attribution; rendered output excludes the rejected payload. |
| Deterministic cleanup | Existing lifecycle tests observe reaping, connection-directory removal, process-group cleanup, forced termination, and rollback. | Success, disallowed errors, timeouts, cancellation, and drop verify the real kernel PID is gone and its private connection directory has been removed. Failure tests check that earlier figures are discarded, later cells never run, and no cache artifact is published. |

The real failure tests synchronize cancellation with a sentinel written by the
running second cell. Before cancellation, they verify that the first cell's
figure exists in staging. This distinguishes rollback from a test that never
created an asset. Normal success and explicit failure require cleanup before
return; dropping the future uses a bounded observation of the supervisor's
asynchronous cleanup.

The structured-output goldens live in `tests/snapshots/milestone-six/`, with
authored inputs in `tests/fixtures/execution-kernels/`. They exclude page-level
build and runtime identities, which vary with the test executable and environment.
They retain complete cell records, accepted representation fingerprints, typed
output, diagnostic references, and asset metadata. Fresh/cache comparisons use
the complete page record and permit only the documented execution-origin change.
The run sentinel proves that restoring a cache entry submits no authored cells.

## Related exit-gate evidence

The [command-authority tests](execution-authority-validation.md) directly monitor
disabled collections and every `check` path for discovery, startup, cache access,
and execution writes. The [cache tests](execution-cache-validation.md) additionally
cover relocation, corruption, invalidation, concurrency, warning replay, and
failures during lookup. The [watched-site tests](execution-watched-validation.md)
cover preservation and recovery of the last successful publication.

## Validation

Run the focused acceptance tests with:

```sh
cargo test --locked --lib execution::jupyter::tests::engine::acceptance
cargo test --locked --test milestone_six
```

The commands require the declared development environment, including
`JUPYTER_PATH`. The complete regression command is:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
```

The initial full run exposed four unrelated fixture tests that exhausted their
shared three-second startup limit under parallel load. Ordinary fixture startup
now allows ten seconds. Tests that exercise startup deadlines still inject their
own short limits, and production deadlines retain their existing values.

One initial CLI snapshot-generation attempt timed out in the first Python cell.
The next attempt generated both snapshots and passed all fresh/cache checks.
Three subsequent standalone runs of that same CLI test also passed. The tests
fail on execution errors and do not retry kernels or regenerate expected output
during normal validation.

Final validation on September 25, 2026, in the declared development environment:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --locked --all-targets` | 632 tests passed, with no failures or ignored tests. Includes all four new real-kernel acceptance tests and both new protocol tests. |
| `cargo test --locked --doc` | 27 documentation tests passed. |
| Three additional standalone CLI acceptance runs | All passed for both Python and R, including exact snapshots and cache restoration. |
| `git diff --check` | Passed. |
