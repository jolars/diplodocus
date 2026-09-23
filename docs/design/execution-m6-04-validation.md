# M6-04 validation ledger

Date: September 23, 2026. Task: `ct-01M36D4QH88SRYDPBYTSC3TFBJ`.
Assignment: `ca-01M36FNAC4A27GGYTQXC4WYPF2`.
Base: `f14c60d692d6a1da31c3d1b10126e49384f59778`.

The original worker commands through the first review correction used non-login
Bash in this assigned worktree. The recovery section records a fresh assignment
and distinguishes its checks from this historical evidence:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36FNAC4A27GGYTQXC4WYPF2
```

The selected policy was `workspace-write`, network `deny`, approvals `never`.
No permissions, Nix/cache locations, or Git metadata access were widened. The
coordinator confirmed commit ownership before edits in
`cm-01M36FNYHBWTYZ6AVBZNMSWTR5`. Narrow shared edits were explicitly authorized:
`pub mod identity;` in `src/execution.rs`, and `build.rs` with the existing pinned
TOML parser as a build dependency (`cm-01M36FSAE6S80NVE5V7256YP4G`). No lockfile
upgrade or fixture edit occurred. Workers do not stage or commit this worktree.

## Environment evidence

| Command | Outcome | Diagnostic |
| --- | --- | --- |
| `pwd`; `git status --porcelain=v1`; `git rev-parse HEAD` | Passed | Correct assigned cwd, clean initial tree, exact base above. |
| `devenv shell -- cargo test --locked --test execution_identity` before adding the test | Failed, exit 101, after successful environment entry | No such test target; not an environment failure. |
| Same documented command after adding codec tests | Failed, exit 101 | Unresolved identity module in the preimplementation test; devenv hooks also reported that compile error. |
| Same documented command after initial codec | Failed, exit 101 | SHA-256 output needed explicit hexadecimal formatting; corrected. |
| `devenv shell -- cargo test --locked --test execution_identity > /tmp/m6-04-codec.log 2>&1` | Blocked, exit 1, before Cargo | Failed to create GC root; Nix daemon socket connection denied. |

The last attempt reported `Operation not permitted` connecting to
`/nix/var/nix/daemon-socket/socket`. It did not report a `.devenv` write failure
or a Git permission failure. Earlier successful environment entry does not
establish current availability. This blocker was sent durably in
`cm-01M36G0MZ2C2253Z0VT55WJN7V`; the coordinator agreed to run required checks in
the frozen assignment through separately authorized access
(`cm-01M36G1NY59QPHH0J44B428BK5`). No further environment retries were requested.

Required coordinator checks before commit and acceptance remain:

```console
devenv shell -- cargo fmt --all -- --check
devenv shell -- cargo clippy --locked --all-targets --all-features -- -D warnings
devenv shell -- cargo test --locked --all-targets
devenv shell -- cargo test --locked --doc
devenv shell -- env RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
devenv shell -- cargo audit
devenv shell -- cargo deny check
```

The dependency-policy checks apply to the approved build-dependency declaration,
even though it selects the same already locked TOML package.

## Supplemental validation and regression evidence

These commands used the same cwd and unchanged worker policy. They do not
replace required checks in the documented environment.

| Command | Outcome and evidence |
| --- | --- |
| `cargo test --locked --offline --test execution_identity` after codec/build implementation | Passed, 4 tests; `/tmp/m6-04-codec-supplemental.log`. |
| Same focused command with snapshot API stubs | Behavioral failure, 2 of 6 tests; successful capture/relocation rejected. `/tmp/m6-04-snapshot-red.log`. |
| Same focused command after snapshots | Passed, 6 tests; `/tmp/m6-04-snapshot-green.log`. An intermediate compile error used the wrong language field shape and was fixed. |
| `cargo test --locked --offline --test execution_identity strict_key_schema` | Behavioral failure for an unknown field at the root; `/tmp/m6-04-key-red.log`. |
| Focused suite after closed schema validation | Passed, 7 tests; `/tmp/m6-04-key-green.log`. |
| `cargo test --locked --offline --lib identity::launch::tests` | Behavioral failure: a slash before a connection segment was lost; `/tmp/m6-04-segments-red.log`. |
| `cargo test --locked --offline --test execution_identity --lib identity` | New segmentation test passed, existing Jupyter startup test failed; details below. `/tmp/m6-04-current.log`. |
| `cargo test --locked --offline --test execution_identity` with input-change coverage | Passed, 14 tests; `/tmp/m6-04-acceptance.log`. |
| `cargo test --locked --offline --test execution_identity public_launch` | Behavioral failure: lexical cleanup merged distinct materialized arguments; `/tmp/m6-04-launch-public-red.log`. |
| `cargo test --locked --offline --lib execution::identity` with root `/` case | Behavioral failure: root `/` was not recognized; `/tmp/m6-04-root-red.log`. |
| Focused integration and unit suites after literal suffix and root fixes | Passed, 15 integration tests and 1 unit test; `/tmp/m6-04-launch-public-green.log`, `/tmp/m6-04-root-green.log`. |
| `cargo test --locked --offline --test execution_identity missing_build_record` | Behavioral failure: missing root version panicked; `/tmp/m6-04-build-red.log`. Fixed checked graph access. |
| `cargo test --locked --offline --test execution_identity key_decoder_rejects_ranges` | Behavioral failure: requirement string accepted as exact version; `/tmp/m6-04-version-red.log`. Fixed shared exact-version validation. |
| `cargo fmt --all` and `cargo fmt --all -- --check` | Passed after formatting new code. Final check log `/tmp/m6-04-fmt.log`. |
| `cargo test --locked --offline --test execution_identity` on final implementation | Passed, 22 tests; `/tmp/m6-04-identity-final.log`. |
| `cargo test --locked --offline --lib execution::identity` | Passed, 1 unit test; `/tmp/m6-04-unit-final.log`. |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` | Passed; `/tmp/m6-04-clippy-final.log`. Earlier collapsed-if and test boolean-style warnings were fixed. |
| `cargo test --locked --offline --doc` | Passed all doctests, including both new no-Serde examples; `/tmp/m6-04-doc-tests.log`. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` | Passed; `/tmp/m6-04-rustdoc.log`. An initial unquoted `argv[0]` rustdoc link was corrected. |
| `git diff --check` | Passed. |

The overbroad `--lib identity` filter also selected
`execution::jupyter::tests::startup_rejects_invalid_protocol_identity_and_authentication`.
Its first `wrong-major` case failed at `src/execution/jupyter/tests.rs:149`:
actual failure kind `Startup`, expected `Protocol`. The assertion prints the
kind, not the full Startup diagnostic, so the underlying cause remains unknown.
The unchanged test needs the coordinator's full documented-environment run;
it was neither edited nor excluded from that required run. The worker reported
this limitation in `cm-01M36H0C4RQGHCT21T3833FJCT`.

Read-only inspection used `rg`, `sed`, `cat`, `wc`, and Git status/diff commands
on the contracts, implementation plan, source, fixtures, and build configuration.
Several initial probes named nonexistent `CONTRIBUTING*`, execution preparation
paths, `src/ir/document.rs`, or `rustfmt.toml`; subsequent reads used the existing
files. Those lookup errors were not validation-environment failures. No
repository `AGENTS.md` was found; the supplied global instructions applied.

## Scope and remaining integration

The identity code never owns a kernel, session, or cache directory. It accepts
explicit local facts, binds the prepared request by repreparing its source,
checks declarations against reread bytes, and exposes immutable observations.
The process adapter must use the same resolved executable, cwd, explicit
environment, and materialized arguments, revalidate immediately before spawn,
and call snapshot revalidation after cleanup. Those engine calls belong to M6-06.

The locked key fixture and complete artifact fixture remain unchanged. Tests
compare the actual snapshot projection to the complete synthetic key, check
both artifact hashes and every representation hash, exercise every key leaf,
and cover nested skipped cells, all effective options, preparation mutations,
relocation, sorted inputs, private debug output, source/launch/containment
changes, and root/connection segmentation. Build metadata records all eleven
required roles plus seven DOM/codec/URL components from selected lockfile edges.
The running executable digest is observed at runtime; Cargo supplies the full
target and target OS/architecture at build time.

This ledger does not claim public-engine execution, cache integration, full
Milestone 6 acceptance, or accepted task closure. Coordinator validation,
review, commit confirmation, clean-HEAD verification, integration, and acceptance
remain part of the authorized handoff.

## Coordinator checks and first review correction

The coordinator entered the documented devenv in this exact assignment using
separately approved `require_escalated` access and non-login Bash. This does not
change the worker's selected policy.

- Initial `cargo fmt --all -- --check` and
  `cargo clippy --locked --all-targets --all-features -- -D warnings` passed.
- `cargo test --locked --all-targets` failed in the lib suite: 74 passed and 2
  failed. `cancellation_uses_the_declared_signal_mode` and
  `pages::skipped_cells_do_not_submit_or_start_another_language` reported
  `Protocol` with `The kernel exited during startup.` The originally observed
  identity/authentication startup test passed. Log:
  `/tmp/diplodocus-m6-04-validation.log`.
- Both exact failing tests passed isolated documented-environment reruns with
  `--nocapture`. Log: `/tmp/diplodocus-m6-04-startup-rerun.log`.
- A second normal-parallel `cargo test --locked --all-targets` failed in the lib
  suite: 75 passed and 1 failed. The failure moved to
  `pages::asset_boundary_failure_stops_before_the_next_cell_and_reaps_the_kernel`
  with the same startup-exit diagnostic. The prior cases passed. Log:
  `/tmp/diplodocus-m6-04-validation-rerun.log`.
- Separate `cargo test --locked --doc` and
  `RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` passed. `cargo audit`
  reached its scan of 310 locked dependencies; the following `cargo deny check`
  failed its license policy (exit 4 for the combined command). The log ends
  `advisories ok, bans ok, licenses FAILED, sources ok`. This is not an
  environment-entry failure. Log: `/tmp/diplodocus-m6-04-remaining-checks.log`.
  The coordinator is investigating baseline policy evidence separately.

The worker's requested read-only investigation found that the parent discards
child stdout/stderr, and the startup supervisor does not retain exit status.
Fixture socket binding uses `unwrap`; a binding failure could produce the
observed exit. Port reservations are released before the child binds, but a
port race remains a hypothesis, not a confirmed cause or fix. Deixis definition
lookup failed with LSP `content modified` after its retry limit; standard source
reads established these findings. No lifecycle files were edited. The
coordinator assigned the startup investigation separately.

Independent review and worker self-review identified a further launch identity
collision for a declared repository root `/`: `/tmp/input` and `//tmp/input`
lost their distinguishing separator. The same applied to the literal fallback
for `/./tmp/input` versus `//./tmp/input`. Coordinator message
`cm-01M36JK5G1GRJTRG78CWX0MR9F` authorized reopening only launch normalization,
its focused tests, and this ledger.

The new public regression failed before the fix, showing identical spec digests
for differing materialized argv. The fix preserves the suffix verbatim when `/`
has already consumed the separator; literal fallback now retains the original
suffix for every root. The regression independently checks argv and explicit
environment changes, differing spec/launch digests, materialized values, and
rejection by the previous observation's `revalidate`.

| Command, same worker cwd and unchanged policy | Outcome |
| --- | --- |
| `cargo test --locked --offline --test execution_identity root_slash_preserves` | Failed as expected before the fix; `/tmp/m6-04-review-root-red.log`. |
| `cargo test --locked --offline --test execution_identity` | Passed, 23 tests; `/tmp/m6-04-review-root-green.log`. |
| `cargo test --locked --offline --lib execution::identity` | Passed, 1 test; `/tmp/m6-04-review-unit.log`. |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` | Passed; `/tmp/m6-04-review-clippy.log`. |
| `cargo fmt --all` and `cargo fmt --all -- --check`; `git diff --check` | Passed. |

Only `launch.rs`, `tests/execution_identity.rs`, and this ledger changed after
the initial freeze. All other handoff hashes and both reference fixtures remain
unchanged. Full validation, final review binding, and commit confirmation remain
pending; the passing focused checks do not erase either parallel-suite failure
or the dependency-policy failure.

## Second review correction and recovery

The source worker also corrected repository recognition that consumed an active
`{connection_file}` marker inside a declared root. The marker must remain an
argv substitution even when the same bytes belong to a repository path. Root
recognition now stops before the next active marker. Environment values and
`argv[0]` remain literal with respect to connection substitution.

The regression `active_connection_markers_take_precedence_over_repository_roots`
compares a root containing the marker with a relocated root. It verifies both
bare and `--config=` arguments, their materialized values, differing spec and
launch digests, and the environment-only control whose digests remain equal.
The historical red log `/tmp/m6-04-review-marker-red.log` records the expected
equal-digest assertion failure. The green log
`/tmp/m6-04-review-marker-green.log` records all 24 identity tests passing, and
`/tmp/m6-04-review-marker-clippy.log` records successful Clippy. Those logs belong
to the original worktree; they are not fresh-checkout validation.

The source assignment exited without a commit. Recovery assignment
`ca-01M36K64XPZ74FFT3V3KTNRTDE` starts at
`a39a7c2c7d729666c9a998f2f54ff939404a3ef9` in:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36K64XPZ74FFT3V3KTNRTDE
```

The continuation read the complete source `assignment_show` recovery handoff
before editing. Lead message `cm-01M36K6VMRGD5HSMSSRY77AZYR` confirmed separately
authorized commit ownership and permission to port the twelve intended paths.
The continuation copied the reviewed source files into its fresh worktree and
added the identity export alongside the integrated safety export. It preserved
the source files and index, verified by SHA-256 before and after the transfer in
`/tmp/m6-04-recovery-provenance.json`. Only `launch.rs`, the identity tests, and
this ledger differed from the original `/tmp/m6-04-freeze.json`. No source
worktree edit or Git write occurred. `Cargo.lock`, reference fixtures, and
lifecycle files remain unchanged.

All continuation commands use non-login Bash in the fresh cwd above with the
selected `workspace-write`, network `deny`, approvals `never` policy. Checks
outside devenv are supplemental and do not replace the required environment.

| Command | Outcome and diagnostic |
| --- | --- |
| `pwd`, `git status --porcelain=v1`, `git rev-parse HEAD` | Passed; fresh assigned cwd, clean starting tree, exact recovery base above. |
| `devenv shell -- cargo test --locked --test execution_identity` | Blocked, exit 1 before Cargo; `/tmp/m6-04-recovery-devenv.log`. Nix could not open its fetcher-cache lock because the filesystem was read-only. |
| `cargo test --locked --offline --test execution_identity` | Passed, 24 tests; `/tmp/m6-04-recovery-identity.log`. Includes both review corrections. |
| `cargo fmt --all -- --check` | Passed; `/tmp/m6-04-recovery-fmt.log`. |
| `cargo test --locked --offline --lib execution::identity` | Passed, 1 test; `/tmp/m6-04-recovery-unit.log`. |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` | Passed; `/tmp/m6-04-recovery-clippy.log`. |
| `git diff --check` | Passed after recovery and the ledger update. |

The exact blocked lock is
`/home/jola/.cache/nix/fetcher-locks/98e6c8e5ca59f95bc9707696420bc5ca1a82a1e4323a1b3baac9f73943984784.lock`.
The failure occurred while resolving the locked devenv-nixpkgs input. Unlike the
original worker's daemon-socket denial, this attempt failed at the fetcher cache.
It reported neither a `.devenv` write denial nor a Git permission error. The
worker neither relocated caches nor widened permissions. Durable message
`cm-01M36KAGHNZS5DFY7WXCWTFP1N` reports the current blocker to the coordinator.
The direct reviewer status message was rejected because workers lack
`send:reviewer`; the continuation routed the review request through the lead.

## Historical coordinator command record

Lead message `cm-01M36KBJMVKW9H1TP3TBQNR1HN` supplied these exact invocations.
Each used non-login Bash with separately approved `require_escalated` access to
the documented devenv. Environment entry succeeded for every invocation. The
first four ran in the original source assignment cwd given at the top of this
ledger, before the two final review corrections. Their outcomes remain
historical; they do not validate the recovered bytes.

```console
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-validation.log 2>&1; cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps && cargo audit && cargo deny check'
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-startup-rerun.log 2>&1; cargo test --locked --lib execution::jupyter::tests::cancellation_uses_the_declared_signal_mode -- --exact --nocapture && cargo test --locked --lib execution::jupyter::tests::pages::skipped_cells_do_not_submit_or_start_another_language -- --exact --nocapture'
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-validation-rerun.log 2>&1; cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps && cargo audit && cargo deny check'
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-remaining-checks.log 2>&1; cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps && cargo audit && cargo deny check'
```

The first invocation exited 101 after the 74-pass/2-fail lib suite; subsequent
checks were not reached. The second exited 0, with one passing test and 75
filtered tests per command. The third exited 101 after the 75-pass/1-fail lib
suite; subsequent checks were not reached. The fourth exited 4 at `cargo deny`;
all six doctests, rustdoc, and audit passed before it.

The coordinator then ran the following in `/home/jola/projects/diplodocus` at
`a39a7c2c7d729666c9a998f2f54ff939404a3ef9` under the same separately authorized
policy. It also exited 4:

```console
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-primary-deny.log 2>&1; cargo deny check'
```

The continuation independently compared the two historical logs. Both reject
exactly these ten package/version entries:

| Package | Version |
| --- | --- |
| `ar_archive_writer` | `0.5.3` |
| `jupyter-protocol` | `2.0.2` |
| `jupyter-zmq-client` | `1.0.1` |
| `option-ext` | `0.2.0` |
| `ring` | `0.17.14` |
| `sha1_smol` | `1.0.1` |
| `unicode_names2` | `1.3.0` |
| `untrusted` | `0.9.0` |
| `version-ranges` | `0.1.3` |
| `win_uds` | `0.2.2` |

Both end with `advisories ok, bans ok, licenses FAILED, sources ok`. Matching
the baseline does not fix or waive the failed license policy. The fresh
assignment still requires coordinator validation, independent review, a commit
in the fresh tree, clean-HEAD verification, integration, and accepted closure.

## Coordinator validation of the recovered implementation

The coordinator validated the frozen recovery files in the fresh assignment cwd
above, using non-login Bash and separately approved `require_escalated` access
to devenv. Messages `cm-01M36KJGXAZSYRWJKB2KAFNK8A` and
`cm-01M36KP982BHW6DQP6HSFJ9F27` record the invocation and result:

```console
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-04-recovery-validation.log 2>&1; cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps && cargo audit && cargo deny check'
```

| Check in the documented environment | Outcome |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --locked --all-targets` | Passed, 501 tests, 0 failed, 0 ignored, across 38 reports. Normal parallel execution; no exclusions or timeout changes. |
| `cargo test --locked --doc` | Passed, 15 doctests, 0 failed, 0 ignored, across 2 reports. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps` | Passed. |
| `cargo audit` | Passed; 310 locked dependencies scanned. |
| `cargo deny check` | Failed, exit 4; the same ten license-policy entries listed above. Advisories, bans, and sources passed. |

The worker independently counted the test results, compared all ten rejected
package/version entries with the historical baseline log, and verified that
the twelve frozen hashes were unchanged during the run. Both launch regressions,
including the marker-root `--config=` case and literal-environment control,
were present in the tested file. Its SHA-256 was
`b87dc3a901158e5146ae7e31580b7f0b2a747fd9b3aec888acae063cb1ebc27e`.

The coordinator then released only the ledger and focused test file for review
follow-up. This update records the completed run; it does not change production
or test bytes. The earlier intermittent startup failures remain historical
evidence and have a separate assigned investigation. The current passing run
does not claim a fix for them. The dependency license check remains failed, and
review, commit confirmation, integration, and accepted closure remain separate
steps.
