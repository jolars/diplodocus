# Execution deadline validation

This historical checkpoint covered the compound timeout and watched-site item
in `TODO.md`. The later [watched-site integration](execution-watched-validation.md)
provides its command and publication acceptance.
This change tightens the existing internal Jupyter lifecycle implementation.
The item remains open: the public production engine and real snapshot, site
publication, and watched command paths still need implementation and acceptance.
No library timeout test proves that a failed watched build preserves a site.

## Behavior

Startup, cell submission and channel waits, interruption, shutdown, termination,
and forced exit now share an absolute monotonic deadline check. Expiry takes
precedence over ready work on every poll, and a synchronous poll that overruns
the deadline cannot produce success. This is cooperative supervision, not
preemption of synchronous code.

Cell execution selects the earlier of the original cell deadline and the
terminal-synchronization deadline. It reports that phase even when both limits
have expired before the next poll. Equal deadlines report the cell phase.
Receiving more output does not restart either deadline. Existing cancellation,
process ownership, interruption, escalation, and reaping behavior remains in
the supervisor.

## Evidence

All commands ran from `/home/jola/projects/diplodocus` in a non-login shell.
`use_default` and `require_escalated` below record the selected execution policy.
The repository's documented entry point was `devenv shell --`.

| Command | Policy | Result and diagnostic |
| --- | --- | --- |
| `devenv shell -- cargo test --locked --lib execution::jupyter::tests::` | `use_default` | Baseline passed: 35 tests, including declared Python and R startup, sequential execution, and cleanup. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::deadline::tests` | `use_default` | Red regression: two failures and one pass with the original Tokio timeout behavior. Expired ready work was polled and returned success; a non-yielding overrun also returned success. |
| Same focused deadline command after correction | `use_default` | Four tests passed. The devenv rustfmt hook reformatted tracked edits and reported failure for that entry; this was not treated as a completed formatting gate. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::` | `use_default` | 82 tests passed, including five deadline tests and two new protocol tests. The entry hook again reformatted edits; final explicit checks below supersede that hook result. |
| Full check command below, redirected to `/tmp/diplodocus-deadlines-validation.log` | `use_default` | Blocked before validation: Nix could not connect to `/nix/var/nix/daemon-socket/socket` (`Operation not permitted`) while creating a GC root. No `.devenv` write denial or Git failure was observed. |
| Same full check command | `require_escalated`, approved | Formatting, all-target/all-feature Clippy with warnings denied, 540 all-target tests, and rustdoc with warnings denied passed. The log records the individual suite results. |
| Focused deadline command after making the pending-poll assertion tolerate expiry before its first poll | `use_default` | Five tests passed; both entry hooks passed. Production code was unchanged from the full check. |

The full check command was:

```sh
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The new protocol tests keep publishing output while withholding terminal
messages. They exercise a missing reply, missing idle, and missing both. They
verify the failure phase, one submitted cell, interruption before shutdown,
absence of cleanup errors, reaped kernel processes, and removed connection
directories. Another test gives the cell a shorter limit than terminal
synchronization and exercises both terminal arrival orders.

The next integration steps remain the production engine's supervised output
validation and rollback, real snapshot and site publication, and the watcher.
Acceptance must include an actual successful served site followed by a failing
authored execution, unchanged published files, continued serving, and successful
recovery on the next valid edit. Keep the original roadmap checkbox open until
that behavior is implemented and verified.

## Git access

Git access was checked independently of the development environment. From the
same working directory, `git diff --check` passed under `use_default`. The
following staging command failed under that policy because `.git/index.lock`
could not be created (`Read-only file system`). Its separately approved
`require_escalated` retry passed.

```sh
git add -- src/execution/jupyter.rs src/execution/jupyter/deadline.rs src/execution/jupyter/execution.rs src/execution/jupyter/process.rs src/execution/jupyter/session.rs src/execution/jupyter/tests/fixture.rs src/execution/jupyter/tests/pages.rs docs/spikes/authored-execution-contract.md docs/design/execution-deadlines-validation.md
```
