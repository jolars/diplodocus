# Execution implementation map

This page describes the implemented execution path. The [authored execution
policy](../spikes/authored-execution-contract.md), [cache
contract](../spikes/page-execution-cache.md), [library
interface](../ir/authored-execution.md), and [snapshot schema](snapshot-schema.md)
hold the detailed rules. Source and tests establish current behavior.

## Command path

`check` assembles static Python, R, and authored sources, then resolves semantic
and local references. It does not execute authored cells or publish output.
`extract` also executes authorized QMD pages and publishes a portable SQLite
snapshot. `build` generates a site from a completed snapshot and publishes it
through a staged directory. `serve` keeps the last successful generation live
while it watches inputs, rebuilds, and optionally refreshes browsers. The command
entry points are in [`src/commands.rs`](../../src/commands.rs), with the build
pipeline in [`src/commands/pipeline.rs`](../../src/commands/pipeline.rs) and the
watcher in [`src/commands/preview.rs`](../../src/commands/preview.rs).

Workspace assembly and execution live in [`src/assembly.rs`](../../src/assembly.rs).
Reference and local asset resolution live in [`src/validation.rs`](../../src/validation.rs).
Snapshot publication and read-only loading live in
[`src/snapshots.rs`](../../src/snapshots.rs). Site generation lives in
[`src/site.rs`](../../src/site.rs). A failed extraction does not publish a new
snapshot or site; a failed generation preserves the previous site.

## Page execution

The caller authorizes each page and command before invoking the public
`ExecutionEngine` interface. On Linux, `JupyterEngine` discovers the selected
kernel, starts one supervised session per page, and submits eligible cells in
source order. It checks source and declared inputs again before returning.
Cleanup is bounded; failure and cancellation discard uncommitted page assets.
The engine is in [`src/execution/jupyter/`](../../src/execution/jupyter/) and its
public types are in [`src/execution.rs`](../../src/execution.rs).

Output reduction preserves ordered slots, updates, clearing, and MIME
alternatives. Markdown, HTML, images, and downloads cross active validation
boundaries before rendering. A successful page returns immutable validation
evidence alongside portable records and staged assets. Portable records can be
serialized, but their deserialization does not establish output safety. The
renderer and cache restore path use checked values and verified assets rather
than treating a portable record as trusted content. See
[`src/execution/output_safety.rs`](../../src/execution/output_safety.rs),
[`src/execution/validated.rs`](../../src/execution/validated.rs), and
[`src/execution/assets.rs`](../../src/execution/assets.rs).

The local page cache is `.diplodocus/cache/execution` beside the configuration
file. It keys a whole page on source, effective options, engine and kernel
identity, output policies, and declared environment inputs. A hit still checks
current kernel readiness and inputs; it restores validated output without
submitting authored cells. Invalid entries cause a warning and a fresh run.
Removing the cache forces fresh execution. The implementation is in
[`src/execution/cache.rs`](../../src/execution/cache.rs).

## Evidence and maintenance

The principal execution tests are [`tests/milestone_six.rs`](../../tests/milestone_six.rs),
[`tests/execution_cache.rs`](../../tests/execution_cache.rs), and the tests under
[`src/execution/jupyter/tests/`](../../src/execution/jupyter/tests/). The broader
[`tests/fixtures/acceptance/MATRIX.md`](../../tests/fixtures/acceptance/MATRIX.md)
maps behavior to fixtures. The real Python and R kernel tests run through the
devenv environment. The [browser workflow](../development/browser.md) covers
site and preview checks.
