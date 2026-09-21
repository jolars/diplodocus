# Authored execution interface

The `diplodocus::execution` module defines the Rust library boundary for authored
page execution. It implements the interface, kernel startup, and sequential
session portions of Milestone 6 and follows the
[authored-execution policy](../spikes/authored-execution-contract.md). It
includes an internal Linux adapter for static kernel discovery, authenticated
startup, sequential submission of prepared cells, and bounded shutdown. The
`diplodocus::documents` module validates QMD options and prepares cells without
I/O. Shared presentation views apply visibility options, and final-output
validation checks figure counts. An internal incremental reducer collects typed
outputs through a representation validator. Rich-output validators, supervised
reducer integration, the public `ExecutionEngine` implementation, site rendering,
and caching remain later work.

The [Milestone 6 implementation boundaries](../design/execution-implementation.md)
freeze module ownership, validation and cache seams, dependency choices, and the
remaining acceptance gates. They distinguish planned APIs from implemented code.

## Engine and caller responsibilities

`ExecutionEngine` is an object-safe, `Send + Sync` trait. Its `capabilities()` and
`requirements()` methods return declarative records without filesystem access,
kernel discovery, or runtime probes. Capability sets describe implemented
languages, output media, and execution features. Requirements describe supported
operating systems and the protocol major version required of the explicitly
selected kernel. Neither method verifies that a runtime is installed.

The asynchronous entry point is:

```rust,ignore
fn execute_page<'a>(
    &'a self,
    context: ExecutionContext<'a>,
    request: &'a PageExecutionRequest,
) -> ExecutionFuture<'a>;
```

`ExecutionFuture` is a boxed, `Send` standard-library future returning
`Result<PageExecutionResult, ExecutionFailure>`. The existing
`configuration::ExecutionEngine` remains the configuration selector enum;
`execution::ExecutionEngine` is the implementation interface. No plugin
registration mechanism is introduced. Tokio and the pinned Jupyter crates are
production dependencies; the client's `test-kernel` feature remains confined to
development builds.

Before dispatch, the caller validates collection authority, the QMD page veto,
and every option declaration, then prepares all authored cells in source order,
including nested cells. Each `PreparedCell` retains the original `CodeCell` and
typed effective options with their winning origins. The preparation API below
produces these records; their public constructors do not establish authorization.

Checks, collections with `mode = "never"`, vetoed pages, and pages with no
potentially executable cells must bypass the engine entirely. The engine
discovers only the explicit kernel selector and matches cells against its
verified language. If every candidate has a different language, it returns
skipped cells without launching a kernel or inventing execution provenance.

An executing engine owns one page session, submits cells sequentially, preserves
session state, and completes bounded cleanup before returning. A success returns
only after the kernel has exited and been reaped. The caller then owns any
retained staged assets and must publish or discard them. Failure discards staging
and returns diagnostics without a page result or publishable asset handles.

`ExecutionContext` contains local repository, page, and staging paths, phase
limits, and a cancellation future. The engine runs in the page's parent
directory, reads generated assets within the repository boundary, and stages
accepted assets within the separate page output boundary. The session adapter
rechecks the canonical repository and page paths before launch. Asset handling
remains future work; the context constructor performs no path checks.

Completion of the cancellation future requests interruption followed by cleanup.
The caller continues awaiting execution until that cleanup finishes. Dropping the
execution future is not a supported cancellation mechanism. The internal session
supervisor also handles unexpected drops of startup futures and session handles.
The Tokio runtime must remain alive until cleanup finishes.

## Internal Jupyter sessions

The Linux adapter separates static discovery from startup so the internal page
runner can skip a page whose cells do not match the selected language before
launching a kernel. Discovery reads only specifications matching the explicit
case-insensitive selector. It follows the documented `JUPYTER_PATH`, user, and
system precedence, deduplicates roots, diagnoses shadowed matches, and rejects
same-directory case ambiguity. A malformed or unreadable selected specification
fails without falling through to another kernel. No discovery command or
language runtime is launched.

The adapter validates arguments, language, interrupt mode, and literal environment
overrides. It rejects variable expansion, unknown top-level fields, kernel
provisioners, and transport-encryption extensions. It resolves the executable
through the captured build `PATH` before applying the kernel's environment
overrides. Relative executable paths and relative `PATH` entries resolve from the
page's parent directory. It substitutes `{connection_file}` within arguments and
invokes the argument vector directly. Local launch records remain private.

Each supervised process group receives five loopback ports, a fresh random
authentication key, and a mode-0600 connection file in a mode-0700 temporary
directory. Startup connects all channels and requires a valid protocol-major-5
kernel-info reply plus its matching IOPub idle message. Repeated information
requests recover from initial subscription delays within one startup deadline.
The adapter accepts either order of reply and idle, ignores unrelated messages,
and works with kernels that do not send `iopub_welcome`. It never sends an
`execute_request` during startup.

Shutdown and cancellation await bounded cleanup. Cancellation uses the selected
interrupt mode; shutdown escalates from a control-channel request to process-group
termination and kill when needed. The supervisor reaps the kernel, closes the
channels, and removes the connection directory before completing. Cleanup errors
remain separate from the original failure. Dropping a handle wakes the supervisor
instead of abandoning the child. No execution assets or cache entries are created
by this adapter, and CLI commands do not dispatch to it yet.

The [session tests](../../src/execution/jupyter/tests.rs) cover injected discovery
environments, a controllable subprocess protocol fixture, cancellation and dropped
futures, signal and message interruption, descendants, forced shutdown, private
connection permissions, and cleanup failures. They also start and stop the
declared Python and R kernels through the production adapter without submitting
code.

The internal page runner takes a `PageExecutionRequest`, preserving prepared
cell order, original source bytes, and authored ordinals. The caller supplies
nested cells in that same order. The runner rejects inconsistent ordinals,
overlapping or reversed source ranges, existing outputs, and absent page
authority before discovery. Empty pages and pages with every effective `eval`
disabled need no discovery. After discovery, cells with a different normalized
language remain skipped, and a page with no matching executable cells needs no
session.

Each executing page starts a fresh session and submits one `execute_request` at
a time. Submission uses the fixed execution policy, including disabled stdin.
The runner advances only after both the matching shell reply and IOPub idle,
in either order. Cell and terminal synchronization deadlines are monotonic;
kernel death, cancellation, input requests, and protocol failures stop further
submission. Disallowed language errors and aborted replies also stop the page.
An allowed language error retains the current session for later cells. Cleanup
completes before either success or failure returns, including cancellation
during shutdown.

The runner retains ordered kernel events in private, nonserializable records.
These records contain unvalidated MIME data, display IDs, and raw error details.
They cannot serve as `PageExecutionResult` or renderer input. Unrelated and late
messages produce source-attributed `unsupported-kernel-message` warnings instead
of being attached to the active cell. The reducer described below converts these
events into typed `CellOutput` nodes. Wiring it into supervised execution,
producing portable provenance, and implementing the public `ExecutionEngine`
boundary remain subsequent work.

The [page tests](../../src/execution/jupyter/tests/pages.rs) use the QMD preparer
and check exact submitted bytes, nested source order, skipped cells, both terminal
arrival orders, parent correlation, allowed errors, failure attribution,
deadlines, cancellation, and dropped futures. Hidden cells still submit with
`silent = false`, collect streams and errors, and obey the effective error
policy. The real Python and R tests retain definitions and imports
across three cells and repeat each page to prove that state does not carry into
another session.

### Incremental output reduction

The private `jupyter::output::OutputReducer` accepts one completed cell at a time
and finalizes the surviving output slots after the last cell. It preserves stream,
display, result, and error order. A page-wide registry replaces every surviving
slot for a display ID, including slots in earlier cells. Updates retain the
original owner, producer, and slot number, while recording the latest updater
and current representation producer separately. Raw display IDs stay private.

Immediate clearing removes only the current cell's slots and registrations.
Deferred clearing waits for its next output, including an update, and expires
when that cell completes. Slot numbers keep gaps after clearing. Unknown or
missing update IDs produce source-attributed warnings without creating slots.

A validator closure receives each supported MIME candidate in the policy's
fixed preference order, with its metadata and producing page, cell, and slot.
It can capture mutable asset staging. Accepted representations retain content
fingerprints and policy evidence. Rejected candidates retain warnings. Fatal
validation errors stop reduction, including for unknown display updates, and
prevent later finalization. Hidden output and lower-priority alternatives still
pass through validation. Unsupported bundles produce payload-free placeholders
with offered MIME names and diagnostic references.

The implemented validator accepts plain-text strings and arrays of strings.
Other supported media require the later fragment, asset, and HTML validators;
the plain-text validator rejects them with a warning. Ordinary streams retain
their literal bytes as preformatted text. As-is stdout parsing remains the next
output step. Errors lose terminal controls, known checkout frame paths become
repository-relative, and external frame paths and IPython execution counts use
stable markers. Ordinary exception messages and source text retain authored
paths. The existing transport coalesces shell and IOPub reports of an exception
before reduction.

Finalization checks figure options before presentation and returns the surviving
asset references. Clearing and replacement never erase validation warnings.
The returned cells and diagnostics are internal reduction results, not a
publishable page: asset validation, retention after cleanup, and execution
provenance remain the engine's responsibility. The engine must invoke reduction
before submitting the next cell so validation can stop execution in time.

The [reducer tests](../../src/execution/jupyter/output/tests.rs) cover ordering,
cross-cell updates, clearing, MIME preference and fallback, malformed payloads,
fatal validation, asset references, hidden figure counts, skipped cells, and
portable error text. The protocol page tests also reduce shell-only, IOPub-only,
and duplicate-channel error reports into one typed error.

## Options and outcomes

`ExecutionDefaults` contains the five inheritable options. Their defaults are
`eval = true`, `echo = true`, output shown, `include = true`, and `error = false`.
`EffectiveCellOptions` adds an optional label, figure alt text, figure caption,
and ordered subcaptions. `OutputVisibility` distinguishes shown output, hidden
output, and as-is stdout. Each `EffectiveOption` retains its value and origin:
policy default, document declaration, inline option, hashpipe declaration, or
fence identifier. Authored origins carry page-relative UTF-8 ranges. Raw and
overridden declarations remain in the request's original `CodeCell`.

Constructing the option types does not validate them or suppress output.
`error = true` permits only ordinary language exceptions; it cannot
override transport, timeout, cancellation, asset, or cleanup failures.

### Preparing a collection document

`documents::prepare_collection_document(source, collection)` returns a
`PreparedDocument` containing `parsed: DocumentParse` and
`preparation: Option<QmdPreparation>`. Invalid collection execution configuration
is a returned error. Authoring errors remain in `parsed.diagnostics` and prevent
preparation. GFM has no QMD preparation. Valid QMD pages retain preparation even
when their collection disables execution or their metadata vetoes it.

`QmdPreparation` contains document `defaults`, `page_veto`, source-ordered
`cells`, and preliminary `execution_eligible`. Eligibility requires collection
authority, no page veto, and at least one cell with effective `eval: true`.
It does not inspect the selected kernel or determine which cell languages match.
It never authorizes a check command to execute. Future command dispatch must
apply its own gate before calling the engine.

`parse_collection_document` uses the same validation and returns only the
`DocumentParse`. `parse_authored_document` remains the permissive syntax reader.
Both collection entry points preserve the original document, declarations, and
source segments. Neither reads declared environment files, discovers kernels,
executes source, or accesses execution caches and assets.

Validation checks supported metadata and every option declaration, including
overridden declarations and disabled cells. It derives effective values using
hashpipe, inline, document, then policy-default precedence. Duplicate options in
any tier are errors; `ambiguous-cell-option` replaces the parser warning with
one error and related declaration ranges. Malformed YAML retains
`invalid-embedded-yaml`. Unsupported options and fence classes use
`unsupported-cell-option`; invalid values, labels, and label conflicts use
`invalid-cell-option`. Authority declarations retain their existing grouped
diagnostic without redundant value errors.

Scalar validation distinguishes literal booleans from quoted strings and rejects
YAML tags, anchors, aliases, and merge keys. String options retain literal text;
quoted escapes and block scalar indentation, folding, and chomping are decoded
without altering authored IR. Labels use the explicit `label` option or the
fence identifier, which must agree when both are present. They must be unique
among cell labels and other authored anchors retained in the document.
Bare chunk labels are unsupported; use `#identifier` or `label` instead.

Preparation validates figure-option types but does not count figures before
execution. The runner applies evaluation and error options, while presentation
and final-output validation consume the prepared values separately. In particular,
preparing `echo: false` does not remove input from a collection with mode `never`.

The [preparation tests](../../tests/qmd_preparation.rs) cover precedence and
origins, nested cells, disabled pages, scalar types, label conflicts, diagnostics,
and Python and R preparation snapshots. They also prepare an eligible page with
an unavailable kernel and missing declared inputs without creating artifacts.

`CellOutcome` distinguishes `ok`, `allowed-error`, and `skipped`. Skipped cells
carry `eval-false` or `language-mismatch`, with language mismatch taking
precedence, and have no submitted-source digest or outputs. Entire pages that
bypass execution remain the caller's responsibility.

The phase limits are expressed in milliseconds: startup uses 30,000, cells use
60,000, and terminal synchronization, interruption, shutdown, termination, and
forced exit each use 5,000. Constructors and deserializers do not start timers
or enforce positive limits. The producer records actual configured limits,
including shorter test limits, without recording elapsed durations.

## Portable results and local assets

### Cell presentation and final figure validation

`rendering::present_prepared_cell(mode, cell)` presents a prepared cell when no
execution result exists. `rendering::present_cell(page, cell_result)` presents a
completed cell after all page-level display updates and clearing. Both return a
borrowed `CellPresentation` with optional source segments and a slice of visible
outputs. They retain the original source, options, and execution evidence.

The collection mode determines presentation policy. In `never` collections,
source remains visible regardless of execution options. In authorized
collections, `echo: false` hides source, `output: false` hides all output, and
`include: false` hides both. These rules also apply to skipped cells and vetoed
pages. `output: asis` shows converted output without changing its representation;
stdout fragment parsing belongs to the subsequent output-conversion step.
Allowed errors follow the same output visibility as other results. Neither view
grants rendering trust or replaces MIME, asset, HTML, or record validation.

`execution::validate_figure_options(page, cells)` checks final output slots before
publication. A nonempty `fig-subcap` list must match the number of selected SVG,
PNG, or JPEG asset representations. Each surviving slot counts once, including
repeated uses of the same asset. MIME alternatives and cleared slots do not
count. Skipped cells retain their options without figure-count validation.
An executed cell with no figures still fails when it declares subcaptions.

Mismatches return `ExecutionFailureKind::OutputValidation` with one
`invalid-figure-options` error per cell, in supplied cell order. Each diagnostic
points to the winning declaration and relates it to the owning cell, even if
another cell updated that output. `present_cell` runs the same validation before
applying visibility, so hidden output cannot conceal a mismatch. The future
output converter must also call the page-wide validator after applying all
updates and clearing, before constructing a successful page result.

The [presentation tests](../../tests/cell_presentation.rs) cover option precedence,
visibility combinations, skipped cells, unchanged evidence, selected figures,
final updated and cleared slots, repeated assets, and hidden figure errors.

### Records and staging

`PageExecutionResult` separates a serializable `PageExecutionRecord` from local
`StagedExecutionAsset` handles. The result envelope, staged handles, and runtime
context do not implement serialization. Portable assets contain an existing
`AssetReference`, validated media type, and byte size. Their paths are relative
to the snapshot and their fingerprints identify accepted content; local staging
paths appear only in the handles.

A record includes authored page evidence, effective defaults, all cell outcomes,
final output slots, diagnostics, and an asset table. The page working directory
is repository-relative; `None` denotes the repository root. Producers sort
assets by digest, retain every referenced accepted alternative, and list each
distinct asset once. Each staged handle must match an asset-table entry.

`ExecutionOutput` wraps the existing `CellOutput` without changing its schema.
It records owning, producing, and latest updating cell ordinals, stable output
slot ordinals, offered MIME names, selected MIME type, representation evidence,
and diagnostic indices. Display replacement preserves slot identity, and clearing
may leave gaps. Unsupported output can retain offered MIME names and diagnostic
references without retaining rejected payloads. Its explicit shape is a display
with empty representation and evidence lists, no selected MIME, and at least one
diagnostic reference. `ExecutionOutput::unsupported_placeholder()` exposes that
shape without changing shared output tags or validating cross-record references.
The separate cache DTO maps it to the contract's `unsupported` representation.
Representations remain in MIME preference order; offered MIME names use lexical
order.

Diagnostics have their final deterministic order before outputs refer to their
indices. Representation evidence has the same order and length as the accepted
representations. Source spans, source segments, and submitted-source fingerprints
refer to authored input; accepted-content fingerprints refer to output. Generated
Markdown retains the shared fragment attribution rules.

`PageExecutionProvenance` supplements the existing execution `Provenance` with
the producing executable digest, components keyed by role, policy identifiers,
launch fingerprints, search-location classes and ordinals, interrupt mode,
protocol observations, platform, and deadlines. The shared activity retains
engine and kernel identity, observed versions, environment fingerprints, and
`executed` or `cache` origin. Its tools include the producing Diplodocus and
engine versions. Cache restoration changes only that origin; absent observations
remain absent. A page on which no cell ran has no execution provenance.

These records are an additive library model, not the final
[execution-cache artifact encoding](../spikes/page-execution-cache.md).
Deserialization checks types but does not establish authorization, validate
cross-record invariants, or confer rendering trust. In particular, shared HTML
representations still contain `UnvalidatedHtml`, and cached markup requires
validation by the active sanitizer before rendering.

## Diagnostics and verification

`ExecutionFailureKind` identifies startup, protocol, input-request, cell,
timeout, cancellation, output, asset, and cleanup failures. Its diagnostic
conversion supplies a stable shared code, portable source, collection ownership,
and timeout phase without copying local runtime error strings. Related ranges can
be attached afterward. `ExecutionFailure` keeps primary diagnostics and additional
cleanup diagnostics separately so cleanup cannot hide the original failure.
Unsupported output normally produces a warning and placeholder, not a fatal
error merely because no supported MIME alternative survives.

The [contract tests](../../tests/execution_contract.rs) use an in-memory fake
engine and explicitly prepared fixtures. They cover trait-object dispatch,
asynchronous completion, cancellation input, cell outcomes, output attribution,
portable serialization, staging separation, provenance, and failure diagnostics.
Compile-fail examples protect the nonserializable local types. These tests do not
claim to verify real kernel execution, cleanup, option enforcement, sanitization,
or cache restoration.
