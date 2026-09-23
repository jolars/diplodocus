# Kernel startup port validation

This record covers the bounded startup investigation on September 23, 2026,
based on `a39a7c2c7d729666c9a998f2f54ff939404a3ef9`. The change prevents
concurrent launches in one process from selecting ports that another launch
still owns during the handoff from reservation listeners to a child kernel.

## Diagnosis and scope

The original launcher reserved five loopback ports with listeners, wrote the
connection file, and dropped the listeners before spawning the child. Another
launch could select those newly available ports before the first child bound
them. Keeping a logical lease after releasing the listeners closes that
same-process reuse window. The allocator claims a numeric candidate before
binding it: binding first and then rejecting an owned port could itself block
the pending child.

Temporary fixture diagnostics recorded only fixed bind stages, I/O error kinds,
process IDs, panic source locations, and bounded socket ownership categories.
They did not print connection keys, endpoints, raw error messages, environment
contents, or kernel payloads. The diagnostics were removed before validation
of the final code.

Two IOPub binds failed with `AddrInUse` in diagnostic run 1. Run 5 captured a
stdin bind failure against a socket in the listening state that belonged to
neither the fixture nor its parent. Its exact owner remains unknown. The
sibling ownership instrumentation in run 6 captured no failure. Run 5 passed
its test assertions despite the captured bind failure, because tests that
expect failures can conceal an earlier startup failure.

These observations establish real bind conflicts. The deterministic regressions
separately establish the same-process reuse bug and verify its fix. They do not
prove that every historical startup exit had this cause. In particular, the
real Python/R page test exited during startup in run 3 without a fixture
diagnostic, so that exit's cause remains unknown.

The final allocator uses the existing randomness dependency to choose a starting
point in the unprivileged port range, examines at most 128 candidates, and keeps
five distinct successful claims. It skips claimed ports before any socket bind,
retries only an OS `AddrInUse` result with another candidate, and propagates
other errors. The mutex protects only integer ownership bookkeeping; no socket
operation or await occurs under it. Allocation errors and cancellation release
partial claims. Normal cleanup holds the lease until the process group exits.
No authored code is retried, and sessions remain parallel.

The registry is process-local. An unrelated process or an outbound connection
can still acquire a port after its listener is released. Exceptional
`KernelProcess` destruction sends the existing best-effort kill signal and
releases the lease, but `Drop` cannot await reaping. These limits are distinct
from the normal supervised cleanup path.

## Environment and authority

All commands below used this assigned working directory:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36JGZR6AZE6AY7DR21R0GJV
```

The worker used non-login Bash with the selected `workspace-write` filesystem,
`network-deny`, and `approvals-never` policy. Its first documented environment
entry succeeded and reproduced a startup failure on unchanged source:

```sh
devenv shell -- cargo test --locked --lib execution::jupyter::tests
```

This command exited 101: 34 passed, 1 failed, and 40 were filtered out. The
`wrong-major` case of
`startup_rejects_invalid_protocol_identity_and_authentication` reported that the
kernel exited during startup. Its output is retained in the worker transcript.

The next worker validation attempt was blocked before tests ran:

```sh
DIPLODOCUS_FIXTURE_DIAGNOSTICS=/tmp/diplodocus-startup-diagnostics.log devenv shell -- cargo test --locked --lib execution::jupyter::tests > /tmp/diplodocus-startup-diagnostic-run.log 2>&1
```

It exited 1 while creating a GC root: access to
`/nix/var/nix/daemon-socket/socket` was denied with `Operation not permitted`.
No `.devenv` write denial was observed. The worker reported this environment
blocker through durable message `cm-01M36JPTMN8D35K84YMFFBMKV9`; it did not widen
permissions or attempt Git metadata writes. The coordinator ran the remaining
tests in the same directory through separately authorized `require_escalated`
non-login Bash and the documented `devenv shell` entry point.

The worker also ran `cargo fmt --all` and `git diff --check`, both successfully,
under its selected policy. These checks did not replace the coordinator's
validation in devenv. Committing remained a separate coordinator handoff because
the worktree's Git metadata lies outside the worker's writable tree.

## Diagnostic runs

The coordinator used the following command for run 1:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-diagnostic-run.log 2>&1; DIPLODOCUS_FIXTURE_DIAGNOSTICS=/tmp/diplodocus-startup-diagnostics.log cargo test --locked --lib execution::jupyter::tests'
```

Runs 2 through 6 used the same command with the run number appended to both
log basenames, for example:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-diagnostic-run-2.log 2>&1; DIPLODOCUS_FIXTURE_DIAGNOSTICS=/tmp/diplodocus-startup-diagnostics-2.log cargo test --locked --lib execution::jupyter::tests'
```

| Run | Exit | Test result | Diagnostic evidence |
| --- | --- | --- | --- |
| 1 | 101 | 33 passed, 2 failed | Two IOPub `AddrInUse` errors; startup exit and submission wait timeout. |
| 2 | 0 | 35 passed | No bind failure captured. |
| 3 | 101 | 34 passed, 1 failed | Real Python/R page startup exit; no fixture diagnostic. |
| 4 | 0 | 35 passed | No bind failure captured. |
| 5 | 0 | 35 passed | Stdin `AddrInUse`; listening socket owned by neither self nor parent. |
| 6 | 0 | 35 passed | No failure captured by the sibling ownership extension. |

All runs used normal test parallelism. The coordinator confirmed these outcomes
in messages `cm-01M36JVCG3TFW7FG7D60TED9R4`,
`cm-01M36JZE39NYPHK608KTNZX5KC`, `cm-01M36K0D72N3ET1NKB8EAEKWM6`,
`cm-01M36K1SK2M6K6DDSXQP5D09XP`, `cm-01M36K2YR2ADQGT1HKR4VE1SC0`, and
`cm-01M36K92V9JNXR720E4GWSZ88H`.

## Deterministic regression and initial validation

The focused regression command was:

```sh
devenv shell -- cargo test --locked --lib execution::jupyter::process::ports::tests::pending_launch_ports_are_not_reallocated_after_listener_release -- --exact
```

First, a minimal extraction retaining the old allocation behavior handed the
same port to two pending launches. The test failed as expected: 0 passed,
1 failed, exit 101. The log is
`/tmp/diplodocus-startup-port-regression-red.log`; coordinator confirmation is
`cm-01M36KDYHNS7GVSWX96D2G28M4`.

The strengthened final regression additionally asserts that an owned candidate
never reaches the socket bind callback. With the ownership claim temporarily
disabled, the same focused command failed at that assertion: 0 passed, 1 failed,
exit 101. The log is `/tmp/diplodocus-startup-port-claim-red.log`; coordinator
confirmation is `cm-01M36KT74VB8E8PN2VQ958K65P`. No full suite ran with the claim
disabled. The claim was restored before final validation.

The coordinator then ran this exact command against the first frozen candidate:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-green-validation.log 2>&1; cargo test --locked --lib execution::jupyter::process::ports::tests && cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The command exited 0. Coordinator message `cm-01M36M4P69F1F1GWGBQB4FGGF0`
confirmed the counts and unchanged code hashes:

| Check | Outcome |
| --- | --- |
| Focused allocator tests | 6 passed, 0 failed. |
| Formatting | Passed. |
| Clippy, all targets and features, warnings denied | Passed. |
| All targets with normal test parallelism | 482 passed, 0 failed, 0 ignored across 37 reports. |
| Doctests | 13 passed, 0 failed, 0 ignored across 2 reports. |
| Rustdoc with warnings denied | Passed. |

The six focused tests cover forced reuse after listener release and reuse after
lease release, concurrent distinct port sets, OS port conflicts, immediate
non-conflict error propagation and partial cleanup, cancellation cleanup, and
bounded exhaustion without binding an owned port. The focused cases also appear
in the all-target count and should not be counted twice.

The production file SHA-256 values for that initial validation were:

| Path | SHA-256 |
| --- | --- |
| `src/execution/jupyter/process.rs` | `ded7630c7b5f22ee1712c7c6786b2c4ff0e8499c604b1d4944bd09da9d02d477` |
| `src/execution/jupyter/process/ports.rs` | `1cbbf90b7151632a77280545157e941854e58ee045bfd2dcd6326e3766eca4b3` |

## Review correction: isolate deterministic test ownership

Independent review found that several tests released global ownership and then
assumed that an exact port remained available for reacquisition. A legitimate
parallel reservation could claim it first. The cancellation test also dropped
a listener before the spawned reservation acquired its claim. These were test
reliability defects even though the first full run passed.

The following command reproduced the defect after a temporary test change
explicitly inserted a valid competing reservation before the original exact-port
reacquisition assertion:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-test-interleaving-red.log 2>&1; cargo test --locked --lib execution::jupyter::process::ports::tests::pending_launch_ports_are_not_reallocated_after_listener_release -- --exact'
```

Under the same coordinator policy and working directory, it exited 101 with
0 passed, 1 failed, and 80 filtered. The original assertion received `AddrInUse`
after the competing reservation correctly claimed the released port. Coordinator
message `cm-01M36MT0HRGGPTPHD7X03JAKJ7` confirms this result. This red result
reproduces the test's assumption defect, not a new production allocator failure.

The corrected deterministic tests use a scoped ownership registry and a
controllable binder. They verify that a competing reservation in the same
registry prevents reacquisition until release, while another test's independent
registry does not interfere. Cancellation directly drops a polled, pending
reservation and checks that both partial claims become available. The production
entry point still uses the static process-wide registry and real TCP listeners.
Real concurrent allocation and OS-conflict coverage remain parallel. The OS
conflict test keeps global ownership throughout its deliberate listener handoff,
and checks logical rollback with a token binder rather than assuming that a
released global port remains free. No test serialization was introduced.

## Final validation after review correction

The coordinator ran this exact command against the revised frozen source:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-reviewed-validation.log 2>&1; cargo test --locked --lib execution::jupyter::process::ports::tests && cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The working directory and separately authorized coordinator policy were unchanged.
The command exited 0. Message `cm-01M36N42R9KY0P8V09C4D7MPCF` confirms:

| Check | Outcome |
| --- | --- |
| Revised focused allocator tests | 6 passed, 0 failed. |
| Formatting | Passed. |
| Clippy, all targets and features, warnings denied | Passed. |
| All targets with normal test parallelism | 482 passed, 0 failed, 0 ignored across 37 reports. |
| Doctests | 13 passed, 0 failed, 0 ignored across 2 reports. |
| Rustdoc with warnings denied | Passed. |

The final validated code hashes are:

| Path | SHA-256 |
| --- | --- |
| `src/execution/jupyter/process.rs` | `f7fd4ae827536bd95a947ef1f0992e40a08bc2b9ee9e8e88b51f92d684c2bc6d` |
| `src/execution/jupyter/process/ports.rs` | `3a19e3bf77843527d635710c031bfd38ecd5e505566f9ba101cf8fe938f8e772` |

The coordinator verified unchanged frozen hashes after validation. Only this
ledger was updated afterward to record the confirmed results. These results
supersede the earlier green evidence for the revised source; the earlier checks
and hashes above remain historical evidence.

The temporary logs are local evidence artifacts, not repository fixtures.
Independent review, the coordinator commit, and accepted task closure remain
separate steps from this validation record.

## Recovery validation on the combined identity base

The original worker exited after commit verification but before submitting its
assignment. The source assignment has no final report. Its recorded recovery
handoff and independent review instead identify the approved source commit as
`b2be3b29ceb122ce01594128891f4b74ff5c562f`, with parent
`a39a7c2c7d729666c9a998f2f54ff939404a3ef9`. The continuation copied only that
commit's three changed files into a fresh worktree based on the integrated
identity commit, `15d9c770737066a13294b3b831d5012a277b0a75`.

The continuation verified the source commit, clean source worktree, and all
three committed file hashes against `/tmp/diplodocus-startup-freeze.json`.
It preserved the source files and index. The source index SHA-256 remained
`a0a74863221ab2a9dd3bddc102689c2f028f5dbf5cc64d5bb0c818c238401b18`.
The two production file hashes remain those in the final validation table
above. Only this ledger received a recovery appendix.

The recovery validation commands used this fresh assigned working directory:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36P2DRC9C4ZEHQX1HGKC73F
```

Under the selected `workspace-write`, `network-deny`, and `approvals-never`
policy, non-login Bash ran:

```sh
devenv shell -- cargo test --locked --lib execution::jupyter::process::ports::tests > /tmp/diplodocus-startup-recovery-worker-validation.log 2>&1
```

This command was blocked before tests ran, exiting 1 because a Nix fetcher
lock in `/home/jola/.cache/nix/fetcher-locks` was on a read-only filesystem.
This is distinct from the original worker's Nix daemon denial. No `.devenv`
write denial was observed. Git metadata remained outside the worker's write
authority, and the worker attempted no Git writes. Durable message
`cm-01M36P6YHNSSY60BYJDF0J5KNA` reported the blocker to the coordinator.

Read-only Git checks and Python SHA-256 comparisons passed under the same
worker policy. They confirmed the fresh base, exactly three intended paths,
no staged changes, no whitespace errors, and preserved source bytes and
index. Their command and outcome ledger is
`/tmp/diplodocus-startup-recovery-worker-ledger.json`.

The coordinator then ran the following command in the fresh worktree through
separately authorized `require_escalated` non-login Bash and the documented
devenv entry point:

```sh
devenv shell -- bash -c 'exec > /tmp/diplodocus-startup-recovery-validation.log 2>&1; cargo test --locked --lib execution::jupyter::process::ports::tests && cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The command exited 0. Coordinator message
`cm-01M36PF25T4M15KXRTTCMBKVSQ` confirms:

| Check | Outcome |
| --- | --- |
| Focused allocator tests | 6 passed, 0 failed. |
| Formatting | Passed. |
| Clippy, all targets and features, warnings denied | Passed. |
| All targets with normal test parallelism | 507 passed, 0 failed, 0 ignored across 38 reports. |
| Doctests | 15 passed, 0 failed, 0 ignored across 2 reports. |
| Rustdoc with warnings denied | Passed. |

The coordinator confirmed unchanged hashes for all three recovered files and
the original source index after validation. The continuation also checked the
test counts in the log. This appendix records validation of the current
combined tree; the 482-test and 13-doctest results above describe the original
source base. Commit handoff, integration, and accepted closure remain separate
coordinator actions.
