# Shared execution record checkpoint validation

This checkpoint binds prepared source and validated output for the future engine
and cache adapters. It does not implement production execution, cache storage,
command authorization, preparation anchor capture, or cleanup scheduling.

## Boundaries

`PreparedExecution` owns the original source bytes, request, and immutable
`AuthoredOutputContext`. Its crate-private constructor checks the source
fingerprint, page/context identity, working directory, and the accepted M6-04
`validate_prepared` checks without duplicating preparation policy. The caller
must supply the complete authored anchors captured during preparation. This
checkpoint does not independently prove arbitrary supplied anchors.

`ValidatedPage` owns its portable record and each final slot's validated values.
Lookup uses the final owning cell, slot, and representation index. The separate
`OutputProducer` records the current producing cell and authored span, plus an
optional producer slot. Live construction may supply that slot. Restoration
merges known Markdown or warning facts and preserves `None` when the artifact
contains no evidence for it. HTML retains no invented fragment or producer slot;
its content passes the same active restore validator through a crate-private
helper after the carrier checks producer identity. Fresh/cache equivalence uses
the portable record and validated assets, not equality of optional local facts. Slot gaps and
repeated producer slots remain intact. Markdown values retain their producing
origin, fragment ordinal, and original byte length. Repeated copies of an update
can share fragment identity; conflicting content under that identity fails.
Portable clones do not provide a way to mutate or reconstruct trusted values.

Checked construction compares every portable representation and digest against
its owned evidence, revalidates Markdown and HTML against the bound context, and
checks the exact sorted asset union. That union includes every direct and nested
asset in every surviving alternative, including hidden and unselected content.
The narrow `PageAssetStore::verify_assets` and `VerifiedAssets::from_store`
bridges require a matching page, healthy store, registered complete metadata,
unchanged contained staging bytes, and active image validation. Active staging
errors retain their `AssetError` type; adapters must roll back and preserve their
fatal failure codes and any cleanup failure.

`DiagnosticEvidence` separates current-build diagnostics from the typed
producing ledger. Construction checks their concatenated portable projection
and requires output indices to refer inside the ledger. Referenced warnings must
match the current producer, agree with known slot facts, and describe an offered
MIME candidate. A placeholder requires a relevant rejection, which may be
specific; an unrelated session warning cannot support it. Fragment identity is
consistent across retained wrappers and all warnings, including warnings for
superseded fragments. A missing diagnostic slot remains unknown and can agree
with a known slot. Cache consumers receive
the typed ledger and its checked offset, without parsing prose. Actual emission
order and index remapping remain the engine adapter's responsibility.

`PageExecutionResult` privately owns a `ValidatedPage` and retained staging.
Its crate-private retention operation consumes a live store, verifies its page
and metadata association, and delegates final byte checks and removal of
superseded assets to `retain`. Every fallible check precedes successful file
transfer. The engine must call this only after cleanup and input revalidation.
Public `RetainedExecutionAssets` parts cannot construct a result.

The carrier also enforces unique display MIME preference order, ASCII offered
MIME names, literal stderr and ordinary stdout, option-controlled as-is stdout,
and the absence of stream updates. A valid as-is rejection may retain literal
fallback text. Cell output provenance is exactly the ordered projection of owned
Markdown wrappers; missing, duplicate, foreign, or unrelated entries fail.
Page execution provenance remains on `PageExecutionRecord.provenance`. This
checkpoint does not validate runtime observations or reconstruct an event trace.

The storage-independent canonical adapter explicitly projects all six content
forms and every typed fragment node. The immutable reference artifact's eight
representations match its exact content and digest results. Safety has no new
identity or cache dependency. Workspace serialization and immutable fixtures
are unchanged.

## Regression evidence

Worker assignment: `ca-01M36MJFZ3QW7AAZG2J1VNA4DC`.
Base: `15d9c770737066a13294b3b831d5012a277b0a75`.
All worker commands used non-login Bash in:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36MJFZ3QW7AAZG2J1VNA4DC
```

Selected policy: filesystem `workspace-write`, network `deny`, approvals
`never`. No worker Git metadata writes or permission changes were attempted.
The repository has no additional `AGENTS.md`; supplied global instructions apply.

The initial documented baseline command
`devenv shell -- cargo test --locked --test execution_contract` passed all 16
original contract tests. Later shell entry failed at the Nix daemon. The direct
inherited Rust toolchain is supplemental evidence, not a substitute for the
complete documented development environment.

| Phase | Command | Outcome and diagnostic |
| --- | --- | --- |
| Initial scaffold | `cargo test --locked --lib execution::validated` | Expected compile failure: new types did not exist. This is not semantic RED evidence. |
| Association RED | `cargo test --locked --lib execution::validated` | 2 passed, 1 failed: the compiling permissive constructor accepted missing evidence. |
| Association GREEN | Same command | 3 passed after checked association was implemented. |
| Boundary RED | Same command | 10 passed, 2 failed: a specific HTML rejection was incorrectly rejected as placeholder evidence, and conflicting fragment identity was accepted. |
| Boundary GREEN | Same command | 12 passed after both guards were corrected. |
| Final focused | Same command | 14 passed, including active staging failure kind, diagnostic offsets, same producer/final owner separation, complete assets, source binding, and trait-object success construction. |

RED logs are `/tmp/diplodocus-shared-records-direct-red.log`,
`/tmp/diplodocus-shared-records-behavioral-red.log`, and
`/tmp/diplodocus-shared-records-boundary-red.log`. GREEN logs use the corresponding
`first-green`, `boundary-green`, and final `focused` names. The public fake-engine
success test moved to checked crate-unit construction. Public serialization and
biased cancellation tests remain. Meaningful compile-fail examples cover private
construction, immutable access, `Serialize`, and `Deserialize` for all three
opaque carriers, with passing immutable-access and portable-Serde controls.

## Initial frozen worker validation

The exact commands, working directory, selected policy, exit code, and log paths
are recorded in `/tmp/diplodocus-shared-final-ledger.json`. Direct-toolchain logs
are `/tmp/diplodocus-shared-final-<check>.log`.

| Check | Command | Worker outcome |
| --- | --- | --- |
| Format | `cargo fmt --all -- --check` | Passed. |
| Focused unit tests | `cargo test --locked --lib execution::validated` | 14 passed. |
| Related integration tests | `cargo test --locked --test execution_contract --test execution_representation_projection --test execution_artifact_contract --test execution_output_safety` | 39 passed: 15 contract, 1 projection, 6 artifact, and 17 safety tests. |
| Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed. |
| All targets | `cargo test --locked --all-targets` | Failed in the library suite: 64 passed, 26 failed. Runtime setup reported unavailable configured kernels and inability to reserve loopback ports. Later targets were not reached. |
| Explicit doctests | `cargo test --locked --doc` | 27 passed: 2 ordinary and 25 compile-fail examples. |
| Rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` | Passed. |
| Whitespace | `git diff --check` | Passed. |

Each required command was also attempted with `devenv shell --`, including the
focused filter. Rustdoc used
`devenv shell -- env 'RUSTDOCFLAGS=-D warnings' cargo doc --locked --no-deps`.
All six attempts were blocked before Rust ran: `Failed to create GC root` and
`cannot connect to socket at '/nix/var/nix/daemon-socket/socket': Operation not
permitted`. This is a Nix daemon access blocker, not a Git permission failure or
an observed `.devenv` write denial. Exact attempts are recorded in
`/tmp/diplodocus-shared-devenv-ledger.json` and the corresponding
`/tmp/diplodocus-shared-devenv-<check>.log` files.

## Coordinator validation of the initial freeze

The lead reported completed required validation in durable message
`cm-01M36P4XEWFHEE67T0KK2GR4YF`. Both runs used the exact assignment directory
above, non-login Bash, and the coordinator's separately approved
`require_escalated` development-environment access. The worker did not acquire
that access. The lead verified all 14 frozen hashes remained unchanged across
both runs; the worker subsequently read the logs to corroborate the report.

The first run, logged at
`/tmp/diplodocus-shared-coordinator-validation.log`, entered `devenv shell -- bash
-c` and ran these commands sequentially with stop-on-failure chaining:

```sh
cargo test --locked --lib execution::validated
cargo test --locked --test execution_contract --test execution_representation_projection --test execution_artifact_contract --test execution_output_safety
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
```

Focused tests (14), related integration tests (39), formatting, and Clippy passed.
The all-target run stopped with 89 library tests passing and one failing:
`execution::jupyter::tests::cleanup_errors_preserve_the_primary_failure` reported
`Protocol: The kernel exited during startup` at `tests.rs:400`. The command
returned 101; doctests and rustdoc were not reached. This remains recorded
failure evidence. This assignment predates the independent startup change;
neither that fact nor a successful rerun proves the cause of this failure.

The second run, logged at
`/tmp/diplodocus-shared-coordinator-validation-rerun.log`, used the same
`devenv shell -- bash -c` entry and stop-on-failure sequence:

```sh
cargo test --locked --lib execution::jupyter::tests::cleanup_errors_preserve_the_primary_failure -- --exact
cargo test --locked --all-targets
cargo test --locked --doc
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
```

It returned zero: the isolated test passed, all 515 all-target tests passed with
none failed or ignored, all 27 doctests passed, and rustdoc passed. There were
39 all-target result reports and two doctest reports. No further broad testing
is required unless review changes or failures justify it.

Independent review and the authorized commit handoff remain pending. The lead
owns integration and accepted task closure.


## Review corrections and final freeze

After the initial coordinator run, review identified diagnostic association,
fragment identity, MIME/stream shape, and output provenance gaps. The lead
approved bounded corrections in `cm-01M36P5Y7TN70S31ZRQ4CVHZD5`,
`cm-01M36P7XDV1YC5Z59FJF29BB8S`, `cm-01M36PAQ8HWCWCTGFA22X8CVJ6`,
and `cm-01M36PM9SANGB7B2QPCBABSXF1`. The last message also approved factoring
`output_safety/html.rs` to share its unchanged content validator without
requiring an invented producer slot. Public `restore_html` retains its signature
and origin checks. The final inventory has 15 intended paths.

The following worker checks used the same assignment directory, non-login Bash,
and unchanged `workspace-write`/network-denied/no-approval policy. They remain
supplemental direct-toolchain evidence.

| Phase | Command | Result and log |
| --- | --- | --- |
| Nullable-slot RED | `cargo test --locked --lib execution::validated::tests::fragment_diagnostic_slot_is_nullable_but_cell_and_source_rules_hold -- --exact` | 1 expected failure on the legal missing slot; `/tmp/diplodocus-shared-nullable-red.log`. |
| Nullable-slot GREEN | `cargo test --locked --lib execution::validated` | 15 passed, including missing-cell and wrong-source negatives; `/tmp/diplodocus-shared-nullable-green.log`. |
| Diagnostic/fragment RED | Same focused module command | 15 passed, 2 expected failures: unrelated warning accepted and conflicting warning/wrapper identity accepted; `/tmp/diplodocus-shared-review-red.log`. |
| Diagnostic/fragment GREEN | Same command | 17 passed; `/tmp/diplodocus-shared-review-green.log`. |
| Shape/provenance RED | Same command | 17 passed, 3 expected failures: reversed MIME order, nonliteral stderr, and duplicate provenance accepted; `/tmp/diplodocus-shared-shape-red.log`. |
| Shape/provenance GREEN | Same command | 20 passed; `/tmp/diplodocus-shared-shape-green.log`. |
| Final focused | Same command | 24 passed; `/tmp/diplodocus-shared-review-final-focused.log`. |
| Final related integration tests | Same four integration targets listed above | 39 passed, including all safety tests and eight immutable representation vectors; `/tmp/diplodocus-shared-review-final-contracts.log`. |
| Final Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed; `/tmp/diplodocus-shared-review-final-clippy.log`. |
| Final doctests | `cargo test --locked --doc` | 27 passed; `/tmp/diplodocus-shared-review-final-doctests.log`. |
| Final rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` | Passed; `/tmp/diplodocus-shared-review-final-rustdoc.log`. |
| Final formatting and whitespace | `cargo fmt --all -- --check`; `git diff --check` | Both passed. |

Additional positive tests cover shared warning indices on copied updates,
unknown producer slots for restored HTML/text/assets/placeholders, portable
fresh/cache equivalence despite optional local facts, known-slot conflicts,
warning-only superseded fragment identities, and both valid as-is stdout paths.
The earlier 515-test coordinator success applies to the prior frozen bytes.
Required validation and final independent review of these corrected bytes remain
pending before the commit handoff.

## Final transitive consistency correction

Review found a transitive case in the second freeze: a warning with an unknown
slot could connect a fragment known to use slot zero with an output known to
use another slot. The lead authorized only this correction in
`cm-01M36Q2MBS8V3DV5AGBBKNBEM1`. The checker now resolves connected components
of checked output and fragment facts. Each component either has one consistent
known slot or remains unknown; contradictory known facts fail regardless of
iteration order. Warning-only chains propagate the same facts, and unrelated
superseded warnings remain valid. This graph is temporary validation state and
does not alter the artifact schema or canonical representation content.

Under the same worker directory, non-login shell, and selected policy,
`cargo test --locked --lib execution::validated` first produced 24 passes and
two expected failures in `/tmp/diplodocus-shared-transitive-red.log`. One case
accepted the conflicting wrapper/warning/output chain; another failed to
propagate a known fact through a warning-only chain. After the correction, the
same command passed all 26 focused tests in
`/tmp/diplodocus-shared-transitive-green.log`. The regressions cover both wrapper
owner orders, both directions of a three-output/two-fragment chain, conflicting
known slots, and consistent nullable positives. `cargo fmt --all` completed,
and `git diff --check` passed. No further broad worker suite ran on these bytes.

The lead relayed the independent reviewer's source verdict in
`cm-01M36QBV8D69Y2S5DJN8GJ185H`, citing reviewer message
`cm-01M36QAEJTRSDFW5BRYRX74KJD`: no remaining findings. The final 15-path freeze
is the worker commit-handoff snapshot. At that handoff, final documented
environment validation and commit-bound review remain coordinator gates;
subsequent command outcomes and the confirmed commit are recorded in durable
handoff messages. The user narrowed the remaining run to closing existing work;
this checkpoint does not authorize engine or cache implementation.
