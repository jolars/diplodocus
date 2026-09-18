# Snapshot storage and future incremental rendering

This note expands the [snapshot contract](../../DESIGN.md#sqlite-snapshot).
Storage and publication belong to the initial design. Incremental rendering is
deferred, but its requirements inform the records preserved today.

## Record storage

SQLite persists the typed documentation IR. Extractors return structured values,
and the core owns their storage. Top-level entities should be queryable by
semantic ID; nested documents, signatures, and language extensions may use
versioned serialized values instead of a table for every syntax node. A
canonical text export supports golden fixtures and readable comparisons.

Store local asset bytes by content fingerprint and reference them from the IR.
Generation writes those assets to the output tree. Source locations remain
repository-relative evidence, not files generation must open. Built-in theme
assets ship with Diplodocus. A future custom-theme facility must specify how its
assets travel with the snapshot before claiming the same portability.

The storage schema has an explicit version alongside the IR schema. Readers and
writers reject unsupported versions with a diagnostic; the initial
implementation does not migrate old snapshots automatically. Generation
validates records, asset fingerprints, paths, and references before constructing
the site model. Serialized HTML conveys no rendering trust and must pass the
active sanitizer policy again.

## Fingerprints and refreshes

Content fingerprints use a versioned canonical encoding of semantic records,
preserving meaningful order while sorting unordered collections. Database layout
and transient build metadata do not participate in those fingerprints. Logical
snapshot equivalence depends on records and asset contents, not SQLite file
bytes.

Stable IDs match retained entities across refreshes. Removed packages, items,
pages, relationships, and unreferenced assets disappear from the new snapshot;
refreshing does not accumulate duplicates. Source files and configuration remain
authoritative. Manual database edits are not preserved, and refreshes do not
append historical snapshots. The initial implementation may replace the whole
snapshot; skipping unchanged extraction work is a later optimization.

## Publication

A refresh publishes the complete workspace atomically, using a transaction or
replacement of a completed temporary database. Readers see one coherent
snapshot, and a failed refresh leaves the previous successful snapshot intact.
`build` and `serve` report a failed refresh rather than generating from stale
records.

Published snapshots are standalone files with no dependency on a live journal or
write-ahead log. If extraction uses a live database internally, it must publish
a consistent standalone copy, following SQLite's [snapshot and backup
rules](https://www.sqlite.org/backup.html).

## Provenance

Record each repository's canonical URL, revision, declared-input fingerprint,
and dirty state when available, together with each package's extracted version
and declared relationships. Local checkout roots never enter portable records.

For executed content, also record the engine, kernel, kernel-reported language
and version, normalized cell options, source fingerprint, declared environment
fingerprints, relevant toolchain versions, and whether outputs came from fresh
execution or the page-level cache. The [authored-execution
contract](../spikes/authored-execution-contract.md) and [page execution-cache
contract](../spikes/page-execution-cache.md) define those fields in detail.

A snapshot records obtained results. Storage idempotence does not make authored
execution deterministic or free of side effects. Generation cannot establish
whether absent source checkouts have changed since extraction.

## Future incremental rendering

Stable entity IDs, per-entity content fingerprints, and structured references
let a future generator determine which outputs depend on changed documentation.
A changed database file alone does not imply that every page has changed. Full
extraction and incremental rendering are independent choices.

A future incremental generator keeps a disposable build manifest alongside the
generated output, separate from the input snapshot. It records input
fingerprints, dependencies, and emitted paths for pages and shared outputs.
Dependencies include referenced entities, package navigation, concepts, and
search data. The generator also accounts for renderer and sanitizer versions,
theme assets, and effective presentation settings, falling back to a full render
when its previous state is missing or incompatible.

For example, changing a function's documentation may affect its reference page
and search entry, while renaming a package can affect navigation throughout the
site. Deleted entities and changed URLs require removing obsolete output files.
Selective generation must produce the same files as clean full generation from
the same snapshot and settings. Dependency tracking and invalidation remain
future work; SQLite does not supply them automatically.
