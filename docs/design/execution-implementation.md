# Milestone 6 implementation boundaries

This decision freezes the division of work after
`5c56119ecbf74180731a6d0a0e4c2aeb32e07ea7`. It supplements the
[execution interface](../ir/authored-execution.md),
[output policy](../spikes/authored-execution-contract.md), and
[cache contract](../spikes/page-execution-cache.md). Those contracts retain their
requirements. The table below distinguishes implemented boundaries from their
remaining consumers. The initial shared change added the unsupported display
view, missing diagnostic names, an input-change failure, and dependencies.

## Evidence already in production code

| Area | Existing implementation and tests | Remaining boundary |
| --- | --- | --- |
| Engine records | `src/execution.rs`, `records.rs`, `failure.rs`; `tests/execution_contract.rs` | Portable records remain untrusted; the public engine returns immutable validated records and owned staging. Command dispatch and cache restoration remain separate. |
| Public engine | `src/execution/jupyter/engine.rs`; `tests/engine.rs` | Preparation, independent input declarations, observed launch/build/runtime, supervised reduction, cleanup, post-cleanup revalidation, and retention are composed without cache storage. Commands and site publication remain work. |
| QMD preparation | `documents::prepare_collection_document`; `tests/qmd_preparation.rs` | Reuse validation, origins, eligibility, source order, and exact source. Command authority remains separate. |
| Discovery and readiness | `src/execution/jupyter/discovery.rs`, `session.rs`; `tests.rs` | Production tests cover explicit selection, malformed/shadowed specs, authentication, readiness ordering, cancellation, process groups, and bounded cleanup. |
| Launch identity | `src/execution/jupyter/launch.rs`, `process.rs`; `tests/launch.rs` | Discovery binds the selected spec bytes to the immutable identity plan. The supervisor rechecks and spawns from that plan. The public engine snapshots declared inputs before startup and revalidates after cleanup. |
| Sequential execution | `src/execution/jupyter/execution.rs`, `page.rs`; `tests/pages.rs` | Production tests cover prepared bytes, terminal ordering, correlation, skipped/hidden cells, allowed errors, deadlines, and cleanup. Collected events are private and unvalidated. |
| Output reduction | `src/execution/jupyter/output.rs` and child modules/tests | Incremental typed slots, page-wide updates and clearing, live Markdown/HTML safety, inline and nested images, normalized errors, typed diagnostics, and final figure validation are implemented. The public engine preserves this evidence through checked final records and retention. |
| Execution assets | `src/execution/assets.rs` and child modules/tests; `tests/execution_assets.rs` | Image validation, boundary checks, deterministic namespaces, collision detection, cache-byte validation, rollback, and retention are implemented. The session has a supervised cell-consumption hook with protocol tests for incremental staging and stopping on fatal asset failure. |
| Real kernels | Startup and page-state tests plus `jupyter/tests/engine.rs` | Unconditional `python3` and `ir` tests exercise state and validated rich output through the public engine. Cache restoration remains work. |
| Fragment parsing | `documents::parse_markdown_fragment`; `tests/markdown_fragments.rs`; `jupyter/output/text.rs` | The reducer reuses inert parsing and attribution for Markdown MIME and adjacent as-is stdout, then validates URLs and stages nested images. Cache restoration remains a separate consumer. |
| Presentation | `src/rendering.rs`, `execution/figures.rs`; `tests/cell_presentation.rs` | Reuse visibility and final selected-figure counting. Tests supply records; they do not prove reduction or site rendering. |

`tests/jupyter_execution_spike.rs`, `tests/jupyter_real_kernels.rs`, and spike
snapshots supplement this evidence but do not replace tests through the public
production engine. `src/commands.rs` still returns not-implemented errors for
build, check, and serve. No literal command or watched-site guarantee follows
from those stubs.

## Ownership and scheduling

The contracts coordinator owns `Cargo.toml`, `Cargo.lock`, `src/execution.rs`,
shared execution records, shared IR, diagnostics, and this decision. Workers
request edits to those files through the coordinator. Module registration and
visibility changes are small reviewed integrations, not concurrent edits.

| Task | Exclusive implementation and test ownership |
| --- | --- |
| M6-02 | `src/execution/jupyter/output.rs` and its child modules/tests; reducer snapshots under `tests/snapshots/execution/output/` |
| M6-03 | `src/execution/assets.rs` and child modules/tests; `tests/execution_assets.rs` |
| M6-04 | `src/execution/identity.rs` and child modules/tests; `tests/execution_identity.rs`; existing reference vectors are immutable expectations |
| M6-05 | `src/execution/output_safety.rs` and child modules/tests; `tests/execution_output_safety.rs` |
| M6-06, M6-08 | Production engine and `jupyter/page.rs`, `execution.rs`, `session.rs`, and `tests/pages.rs`; request shared entry-point edits |
| M6-07 | `src/execution/cache.rs` and child modules/tests; `tests/execution_cache.rs`; no session-runner edits |
| M6-09 | `tests/milestone_six.rs`, acceptance snapshots, matrix, and execution documentation |

The lead serializes edits to `jupyter/tests/fixture.rs` and its dispatcher in
`jupyter/tests.rs`. Reducer unit tests construct private `CellEvent` records and
need no subprocess fixture. New protocol modes are integrated one at a time by
the engine owner. The Jupyter client's `test-kernel` feature remains a
development dependency; no additional production fixture transport is needed.

M6-01, M6-02, and M6-03 have landed. Follow the
[remaining-work plan](execution-remaining-plan.md) for the current schedule and
additional shared-interface decisions. M6-04 and M6-05 can proceed in parallel,
then M6-06 and M6-07 can run together after identity and safety acceptance. Only
the engine owner edits the session. M6-08 joins them, then M6-09 proves the public
execution core. M6-10a and M6-10b require real later command and publication
implementations before M6-11 can close the full gate. Submission is not
acceptance; the lead reviews, integrates, validates, and closes each task.

## Output reduction and validation

Keep the reducer inside `jupyter` so `CellEvent`, `MimeBundle`, display IDs, and
raw error data do not become public or serializable. Its narrow inputs are the
prepared request, ordered events for a completed cell, its `CellOutcome`, and a
representation validator. Its output is final `CellExecutionResult` records,
ordered diagnostics, and retained asset references. It neither discovers kernels
nor decides execution authority.

The reducer owns one page-wide display registry. It exposes an incremental
`accept_cell(prepared, outcome, events, validator)` operation and a finalization
operation. The engine invokes `accept_cell` within supervised execution, after
both terminal messages and before the next `execute_request`. The hook is
fallible and awaited under cancellation and kernel-liveness supervision. A fatal
asset or validation failure stops further submission and follows normal cleanup.
Converting the entire raw page after `execute_page` returns is insufficient.
Local image bytes are validated and staged before later cells can overwrite or
delete their source paths. The `execute_with` hook and asset protocol fixtures
now prove that a fatal boundary violation in the first cell prevents submission
of the second. Missing nested images in unselected Markdown/HTML alternatives
also stop submission and roll back staged assets. The public engine composes
this hook and the live validators with input revalidation and provenance.

Keep monotonically increasing slot counters per owning cell. Updates replace all
surviving registrations, including earlier cells, without changing slot, owning
cell, or original producing cell. Representation evidence identifies the current
producer; `updating_cell` identifies the latest replacement. Clear removes current
cell registrations and retains counter gaps. Deferred clear happens before the
next output event, including an update; no next output means no clear. An unknown
display ID adds a warning. Clearing or replacing a slot never removes warnings or
erases a fatal validation failure. Prune unreferenced accepted assets only when
the final surviving representations are known.

The validator boundary returns one of: an accepted representation with its
content evidence and warnings; a rejected candidate with warnings; or a fatal
`ExecutionFailure`. It receives a producing page/cell/slot origin and mutable
page asset staging. Both accepted and rejected results may carry diagnostics.
Fatal results are never MIME fallbacks. Validate every supported offered
candidate in policy order, including alternatives hidden by selection or cell
visibility. Keep diagnostic order stable before assigning indices. The reducer
can test this boundary with an injected validator while M6-03 and M6-05 proceed.
Do not publish an unused trait scaffold merely to make a test double available.

Ordinary streams and errors are escaped text. Only adjacent stdout events under
`output: asis` concatenate for fragment parsing; stderr, display, error, update,
and clear events break a run. Reuse the existing fragment parser. Error handling
coalesces protocol reports, removes terminal controls, normalizes known source
paths, and replaces external frame paths, while preserving ordinary authored text.
After all cells and display operations, call `validate_figure_options` before
visibility filtering and successful page construction.

### Unsupported output and schema compatibility

Do not add an unsupported variant to workspace-v1 `OutputRepresentation`, or
pretend rejected media was accepted `text/plain`. An unsupported display has:

- `CellOutputKind::Display`;
- empty `output.representations` and representation evidence;
- `selected_mime_type = None`;
- lexically ordered offered MIME names and nonempty diagnostic references.

`ExecutionOutput::unsupported_placeholder()` exposes this shape as a borrowed
`UnsupportedOutput`. It checks shape, not diagnostic bounds or rendering trust.
An empty bundle may have no offered MIME names. Streams and errors cannot acquire
this meaning from an empty representation list. The renderer shows escaped MIME
names, or an empty-bundle placeholder, through this view. The cache DTO maps the
view to the contract's explicit `unsupported` representation and computes its
structured content digest there. It reverses that mapping on restore. An HTML or
SVG rejection's specific warning suffices; do not add a redundant generic warning.

Workspace schema v1 and its `sanitized-html` wire spelling remain unchanged.
`HtmlCandidate` still contains `UnvalidatedHtml`. `PageExecutionRecord` is an
untrusted library record, not the `execution-result-v1` cache wire format. M6-07
owns explicit cache DTOs with the exact contract fields, tags, required nulls,
strict unknown-field handling, and structured diagnostic arguments. Serializing
`PageExecutionRecord` directly is not a conforming artifact. Field-meaning changes
to the cache contract require a new schema, not an adapter that silently changes
v1. This distinction also applies to fingerprints, options, provenance, and images.

## Assets and the fragment image bridge

M6-03 supplies a page-scoped staging owner. Its operations validate inline image
bytes, resolve and stage a generated local image, validate cached bytes against an
expected digest/media/size, and retain only the final referenced asset set. All
return existing portable `ExecutionAsset` records. Local paths stay in the owner
and `StagedExecutionAsset` handles. Byte validation is reusable without opening a
source path. Deduplication checks bytes and media metadata, including through an
injectable digest-collision seam in tests. Missing/escaping/nonregular local files
fail the page; invalid inline media can reject only that candidate.

The staging owner uses a private child of the page staging boundary and removes
it on failure or drop. Explicit rollback reports cleanup errors. Retention is a
consuming operation allowed only after kernel cleanup, input revalidation, and
final output validation; it transfers handles to the caller's existing
nonserializable `PageExecutionResult`. Do not create staging for disabled paths.
No operation accepts a kernel-supplied output filename. Content-addressed portable
paths use a deterministic page namespace derived from configured repository and
collection IDs and the normalized page path, independent of checkout or staging.
M6-03 must document that encoding with relocation/collision tests before acceptance.

`Inline::Image.target` is an authored string, not a typed digest. Preserve that
workspace-v1 meaning. M6-05 implements a separate nonserializable
`ValidatedMarkdown` wrapper, with private immutable blocks and an ordered map of
image-node addresses to typed `ExecutionAsset` records. A node address is a path
of named child edges and zero-based indices from the fragment block root, not a
URL, alt string, or byte span. Thus two images with identical targets or spans
still have separate bindings. Every image has exactly one binding; every binding
identifies an image. Recursive traversal covers nested blocks, table cells, and
inline children. The wrapper exposes immutable views only.

The live constructor takes the parsed fragment, authored anchor context, and the
asset staging owner. It checks inert structure and decoded URLs, stages image
bytes, rewrites each accepted image target to its portable `AssetReference.path`,
and records the typed binding. The raw parsed representation does not gain trust
from this rewrite. Only consumers holding the wrapper can use the bindings. The
renderer gets an image's published URL from its binding and the current asset
publisher, never by interpreting the target string. Unresolved semantic links
remain available for resolution in the current site context.

The execution cache encodes image nodes with typed digest/media/size references
in place of string targets; the original generated image path is absent. The
codec consumes `ValidatedMarkdown`, not arbitrary `MarkdownBlocks`. On restore,
a separate constructor takes the decoded DTO and a verified asset table. It
checks bindings, reconstructs inert blocks and portable targets, and validates
all structure and links without filesystem lookup of generated images. There is
no constructor that trusts a string merely because it resembles an asset path.
The active validator must produce the wrapper again after deserialization. The
representation content digest covers the canonical fragment DTO, including typed
image references, as specified by the cache contract.

HTML uses the same split. A private nonserializable `ValidatedHtml` owns an
allowlisted tree whose image nodes hold typed assets, rather than browser URLs.
The live constructor rejects internal asset schemes, remote images, active
elements/attributes, and obfuscated forbidden URLs. It drops only comments and
the explicitly discardable class/id/data attributes. Cache serialization writes
the exact `diplodocus-asset:sha256:<hex>` spelling; only the restore constructor
accepts it, exactly and without encoded aliases, against verified assets. It
rechecks the allowlist and emits canonical escaped markup before fingerprinting.
The renderer substitutes current published URLs from typed references. No
unchecked string, deserialized `HtmlCandidate`, or cache DTO constructs trusted
HTML. Keep parser DOM values within the synchronous validation step; do not
carry the `Rc`-based DOM across an awaited operation in a `Send` engine future.

## Identity, cache, and session ownership

M6-04 supplies restricted canonical encoding, strict decoding, domain-separated
structured hashes, key-input construction, and portable observations. Reuse
`provenance::fingerprint_bytes` for exact byte hashes. Consume explicit input
records; do not start kernels or store cache entries. A local input snapshot owns
page/environment bytes and private resolved launch identity. Its post-cleanup
revalidation returns `InputChanged` on byte, containment, or launch changes.
Failure must not automatically repeat authored execution.

The engine provides normalized selected-spec data, resolved executable and launch
data, and the current validated `KernelRuntime`. M6-04 may define local identity
input records without exposing `SelectedKernel` or making it serializable. The
engine adapter projects into them. The key includes exact component versions,
policies, platform/target, actual deadlines, full source, normalized defaults and
ordered cells, and sorted declared inputs. The existing key/encoding fixture is
the byte-for-byte oracle. A struct's derived Serde encoding is not that oracle.

M6-06 preserves the existing discovery/readiness split. After authority and
language checks, start one `KernelSession`; read its observed runtime before
calling its consuming `execute` method. M6-08 inserts lookup at this point. A
cache hit shuts down this session without sending any execute request, then
revalidates inputs. A miss executes in this same session with the fallible cell
reduction hook. Both paths await cleanup and keep primary and cleanup failures.
The ready-session wait must remain cancelable and supervised while cache work
runs. An asynchronous hook may not leave a ready kernel without an owner.

M6-07 exposes lookup and publication over explicit current key/input records,
the prepared request, validators, and page staging. A lookup returns miss,
rejected-with-one-warning, or a completely validated restored candidate. It does
not discover, start, execute, or shut down a kernel. A hit is provisional until
the engine has completed cleanup and input revalidation. Publication accepts a
complete successful validated result; storage warnings do not invalidate that
result. It owns cache locks, immutable directories, strict manifest/file-set
validation, atomic rename, and nondeterminism warnings. No partial reuse or stale
fallback is allowed. Asset restore consumes verified bytes, never source paths.
M6-06 can compile without storage and M6-07 can test without a session; M6-08
connects them without moving authority into the cache.

## Dependencies, policies, and diagnostics

These direct pins are shared by the output workers. The lockfile retains prior
dependency versions and adds only the selected dependency closure.

| Role | Selection |
| --- | --- |
| Base64 | `base64 = 0.22.1`, default `std`; use strict standard decoding |
| Raster decoding | `image = 0.25.10`, defaults disabled, only `jpeg` and `png`; strict validation calls the existing codecs directly through pins `png = 0.18.1`, `zune-jpeg = 0.5.15`, and `zune-core = 0.5.3` |
| SVG XML | `roxmltree = 0.21.1`, default `std` and `positions`; reject DTDs, entity declarations, and processing instructions |
| SVG values | `svgtypes = 0.16.1`, default `std`; parsing is subordinate to the finite-value/keyword/paint allowlist |
| HTML parsing | `html5ever = 0.39.0` and `markup5ever_rcdom = 0.39.0+unofficial`; Cargo requirement `=0.39.0` omits semver build metadata |
| URL decoding/parsing | `percent-encoding = 2.3.2` and `url = 2.5.8`, default `std`; inspect decoded scheme/path before normalization can hide traversal |

Keep `panache-parser = 0.29.2`, `jupyter-protocol = 2.0.2`, the production
`jupyter-zmq-client = 1.0.1` Tokio feature, and the locked Tokio version. The HTML
sanitizer, SVG validator, and raster policy validator are Diplodocus adapters,
each recorded at the crate version and covered by the executable digest. Record
parser and decoder roles separately, including the underlying PNG/JPEG codecs
and SVG-value parser. The active identifiers remain `qmd-mvp-v1`,
`execution-mvp-v1`, `mime-mvp-v1`, `html-mvp-v1`, and `svg-mvp-v1`. This decision
does not change their allowlists or MIME order.

The shared diagnostic additions are `invalid-cell-output`, `unsafe-kernel-html`,
`unsafe-kernel-svg`, `execution-input-changed`, `invalid-execution-cache`,
`execution-cache-unavailable`, and `non-deterministic-execution`. Input changes
have a distinct fatal `ExecutionFailureKind::InputChanged`. Existing asset and
cleanup failure kinds retain their meanings. Specific HTML/SVG warnings survive
safe fallback; source-asset boundary and missing-file errors never fall back.

## Execution acceptance map

The rows map implementation to acceptance for TODO.md Milestone 6. Reducer,
fragment parsing, and asset implementation are now checked in that roadmap;
their public-engine acceptance remains part of the later tasks. This table does
not mark the full milestone complete.

| Roadmap requirement | Implementation | Required acceptance |
| --- | --- | --- |
| Typed streams/errors/displays/updates/results | M6-02, M6-06 | Protocol-order and cross-cell reducer tests, then public engine snapshots in M6-09 |
| Escaped streams and isolated Markdown/as-is | M6-02, M6-05 | Adjacent stdout boundaries, inert fragments, URL/image checks, M6-09 snapshots |
| Content-addressed binary figures and boundaries | M6-03, M6-06 | Media validation, traversal/symlink/collision/rollback tests, stop-before-next-cell fixture |
| Sanitized HTML and faithful safe fallback | M6-05, M6-02 | Active/obfuscated markup, alternative order, typed image restoration, visible placeholders |
| Timeouts, interruption, reaping, last watched site | Existing lifecycle plus M6-06/M6-08 | M6-09 rechecks integrated cleanup; M6-10 proves last-site retention through real publication |
| Complete page cache and validated assets | M6-04, M6-07, M6-08 | Exact vectors, corrupt/adversarial artifacts, current-runtime hits, input changes, atomic publication |
| Executed/cache origin and portable provenance | M6-04, M6-06, M6-07 | Relocation and private-data exclusions; hit changes only origin, M6-09 |
| Never and every check path avoid side effects | M6-08; real command integration in M6-10 | Library sentinels in M6-09 plus actual check/disabled command evidence; stubs do not count |
| Unit and Python/R end-to-end acceptance | M6-02 through M6-09 | Public engine, stateful Python/R pages, rich output, timeouts, missing kernels, deterministic cleanup |

M6-11 independently reviews every checkbox and the literal exit gate. M6-09 may
establish an execution-core gate while M6-10/11 remain blocked, but cannot reduce
or mark complete the full milestone gate.

## Validation record

The baseline and assigned-worktree commands, selected policies, results, and
access diagnostics are recorded in
[the M6-01 validation ledger](execution-m6-01-validation.md). A blocked documented
environment is not a passing baseline. Direct toolchain checks are supplemental;
they do not substitute for the declared devenv and its Python/R kernels.
