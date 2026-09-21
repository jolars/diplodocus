# M6-01 validation ledger

Date: 2026-09-21. Base:
`5c56119ecbf74180731a6d0a0e4c2aeb32e07ea7`.
Assignment: `ca-01M32654GX49T9ZGPN1WA1Y7TZ`.

All worker commands below use a non-login Bash shell in:

```text
/home/jola/.local/state/coterie/runs/cr-01M325FW0XP0N03DW97X5EDQ66/workspaces/cp-01M325FW0XHS5NG77N7528SCET/ca-01M32654GX49T9ZGPN1WA1Y7TZ
```

The selected worker policy is `workspace-write`, network `deny`, approvals
`never`. No command changes permissions, relocates a cache, or grants access to
common Git metadata. The lead confirmed coordinator ownership before editing.

## Worker environment and required checks

| Command | Result | Diagnostic |
| --- | --- | --- |
| `devenv shell -- true` | Blocked, exit 1 | Nix fetcher-cache lock write denied before shell entry. |
| `devenv shell -- cargo fmt --all -- --check` | Blocked, exit 1 | Same environment failure; rustfmt did not run. |
| `devenv shell -- cargo clippy --locked --all-targets --all-features -- -D warnings` | Blocked, exit 1 | Same environment failure; Clippy did not run. |
| `devenv shell -- cargo test --locked --all-targets` | Blocked, exit 1 | Same environment failure; tests and declared kernels did not run. |
| `devenv shell -- env RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` | Blocked, exit 1 | Same environment failure; rustdoc did not run. |
| `devenv shell -- cargo test --locked --test execution_contract` | Blocked, exit 1 | Test-first check after adding assertions, before implementation; same environment failure, not an observed failing test. |

The exact failing resource was:

```text
/home/jola/.cache/nix/fetcher-locks/98e6c8e5ca59f95bc9707696420bc5ca1a82a1e4323a1b3baac9f73943984784.lock
Read-only file system
```

This occurred while realizing the locked `devenv-nixpkgs` input. The worker
successfully created `.devenv/bootstrap`; no `.devenv` write denial was observed.
This worker failure was not a daemon denial or a Git permission test. The worker
reported it durably to the lead in message `cm-01M3267TYFNWK5Z0BESQ5CCRZD`.

## Supplemental worker checks

These commands used the same cwd and selected policy. They establish specific
local evidence, not a replacement for the documented environment checks.

| Command | Result | Diagnostic or scope |
| --- | --- | --- |
| `git status --short` and `git rev-parse HEAD` before edits | Passed | Clean assignment at the full base commit above. No Git writes attempted. |
| `cargo generate-lockfile --offline` | Passed, superseded | Resolved dependencies, but also upgraded unrelated packages. That generated file was discarded before validation. |
| `git show HEAD:Cargo.lock > Cargo.lock` | Passed | Restored only the worker's generated lockfile from the base; no index or Git metadata mutation. |
| `cargo update --offline -p diplodocus` | Passed | Added 37 packages; retained existing locked versions. |
| `cargo tree --locked --offline -e features -p image` | Passed | Resolved decoder dependency closure. |
| `cargo tree --locked --offline -e features -i image` | Passed | Only the `jpeg` and `png` image features are selected. |
| `cargo fmt --all` | Passed | Formatted the small shared Rust changes and tests. |
| `cargo fmt --all -- --check` | Passed | No formatting diff. |
| `cargo test --locked --offline --test execution_contract` | Passed | 16 tests, no failures or ignored cases; includes placeholder shape, diagnostic spellings, unchanged HTML deserialization trust, and staging separation. |
| `git diff --check` | Passed | No whitespace errors. |

No snapshots changed. New tests were authored before the accessor and diagnostic
implementation. The documented-environment test-first run was blocked before
compilation; do not report an observed red/green cycle.

## Coordinator baseline and worktree acceptance

The lead reported the primary checkout baseline as passed at the base commit in
durable message `cm-01M326A17ZMN0JNQDZEK6B9QSA`. The cwd was
`/home/jola/projects/diplodocus`. Its initial `use_default` command was blocked at
GC root creation by a Nix daemon socket denial (`Operation not permitted`), which
is distinct from this worker's fetcher-cache failure. The user approved the
coordinator's `require_escalated` retry of this exact command:

```console
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

That baseline command exited 0, including both declared Python/R production
lifecycle and state-retention tests. Coordinator evidence is in
`/tmp/diplodocus-m6-baseline.log` and `/tmp/diplodocus-m6-validation.md`.
This is reported baseline evidence, not validation of this assignment's edits.

The coordinator must run the required checks against the changed assigned
worktree, record its selected policy and outcome, review the dependency policy,
and commit the intended paths before assignment completion. Worker Git staging
and committing remain unsupported because Git metadata is outside the writable
tree. A validation or commit blocker keeps the assignment active. After the
handoff, the worker stops editing and verifies the returned full commit against
HEAD and worktree cleanliness before submitting its result.

### Coordinator validation of the intended changes

The lead reported the following completed validation in durable message
`cm-01M327A05M8ZB7BHM5GB49DJQM`. The cwd was the exact assigned worktree above,
the shell was non-login Bash, and the selected policy was separately
user-approved `require_escalated`. The worker policy remained unchanged.

```console
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps && cargo audit && cargo deny check' > /tmp/diplodocus-m6-01-validation.log 2>&1
```

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Passed | No formatting changes required. |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed | Warnings denied. |
| `cargo test --locked --all-targets` | Passed | Includes the declared real Python/R lifecycle and state-retention tests and all 16 execution-contract tests. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps` | Passed | Rustdoc warnings denied. |
| `cargo audit` | Passed | No reported vulnerabilities. |
| `cargo deny check` | Failed, exit 4 | License policy rejected the ten package versions below; advisories, bans, and sources passed. |

The coordinator also ran this baseline comparison in
`/home/jola/projects/diplodocus`, at unchanged HEAD
`5c56119ecbf74180731a6d0a0e4c2aeb32e07ea7`, using non-login Bash and separately
user-approved `require_escalated`:

```console
devenv shell -- cargo deny check > /tmp/diplodocus-m6-baseline-deny.log 2>&1
```

The baseline command also exited 4 and rejected exactly the same package/version
set. The changed worktree introduced no new license-policy failures:

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

The four checks required by M6-01 passed in the documented environment against
the intended code and dependencies. The additional dependency-policy check
remains a recorded baseline failure, not an overall pass. `deny.toml` is
unchanged. The coordinator released only this ledger for the evidence append
before final review and the separate Git commit handoff.
