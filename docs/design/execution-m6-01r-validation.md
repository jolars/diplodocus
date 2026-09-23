# M6-01R validation ledger

Date: September 23, 2026. Assignment: `ca-01M36DMMPW55FG24VFJZSXHX35`.
Base: `f4822d03cfcbbff4be6fce0a5ab87527f57e16fc`.

All worker commands used Bash and the assigned worktree:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36DMMPW55FG24VFJZSXHX35
```

The selected policy was `workspace-write`, network `deny`, approvals `never`.
No permissions, cache locations, or common Git metadata access were changed.
The lead confirmed coordinator commit ownership before edits in durable message
`cm-01M36DN59FJ68ZNKBS5ADXCV6K`. Only documentation, reference data, and a new
integration test changed; production Rust and dependency files did not change.

## Required environment checks

| Command | Outcome | Diagnostic |
| --- | --- | --- |
| `devenv shell -- true` | Blocked, exit 1 | Nix fetcher lock could not be opened; shell did not start. |
| `devenv shell -- cargo fmt --all -- --check` | Blocked, exit 1 | Same environment failure; rustfmt did not run. |
| `devenv shell -- cargo clippy --locked --all-targets --all-features -- -D warnings` | Blocked, exit 1 | Same environment failure; Clippy did not run. |
| `devenv shell -- cargo test --locked --all-targets` | Blocked, exit 1 | Same environment failure; tests did not run. |
| `devenv shell -- env RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` | Blocked, exit 1 | Same environment failure; rustdoc did not run. |
| `devenv shell -- cargo test --locked --test execution_artifact_contract` | Blocked, exit 1 | Same environment failure; focused tests did not run. |

Each failure reported this resource while realizing `devenv-nixpkgs`:

```text
/home/jola/.cache/nix/fetcher-locks/98e6c8e5ca59f95bc9707696420bc5ca1a82a1e4323a1b3baac9f73943984784.lock
Read-only file system
```

`.devenv/bootstrap` was created successfully. No `.devenv` write denial or Nix
daemon denial was observed. These are environment failures, not Git permission
tests. The worker reported the initial blocker durably in
`cm-01M36DQZ15823Q7EFQJE6AQ9JC`. Individual command logs are
`/tmp/m6-01r-{fmt,clippy,tests,rustdoc,contract}.log`.

## Supplemental checks and reference construction

These checks used the same cwd and selected policy. They do not replace the
required checks in the documented environment.

| Command or check | Outcome | Evidence |
| --- | --- | --- |
| `git status --porcelain=v1`; `git rev-parse HEAD` before edits | Passed | Clean assignment at the base above. No Git mutations attempted. |
| `cargo test --locked --offline --test execution_artifact_contract` before reference data | Failed as expected | Five tests compiled and failed because the fixture files did not exist. |
| Same focused command after initial fixture | Passed | Five tests. |
| Same focused command with reviewer-requested cleared-fragment checks before fixture correction | Failed as expected | Two failures: missing third warning and missing null fragment source. |
| Same focused command after correction | Passed | Six tests, none ignored. |
| `cargo fmt --all`; `cargo fmt --all -- --check` | Passed | Formatted the new Rust test; existing production files unchanged. |
| `cargo clippy --locked --offline --test execution_artifact_contract -- -D warnings` | Passed | New test and its compiled dependencies checked with warnings denied. |
| Python read-only local Markdown link and trailing-whitespace checks | Passed | All changed Markdown link targets exist. |
| `git diff --check` | Passed | No whitespace errors in tracked changes; the separate scan covered new Markdown files. |

A temporary Rust helper in `/tmp/execution-contract-inputs.rs`, compiled with
`rustc --edition=2024 -L dependency=target/debug/deps` and the existing compiled
`diplodocus`, `serde_json`, and `toml` libraries, extracted current preparation
and fragment-parser output. An initial helper setup stopped on two available
`serde_json` feature builds; selecting a compatible existing build succeeded.
The helper did not run a kernel. Python then authored the canonical manifest,
explicit DTO projections, and digest-named SVG files. The checked-in Rust tests
independently verify the canonical encoder against the preexisting key/encoding
oracle, all artifact/content hashes, actual prepared source/options/declarations,
actual fragment trees and warning ranges, asset format/closure, slot identities,
and diagnostic-index-independent unsupported digests. Neither helper remains in
the repository, and no production cache or safety implementation is claimed.

## Review and coordinator checks

The lead and independent reviewer reviewed the draft and complete fixture.
Corrections preserve existing clear semantics, place unsupported warning indices
outside content hashes, retain cleared-fragment provenance, remove a speculative
parser-warning code, make cancellation/staging ownership explicit, and establish
a coordinator checkpoint for shared records before parallel engine/cache work.
The reviewer approved the final content, as reported by the lead in
`cm-01M36F0DAVBZRFM4XH75A47AZD`.

The coordinator reported validation of the frozen assignment in durable message
`cm-01M36F3ZQ6C5RFNF0PXS37Z01Q`. The cwd was the exact assigned worktree above.
It used non-login Bash and separately approved `require_escalated` access for
the documented environment command; worker permissions remained unchanged.

```console
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

| Check | Outcome | Evidence |
| --- | --- | --- |
| Rustfmt | Passed | No formatting changes required. |
| All-target, all-feature Clippy | Passed | Warnings denied. |
| All-target tests | Passed | 459 tests, zero failed or ignored, across 36 test reports; includes six new artifact tests and the declared Python/R tests. |
| Rustdoc | Passed | Warnings denied. |

The command exited 0. Captured output is in
`/tmp/diplodocus-m6-01r-validation.log`. No Nix daemon, fetcher-lock, `.devenv`,
or Git write denial occurred in that coordinator validation run. This validates
the changed worktree, independently of the historical baseline and the worker's
blocked environment entry. After these checks, only this evidence append and
the authorized plan-introduction status correction changed. The coordinator
handles the Git commit separately; the worker verifies its full ID and worktree
cleanliness before submission.
