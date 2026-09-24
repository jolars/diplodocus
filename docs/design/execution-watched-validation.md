# Timeout and watched-site integration

This integration completes the behavior requested by the compound timeout and
watched-site item in `TODO.md`. It connects the existing supervised execution
engine to real snapshot publication, generation, and preview commands. It does
not establish the separate execution-cache or full Milestone 6 acceptance gates.

## Failure boundary

`build` performs static assembly and resolution, checks output destinations,
executes authorized pages, revalidates inputs, and copies verified outputs and
assets into a standalone SQLite snapshot. Only a successful extraction publishes
that database. Generation loads that completed snapshot, constructs the site,
renders all files, and publishes a staged sibling directory. A generation error
preserves the previous site and leaves the successfully extracted snapshot
available for another attempt. Commands never generate from a stale snapshot
after failed extraction.

The [snapshot schema](snapshot-schema.md) documents portable records, canonical
fingerprints, read-only loading, and active output validation. The loader reuses
the execution record validator with bytes verified in memory. Decoding cannot
manufacture rendering trust. Hidden and unselected output alternatives undergo
the same validation as visible alternatives.

Preview serves an immutable in-memory generation while a new attempt runs.
Changes become visible only after the new output tree has been published.
Startup, execution, reference, rendering, or publication errors leave the prior
served generation intact. The watcher compares content fingerprints, debounces
changes for 200 milliseconds, and remembers failed attempts so it does not
repeatedly execute unchanged failing inputs. Its initial observation precedes
the initial build, preventing an edit during startup from becoming an unnoticed
baseline. Configuration changes update the declared roots. Missing assets and
inputs remain observable so repairing them can trigger recovery.

The watcher includes configured metadata, Python and R extraction inputs,
authored pages, their local assets, assets referenced by extracted documents,
and declared environment files. It excludes the selected output directory,
`.diplodocus`, `.git`, and unrelated files. Generated snapshots, temporary
storage, and output assets do not trigger another execution. Ctrl-C and SIGTERM
request cooperative cancellation and wait for kernel cleanup before exiting.

## Evidence

The deterministic lifecycle tests remain in `execution::jupyter`:

- Deadline tests reject already-expired ready work and synchronous overruns.
- `ongoing_output_cannot_extend_cell_or_terminal_deadlines` covers missing
  reply, missing idle, and missing both while output continues.
- `the_remaining_cell_deadline_bounds_either_terminal_order` checks which
  absolute deadline wins in either terminal-message order.
- `public_engine_enforces_startup_and_terminal_deadlines` exercises the public
  engine, including process cleanup and removed connection directories.
- `public_engine_bounds_unresponsive_shutdown_before_returning_success`
  verifies bounded escalation and reaping of an unresponsive kernel.
- Python and R engine tests verify state, rich output, and cleanup through the
  public interface.

`tests/preview.rs` runs the real preview operation and makes HTTP requests. The
Python test publishes a successful site with a generated image, changes the
source to a cell that exceeds its deadline, and observes the running kernel.
It checks continued HTTP availability during execution, then verifies that the
kernel PID and connection directory disappear, the next cell never runs, every
published file stays identical, and the database bytes stay identical. A valid
edit produces a new served generation. A declared environment edit triggers
another execution, while output, storage, and unrelated edits do not. The test
also requests cancellation during a later executing cell and verifies cleanup.
The static preview test covers missing-asset recovery, asset edits, changed
collection roots, and source deletion and restoration.

`tests/commands.rs` compares combined builds with separate extraction and
source-independent generation, checks disabled execution, rejects overlapping
destinations, and preserves earlier publication after source and route errors.
`tests/site_generation.rs` checks output replacement and resolves every local
HTML link, anchor, and image beneath a hosting prefix. `tests/snapshots_storage.rs`
covers source-free round trips, refresh and removal, corruption, independent
schema versions, WAL rejection, actively revalidated rich output, and forged
records whose integrity fingerprints have been recomputed.

Two regression tests failed before their fixes: API-only assets did not affect
watch observations, and WAL-mode databases were accepted. The corrected watcher
collects asset references from static API documents; the snapshot reader rejects
WAL headers before opening SQLite.

The full validation passed 607 tests and 25 doctests, plus formatting, Clippy,
and rustdoc with warnings denied:

```sh
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets --all-features && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

Command-line smoke checks also passed:

- The Python/R acceptance workspace produced identical logical snapshot records
  through `build` and separate `extract`. After deleting the source workspace,
  `generate` with an empty tool search path reproduced all 55 generated files
  and left the input database unchanged.
- The project's documentation configuration built its executable page and guide.
- Sending SIGTERM to `serve` during an executing Python cell reaped the kernel,
  removed its connection directory, exited successfully, and preserved every
  previously published site file.

The authored-document acceptance gate passed again after refreshing its golden
records for the updated command documentation.

After disabling unused SQLite default features, all 22 storage, site, command,
preview, and acceptance tests passed again, along with formatting, Clippy, and
rustdoc. `cargo audit` passed for the final lockfile. `cargo deny check licenses
sources` passed its source check but still rejected the same ten package
versions recorded in the [baseline license report](execution-m6-01-validation.md).
The new dependency introduced no additional license-policy failures, and
`deny.toml` remains unchanged. The dependency license check is still a failure.

Monotonic timeouts remain cooperative limits rather than preemption of
synchronous Rust work. Authored code runs with the user's permissions; process
group cleanup and asset transactions do not undo arbitrary side effects of that
code. Execution caching, a complete presentation feature gate, and the remaining
release gates retain their separate roadmap entries.
