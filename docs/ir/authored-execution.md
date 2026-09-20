# Authored execution interface

The `diplodocus::execution` module defines the Rust library boundary for authored
page execution. It implements the interface and kernel startup portions of
Milestone 6 and follows the
[authored-execution policy](../spikes/authored-execution-contract.md). It
includes an internal Linux adapter for static kernel discovery, authenticated
startup, and bounded shutdown. Page preparation, the `ExecutionEngine`
implementation, option enforcement, output conversion, and caching remain later
work.

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
typed effective options with their winning origins. Preparing these records is
separate work; their public constructors do not establish authorization.

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

The Linux adapter separates static discovery from startup so the future page
executor can skip a page whose cells do not match the selected language before
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
by this foundation, and CLI commands do not dispatch to it yet.

The [session tests](../../src/execution/jupyter/tests.rs) cover injected discovery
environments, a controllable subprocess protocol fixture, cancellation and dropped
futures, signal and message interruption, descendants, forced shutdown, private
connection permissions, and cleanup failures. They also start and stop the
declared Python and R kernels through the production adapter without submitting
code. Page results and complete execution provenance remain subsequent work.

## Options and outcomes

`ExecutionDefaults` contains the five inheritable options. Their defaults are
`eval = true`, `echo = true`, output shown, `include = true`, and `error = false`.
`EffectiveCellOptions` adds an optional label, figure alt text, figure caption,
and ordered subcaptions. `OutputVisibility` distinguishes shown output, hidden
output, and as-is stdout. Each `EffectiveOption` retains its value and origin:
policy default, document declaration, inline option, hashpipe declaration, or
fence identifier. Authored origins carry page-relative UTF-8 ranges. Raw and
overridden declarations remain in the request's original `CodeCell`.

The types do not parse options, resolve precedence, validate labels, or suppress
output. `error = true` permits only ordinary language exceptions; it cannot
override transport, timeout, cancellation, asset, or cleanup failures.

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
references without retaining rejected payloads. Representations remain in MIME
preference order; offered MIME names use lexical order.

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
