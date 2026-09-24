# Page execution cache

The public Jupyter engine and workspace execution now implement the
[page cache contract](../spikes/page-execution-cache.md). `extract`, `build`, and
`serve` use `.diplodocus/cache/execution` beside the configuration file. Library
callers opt in with `JupyterEngine::with_cache_root`; construction performs no
I/O. Removing the cache forces fresh execution. Output destinations cannot
replace or enter this reserved cache boundary.

## Identity and lifecycle

Lookup follows current kernel readiness. The existing identity snapshot covers
original page bytes, normalized options, the engine executable and component
versions, normalized kernelspec and launch identity, current kernel-info reply,
platform, deadlines, and declared environment files. A hit still starts and
cleans up one kernel, but submits no cells. Cleanup and input revalidation must
succeed before restored results or assets become available.

The session supervisor watches cancellation and process exit while a blocking
worker reads and validates the candidate. The worker holds only read handles
and memory; it creates no staging files. On cancellation or child death, the
caller joins this worker before returning failure. This implements the candidate
isolation requirement without a second writable asset store. A miss or rejection
continues in the same ready session. The engine stages a fully validated hit
only after cleanup and input revalidation, using the existing page asset owner.
Staging failure remains fatal and cannot fall back to partial cached output.

A rejected candidate adds one `invalid-execution-cache` warning and executes the
whole page once. Failure of that execution fails the page. Producing warnings
retain ledger order and indices; current discovery and cache warnings remain
outside the artifact. In memory, a hit changes the execution origin to `cache`.
No local path, port, process ID, timestamp, or connection data is added to
portable execution provenance.

## Format and storage

The codec implements `page-execution-artifact-v1` with explicit field projection,
using the existing canonical codec and representation digests. It checks the
key and result digests before reading assets. Restore checks source evidence,
options, eligibility, warning references, output slots, MIME alternatives,
figure options, active HTML and Markdown policies, and the complete asset set.
Re-encoding the validated candidate must reproduce every manifest byte. This
also rejects unknown fields, omitted nulls, forged producer metadata, and
noncanonical sets at every depth. The original synthetic artifact round-trips
without changing its schema or fixture bytes.

All alternatives are validated, including hidden content and unselected MIME
representations. Cached images enter through digest, size, format, and active
image validation. Generated Markdown and HTML bind only verified digest assets;
restore never reopens a generated source-image path.

Directory descriptors and `O_NOFOLLOW` reject symlinks while keeping reads and
writes anchored to opened directories. Nonregular files, extra files, missing
assets, and malformed digest names reject the complete candidate. Reads are
bounded to 64 MiB per manifest and 512 MiB of combined image bytes. Entries that
exceed these limits are rejected; publication reports a cache-storage warning
without invalidating a successful execution.

Publication uses a nonblocking, per-key filesystem lock. It writes and flushes
a private staging directory, validates the staged payload, then atomically
renames the complete directory into place. Readers never inspect staging.
An existing valid entry remains immutable. A different valid result for the
same key reports `non-deterministic-execution`. Rejected destinations are removed
only under the lock. Failed optional storage leaves the successful page usable
and reports `execution-cache-unavailable`.

## Validation

The focused tests cover:

- The exact reference artifact, all representation kinds, hidden images,
  cross-cell display updates, slot gaps, warnings, allowed errors, and skipped
  cells.
- Forged source, options, attribution, digests, diagnostics, and assets, including
  mutations with recomputed result digests. Every reference-fixture object is
  tested for unknown fields and every nullable field for omission.
- Missing, extra, corrupt, truncated, symlinked, and nonregular entry files;
  incompatible schemas; absent caches without side effects; interrupted writes;
  failed storage; concurrent writers; lock contention; and nondeterministic
  results for the same key.
- Live protocol-fixture hits with zero submissions, rejection followed by fresh
  success or failure, producing-warning replay with discovery offsets, source,
  runtime, and environment invalidation, cancellation and child death during
  supervised work, and post-cleanup input and staging failures.
- Real Python and R builds with state, text, Markdown, HTML, and SVG output.
  Repeated and relocated builds restore the same portable page records and asset
  bytes, produce identical site files, and do not repeat a cell-side sentinel.
  Declared environment edits cause fresh execution.

Run the focused checks with:

```console
cargo test --locked --lib cache
cargo test --locked --test execution_cache
cargo test --locked --test execution_identity --test execution_artifact_contract
cargo test --locked --test commands
```

The independent command-authority and full Milestone 6 acceptance TODOs remain
separate gates. The cache does not provide offline reuse, remote sharing,
per-cell reuse, or a fingerprint of undeclared dependencies.
