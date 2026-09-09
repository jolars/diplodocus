# Page execution-cache contract

## Outcome and scope

The MVP caches one complete QMD page's structured execution results and accepted
asset bytes. The cache key covers the source, effective options, execution
implementation, selected kernel and runtime, output policies, and declared
environment inputs. A hit restores every cell together, preserving stateful
execution and cross-cell display updates.

This is the Milestone 2 cache decision. It extends the [authored-execution
contract](authored-execution-contract.md), whose authority, output-safety, and
cleanup rules apply on hits as well as misses. Milestone 3 will implement the
serialized types; Milestone 6 will implement storage and restore. No execution
cache exists in the current CLI.

Use key schema `page-execution-key-v1`, artifact schema
`page-execution-artifact-v1`, and canonical encoding `execution-json-v1`.
Changes to field meanings, normalization, or encoding require a new schema.
There is no migration, partial reuse, stale-result fallback, or shared/remote
cache in v1. Users can force fresh execution by removing the local cache.

## Canonical bytes and fingerprints

`C(value)` is the following restricted JSON encoding, implemented independently
of serializer map iteration order:

- Encode valid Unicode as UTF-8, without a BOM, insignificant whitespace, or a
  trailing newline. Do not normalize Unicode or line endings inside strings.
- Object keys are unique ASCII strings sorted by unsigned byte order. Duplicate
  or unknown fields are invalid. Arrays retain their specified order.
- Emit `null`, `true`, and `false` literally. Numbers are unsigned 64-bit
  integers written in decimal without leading zeros. Negative numbers,
  fractions, exponent notation, and nonfinite values are not permitted.
- Escape quote and backslash as `\"` and `\\`. Encode every U+0000 through
  U+001F character as a lowercase, four-digit `\u00xx` escape, including tab and
  newline. Emit all other characters literally, including `/`, U+2028, and
  U+2029. Reject unpaired surrogates and invalid UTF-8.
- Required fields are always present. Nullable fields use explicit `null`, and
  empty arrays/objects remain `[]`/`{}`. Defaults are materialized before
  encoding. Arbitrary YAML values and raw Jupyter JSON do not enter this format.

Every fingerprint is the string `sha256:<64 lowercase hexadecimal digits>`. For
original source, submitted cell source, environment files, and binary assets,
hash the exact bytes with SHA-256. Do not prepend a domain string to these
content digests. For structured values, define:

```text
H(domain, value) = "sha256:" + lowercase_hex(
    SHA256(UTF8(domain) || [0x00] || C(value))
)

page_key = H("diplodocus/page-execution-key-v1", key_input)
result_digest = H("diplodocus/page-execution-result-v1", result)
```

The NUL byte separates the domain from the canonical value; it is not the two
characters `\0`. A representation's `content_digest` covers its content,
excluding the digest itself, provenance, and selection metadata. Hash literal
text and accepted HTML as UTF-8 bytes, and assets as their file bytes. Hash
structured content (Markdown blocks, error records, or unsupported MIME lists)
as `H("diplodocus/execution-representation-v1", {kind, content})`.

[The key vector](fixtures/execution-cache-key-v1.json) contains synthetic
inputs, the complete canonical UTF-8 string, and the expected page key. Its
component versions are examples, not additional dependency selections. The
included encoding vector exercises key ordering, Unicode, control characters,
nulls, and arrays. These are reference data for the later Rust implementation.

## Key input

`key_input` is an object with exactly the fields below. Nested records use the
listed field names. All versions are exact strings, never version ranges.

  | Field                | Canonical value                                                                                                                                                                                                                                                                                                                                                                                                   |
  | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `schema`             | `"page-execution-key-v1"`.                                                                                                                                                                                                                                                                                                                                                                                        |
  | `schemas`            | `{encoding, artifact, ir}` with values `"execution-json-v1"`, `"page-execution-artifact-v1"`, and `"execution-result-v1"`. All serialization compatibility participates in identity.                                                                                                                                                                                                                              |
  | `page`               | `{repository, collection, path, working_directory, format, source_digest}`. IDs are configured IDs; `format` is `"qmd"`; paths are relative to that repository. Hash the entire original page, including prose, frontmatter, comments, and line endings.                                                                                                                                                          |
  | `options`            | `{mode, page_veto, defaults, cells}`. `mode` is `"execute"`, `page_veto` is `false`, `defaults` contains all five effective document defaults, and `cells` is the ordered list defined below.                                                                                                                                                                                                                     |
  | `engine`             | `{id, version, build_digest}`. ID is `"jupyter"`, version is the Diplodocus crate version, and `build_digest` hashes the running Diplodocus executable bytes. This also invalidates unreleased implementation changes sharing a crate version.                                                                                                                                                                    |
  | `policies`           | `{qmd, execution, mime, html, svg}` with the active policy IDs from the authored-execution contract.                                                                                                                                                                                                                                                                                                              |
  | `components`         | Records `{role, name, version}` sorted by `(role, name, version)`. Include the authored and fragment parsers, Jupyter transport and protocol crates, Tokio, HTML parser and sanitizer, SVG parser and validator, and raster decoder and validator. Record each role even when one crate supplies several; reject duplicate roles. Built-in adapters use the Diplodocus version and are covered by `build_digest`. |
  | `kernel`             | `{name, spec_digest, launch_digest, search, interrupt_mode, runtime}`. Normalize the selector to lowercase ASCII. `search` is the ordered list of `{class, ordinal, selected}` searched locations, with exactly one selected location. Classes are `jupyter-path`, `user-data`, `system-local`, and `system`; ordinals are zero-based within each class.                                                          |
  | `platform`           | `{os, architecture, target}`. OS is `"linux"` in the verified MVP; architecture and full Rust target triple come from the running Diplodocus build.                                                                                                                                                                                                                                                               |
  | `deadlines_ms`       | `{startup, cell, terminal_sync, interrupt, shutdown, termination, forced_exit}`. V1 values are respectively `30000`, `60000`, and five `5000` values. Injected test limits use their actual values.                                                                                                                                                                                                               |
  | `environment_inputs` | Records `{repository, path, digest}` sorted by `(repository, path)` byte order. These are the collection's explicitly declared files; an absent declaration is `[]`.                                                                                                                                                                                                                                              |

Each `options.cells` record is
`{ordinal, language, eligible, submitted_source_digest, effective}`. Include
every authored `CodeCell`, even when skipped or of another language, using
zero-based source-order ordinals, including nested cells. `language` uses the
execution contract's normalization. `eligible` means it will be submitted after
language selection and effective `eval`; a false value uses
`submitted_source_digest: null`. For eligible cells, hash the exact bytes that
will be sent, after removing fence delimiters and the option preamble, without
an additional newline or whitespace normalization.

`defaults` has `eval`, `echo`, `output`, `include`, and `error`. Each cell's
`effective` additionally has `label`, `fig-alt`, `fig-cap`, and `fig-subcap`.
Use the validated booleans or `"asis"`, explicit null for absent strings, and an
ordered string list for subcaptions. Resolve fence labels and option precedence
before hashing. Source ranges, declaration origins, and overridden declarations
belong to result provenance; the original source digest already covers them.
Equivalent configuration defaults produce the same key, but edits to the page
bytes invalidate it even when effective options are unchanged.

### Paths and declared files

Represent paths using `/` separators and repository-relative components, with no
empty, `.` or `..` components; the repository root itself is `"."` only for
directory references, including the working directory and launch segments.
Require valid UTF-8 and retain case and Unicode spelling. Normalize configured
paths lexically, then resolve symlinks to verify containment. Store the
normalized declared path, not the resolved absolute path. Reject absolute paths,
escapes, missing/unreadable files, nonregular files, and duplicate normalized
declarations. Directories and globs are not environment inputs in v1. Input list
order does not affect the key.

Do not use mtimes, inode numbers, checkout locations, Git dirty flags, or Git
revisions as substitutes for content. A relocation with the same IDs, relative
paths, source, and runtime identity has the same key. A file rename or different
collection ID changes it. URL mounts, navigation, themes, repository display
metadata, and search settings are outside this cache: resolve references and
render again using the current site model.

Read source and declared files into a stable input snapshot. Re-read their
bytes, containment, and launch identity after kernel cleanup, immediately before
accepting a hit or committing a fresh result. A changed input fails this attempt
with `execution-input-changed`; discard staging and let a later rebuild start
from a new snapshot. Do not automatically repeat authored code within the failed
attempt. This detects ordinary concurrent edits; it does not snapshot undeclared
file reads by code.

### Kernel, launch, and environment identity

Compute `spec_digest` as `H("diplodocus/execution-kernelspec-v1", spec)` where
the private normalized record is `{argv, language, interrupt_mode, env}`. Apply
the validated default interrupt mode and language normalization. Sort
environment entries by name in an array of `{name, value}` records. JSON
whitespace, object order, display name, and kernelspec presentation metadata do
not affect this digest. Unsupported behavioral extensions must be rejected, not
silently excluded from identity.

Compute `launch_digest` as `H("diplodocus/execution-launch-v1", launch)` with
private record `{spec_digest, executable, argv, env, working_directory}`.
`executable` is `{location, digest}`, covering the resolved executable's
identity and exact file bytes. Resolve a bare command through the active `PATH`
before hashing. The launch argument and explicit environment vectors reflect the
command actually used. The working directory is `{repository, path}`. Never hash
the random connection filename, connection contents, ports, or authentication
key.

For both private records, encode each argument/environment value as an ordered
array of tagged segments: `{kind: "literal", value: string}`,
`{kind: "repository", repository: id, path: relative_path}`, or
`{kind: "connection-file"}`. Recognize `{connection_file}` only at its validated
substitution positions. Replace known repository-root prefixes only at path
component boundaries, choosing the longest root and then the lexicographically
smallest repository ID for aliases. Do not expand variables or interpret shell
syntax. The segment tags distinguish literal text from substituted paths.

An executable inside a declared repository has location
`{kind: "repository", repository, path}`. For an external executable, location
is `{kind: "external", path_digest}`, hashing the UTF-8 canonical absolute path.
External installation moves conservatively miss even with identical executable
bytes. Symlink retargeting changes the resolved identity. Hashing a launcher
does not fingerprint its whole dependency tree; declared environment inputs
remain necessary for libraries, kernel packages, and data dependencies.

Only the two resulting fingerprints leave these private records. Do not
serialize argument vectors, external paths, or environment values into the
artifact, site, or diagnostics. Fingerprints are not secret encryption; the
execution cache is private local build data and is not published with the site.
Inherited environment values are excluded. Explicit kernelspec overrides are
included, but there is no implicit environment-variable allowlist or arbitrary
runtime version probe.

`kernel.runtime` contains the exact validated `kernel_info_reply` fields
`{implementation, implementation_version, language, language_version, protocol_version}`.
Normalize only the language alias; retain version strings exactly, including
protocol minor version. Do not copy runtime identity from a previous result to
decide whether that same result is reusable.

## Lookup and kernel lifecycle

1. Parse and validate the current page and collection. `check`, mode `never`, a
   page veto, no braced cells, or all cells having `eval: false` bypass the
   execution cache and kernel discovery entirely. After authorized spec
   selection, no matching eligible cells also bypasses cache access and launch.
2. Snapshot declared inputs, discover the configured spec statically, and
   resolve its executable. Start the one page-owned kernel with the normal
   startup deadline and establish readiness and validated kernel info. Compute
   the complete key only now, with the current runtime versions.
3. Look up exactly that key. On a candidate hit, validate the entire artifact
   and asset set as below. Do not send any `execute_request`. Shut down and reap
   the kernel, revalidate inputs, and only then expose restored results with
   `origin = cache`. A startup or cleanup failure fails the page even if a
   matching artifact exists.
4. On a miss or rejected candidate, submit eligible cells in the same fresh
   session in source order. Finish output conversion, bounded cleanup, and input
   revalidation. Only a successful complete page, including allowed language
   errors, can become a new cache entry with producing origin `executed`.

A cache hit therefore saves cell execution, not kernel startup. Starting a
kernel can run kernel initialization code with the user's privileges, so this
path remains behind execution authority. Offline reuse without an installed
kernel is outside v1. No warm-kernel pool or remembered runtime-version index
may weaken current-identity verification.

## Artifact format

The default local root is `.diplodocus/cache/execution/` relative to the project
configuration directory. It is distinct from the output directory and must be
ignored by `serve` input watching. For a key `sha256:<hex>`, use:

```text
.diplodocus/cache/execution/
  v1/sha256/<hex>/
    manifest.json
    assets/sha256/<asset-hex>
  staging/<private-unique-name>/
  locks/sha256/<hex>
```

Assets have no extension and no kernel-provided filename. The media type in the
manifest controls later publication. `manifest.json` contains exactly
`C({schema, key, key_input, result_digest, result})` with schema
`"page-execution-artifact-v1"`. The key is recomputed from the embedded
`key_input`; `result_digest` covers `result` independently because different
authored executions can produce different bytes for the same inputs.

`result` has exactly `{ir_schema, provenance, cells, diagnostics, assets}`.
`ir_schema` is `"execution-result-v1"`, a Diplodocus-owned schema to implement
in Milestone 3. It serializes the complete logical fields in the execution
contract under these constraints:

  | Field         | Stored form and invariants                                                                                                                                                                                                                                                                                                                                                                                                                            |
  | ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `provenance`  | The authored page and producing engine, kernel, environment, and policy records, with `origin = "executed"`. Identity fields agree with `key_input`; source declarations and their origins retain repository-relative UTF-8 byte ranges. Restoring changes only the in-memory origin, never this file.                                                                                                                                                |
  | `cells`       | One record per authored cell in increasing ordinal order, including skipped cells. Each has `ordinal`, `label`, `span`, `source_segments`, `submitted_source_digest`, `effective`, `option_origins`, `outcome`, `skip_reason`, and `outputs`. Outcomes are `ok`, `allowed-error`, or `skipped`; only skipped cells have a nonnull reason and they have no outputs. Use `eval-false` or `language-mismatch`, with language mismatch taking precedence. |
  | `diagnostics` | Ordered portable execution/output diagnostics with stable local indices, severity, code, structured message arguments, source ranges, and related ranges. Replay these warnings on a hit; regenerate current parse, configuration, and kernelspec-discovery diagnostics. No failed-attempt diagnostics or timing measurements are cached.                                                                                                             |
  | `assets`      | Records `{digest, media_type, byte_size}` sorted by digest; one record and one file per distinct accepted asset. Include assets used by hidden output and safe MIME alternatives. Every referenced asset exists, and every listed asset is referenced. Duplicate digests with inconsistent bytes or media types are invalid.                                                                                                                          |

Each output is an explicitly tagged `stream`, `display`, or `error` record with
the execution contract's owning, producing, and latest-updating cell ordinals,
stable slot ordinal, offered MIME names, accepted representations, selection,
and diagnostic indices where applicable. Number slots per owning cell in
creation order, retaining gaps after clearing. Store final slots after all
updates and clears in slot order, including replacements in earlier cells.
Jupyter message, display, session, and execution-count identifiers are absent.
Raw event logs and superseded display payloads are not artifacts.

Representations use tags `text`, `markdown`, `html-candidate`, `asset`, or
`unsupported`. Text contains literal stream/display text or structured error
name, value, and traceback lines. Markdown contains isolated, inert fragment
blocks with cell-relative source attribution and unresolved semantic links. HTML
contains the producing sanitizer's accepted string and policy/version records,
but its tag is deliberately untrusted on disk. Assets contain digest references,
media types, and sizes. Unsupported placeholders contain MIME names and
diagnostic references, never rejected payloads. Retain all accepted MIME
alternatives in preference order, even when presentation hides them.

Markdown image nodes carry typed asset references. In stored HTML, the output
converter rewrites each accepted local image `src` to
`diplodocus-asset:sha256:<asset-hex>`. This exact internal spelling is allowed
only in cache representation data, never in authored or kernel-provided URLs.
The restoring converter validates its digest against the verified asset table
and reconstructs a typed asset reference before applying the ordinary HTML
allowlist. Reject malformed, encoded, or unbound internal references. The
renderer supplies the current page's published asset URL; the internal scheme
never reaches the browser. Fingerprint the stored markup after this rewrite.

Portable IR contains no floating-point numbers or arbitrary metadata objects;
authored literals stay strings. Fragment-local ranges point to the producing
cell and fragment, without synthetic files. Store no resolved site URLs,
rendered pages, raw source/staging paths, or trusted HTML constructor state.
Exact Rust enum definitions and the shared fragment/diagnostic wire types belong
to Milestone 3; changes incompatible with these invariants require a new schema.

### Validation and restoration

Treat every candidate as untrusted serialized input, even in a local cache:

1. Require supported schemas and canonical encoding, exact key equality with the
   current input, matching key/result digests, and no unknown fields or
   variants. Reject truncated JSON, duplicate keys, malformed digest names,
   missing/extra files, and symlinks or nonregular files anywhere in the entry.
2. Validate ordinals, outcome/eligibility agreement, source bounds and UTF-8
   boundaries, option/provenance equality with the current parse, diagnostic
   references, slot ordering, MIME ordering/selection, and all asset references.
   No successful result may contain an execution failure disguised as a cell
   outcome. Allowed errors still require that cell's effective `error: true`.
3. Read asset files using digest-derived paths under the entry, verifying size,
   content digest, actual media format, and active SVG/image validation.
   Revalidate every cached HTML candidate with the active sanitizer before
   constructing trusted IR. Recheck generated Markdown's inert structure and
   URL/asset boundary rules. A changed candidate, invalid representation, or
   newly unsafe payload rejects the whole entry, even if another MIME survives.
4. Restore accepted assets into page-scoped staging from those validated bytes.
   All generated local image references must already be digest references; never
   reopen their original generated files. Resolve retained semantic links
   against the current site context later. A successful restore yields the same
   portable cells, diagnostics, and asset bytes as its producing execution.

A missing entry or incompatible schema is a normal miss. Corruption, unsafe
payloads, or an unreadable candidate produces one `invalid-execution-cache`
warning with a stable reason and page source, then a fresh page execution under
the existing authority. Do not silently repair a subset or copy rejected bytes
to publication. If that execution fails, the page fails with no stale fallback.
Hashes detect damage and mismatched inputs, not the authorship of a cache entry;
no authenticity claim or remote-cache trust is implied.

### Publication and concurrency

Create a private staging directory under the cache root on the same filesystem.
Write all validated assets and the manifest, flush and close them, and
atomically rename the complete directory into its immutable key location only
after input revalidation and successful kernel cleanup. Readers inspect final
entries only. Remove staging after failure; abandoned staging from a crash is
never a hit. Cache storage failures produce `execution-cache-unavailable`
warnings and leave the successfully executed page usable without a cache entry.

Serialize publication for a key with a local exclusive lock, including removal
of a rejected candidate. Use a nonblocking lock attempt; contention skips this
optional cache write without delaying the build or affecting its result.
Revalidate an existing destination under the lock. If another writer has
installed a valid entry, keep it and discard the new cache staging; the current
build still uses its own fresh result. Different valid result digests for the
same key emit `non-deterministic-execution` warnings. Never combine assets or
results across runs. The key describes the declared inputs, not a promise that
authored code is deterministic.

Cache commit and site publication are separate transactions. A page failure
commits neither its result nor its assets. A complete successful page entry may
remain if a different page or later site validation fails. The site publishes
only when the whole build succeeds, and `serve` retains the last successful site
after failure. Cache origin and cache-storage warnings belong to build
provenance/diagnostics; they must not make rendered page bytes differ on a hit.

## Alternatives and implementation gates

Reject per-cell caches because later cells depend on the same session and may
update earlier outputs. Reject source-only or kernelspec-only keys because they
miss options, runtime upgrades, output policies, and declared dependencies.
Reject cached kernel-info shortcuts because an unchanged launcher can load a
different runtime or kernel package. A future environment identity proven
without startup could support a separate policy, but lockfile contents alone do
not establish one.

Reject generated Markdown, rendered HTML pages, pickle, and raw notebook/event
dumps as artifacts: they lose typed provenance, bake in site state, or expose
runtime-dependent and unsafe representations. Use inspectable canonical JSON and
separately hashed assets. Do not cache failures or reconstruct kernel memory;
cached output cannot reproduce filesystem or network side effects.

Before marking the Milestone 6 cache implemented, write failing tests for:

- The reference vectors, object-order independence, configuration-default
  normalization, input-list reordering, and checkout relocation; source bytes,
  ordered cells/lists, paths/IDs, every option, executable/spec/env change,
  runtime/component/policy/schema version, platform, and deadlines must each
  exercise key invalidation.
- Authority bypasses with zero discovery, startup, cache access, or asset
  writes; startup/cleanup on hits with zero submitted cells; runtime upgrades
  under the same spec; complete stateful Python and R result restoration.
- Updates to earlier cells, clearing, allowed errors, skipped and hidden cells,
  warning replay, and safe MIME alternatives/figures surviving source-asset
  deletion; site-context changes resolving links again without re-execution.
- Truncated or noncanonical manifests, wrong hashes/versions, forged ordinals,
  missing/extra/corrupt assets, symlink escapes, unsafe HTML/SVG/Markdown, and
  rejected candidates followed by successful or failed fresh execution.
- Changed inputs during execution, cancellation and every cleanup failure,
  interrupted staging writes, concurrent readers/writers, storage failure,
  nondeterministic same-key results, and preservation of the last good site.

The encoding vectors and this contract define expected behavior; they are not
evidence that the future cache or its safety boundaries have been implemented.
