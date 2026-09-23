# M6-05 validation ledger

Date: September 23, 2026. Assignment: `ca-01M36FNYHKQ11GKAX1G530A7F6`.
Task: `ct-01M36D4QHEGA3782C753HXWZRZ`.
Base: `f14c60d692d6a1da31c3d1b10126e49384f59778`.

All worker commands used non-login Bash in this assigned worktree:

```text
/home/jola/.local/state/coterie/runs/cr-01M36CJ6S3PH09F9VXEKTD8JFG/workspaces/cp-01M36CJ6S324YQY6J3MVRBXEDE/ca-01M36FNYHKQ11GKAX1G530A7F6
```

The selected policy was `workspace-write`, network `deny`, approvals `never`.
No permission, cache-location, or common Git metadata changes were attempted.
The coordinator established authorized commit ownership before edits in message
`cm-01M36FPKDDM8EMS816MEE2FW61`. That message also authorized the sole shared
production edit, `pub mod output_safety;` in `src/execution.rs`. Other changes
are the owned safety module and children, its integration test, and this ledger.

## Environment and Git access

| Command | Outcome | Diagnostic |
| --- | --- | --- |
| `git status --porcelain=v1` before edits | Passed | Clean assignment. |
| `git rev-parse HEAD` before edits and at review freeze | Passed | Full base ID above. |
| `devenv shell -- true` | Blocked, exit 1 | Nix fetcher lock was read-only before shell entry. |
| `git diff --check` | Passed | No whitespace errors in tracked changes. |
| `git status --porcelain=v1` at review freeze | Passed | Only intended owned paths and the authorized module registration changed. |

The environment entry failure named:

```text
/home/jola/.cache/nix/fetcher-locks/98e6c8e5ca59f95bc9707696420bc5ca1a82a1e4323a1b3baac9f73943984784.lock
Read-only file system
```

This occurred while realizing the locked `devenv-nixpkgs` input.
`.devenv/bootstrap` was created successfully. No Nix daemon denial or `.devenv`
write denial was observed. Git read commands succeeded; direct worker staging
and committing were not attempted because the authorized coordinator owns that
handoff. These are separate environment and Git boundaries.

The worker reported the environment blocker in `cm-01M36FRM7MGRM1BK3TKGNM5DQ0`.
The coordinator acknowledged it in `cm-01M36FSRZMJ9ZQ5479BXN66S7D`, instructed
the worker not to repeat entry attempts without an external change, and agreed
to run the required checks in this exact frozen worktree using separately
authorized access. The checks below are supplemental, not a replacement for
that documented environment.

## Test-first implementation and supplemental checks

All rows use the cwd and selected policy above. Cargo used the inherited
pinned toolchain and its locally available dependencies, without network access.

| Command or check | Outcome | Evidence |
| --- | --- | --- |
| `cargo test --locked --offline --test execution_output_safety` before implementation | Failed as expected | The safety module import was unresolved; `/tmp/m6-05-red.log`. |
| Same command during initial implementation | Failed, then passed | Four shadowed-helper compile errors were corrected; five initial cases passed in `/tmp/m6-05-green.log`. |
| Same command after expanding adversarial tests | Failed as expected | Cross-page asset use was accepted; 11 of 12 cases passed in `/tmp/m6-05-expanded-red.log`. |
| Same command after namespace and HTML-origin checks | Passed | All 12 cases passed; `/tmp/m6-05-expanded-green.log`. |
| `cargo test --locked --offline --test execution_output_safety fragment_warnings_survive` before warning correction | Failed as expected | Prior unsupported-fragment warning was dropped; one warning instead of two; `/tmp/m6-05-warning-red.log`. |
| Full safety test after warning correction | Passed | All 13 cases, including warning retention before unsafe links and fatal missing/escaping images; `/tmp/m6-05-warning-green.log`. |
| Full safety test after structural traversal cases | Failed as expected | Caption bindings followed rows, and a valid ragged GFM body row was rejected; `/tmp/m6-05-bindings-red.log`. |
| Full safety test after traversal and ragged-row corrections | Passed | All 16 cases; `/tmp/m6-05-bindings-green.log`. |
| Final `cargo test --locked --offline --test execution_output_safety` | Passed | All 17 cases, none ignored; `/tmp/m6-05-fixture.log`. |
| `cargo test --locked --offline --test execution_artifact_contract --test markdown_fragments --test execution_assets` | Passed | 6 artifact, 10 fragment, and 11 asset tests; `/tmp/m6-05-adjacent-tests.log`. |
| `cargo fmt --all` and `cargo fmt --all -- --check` | Passed | Final Rust formatting is clean. |
| `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` during implementation | Failed, then passed | Redundant generated projection field names and one nested test conditional were corrected. Final all-target/all-feature run passed; `/tmp/m6-05-clippy.log`. |
| `cargo test --locked --offline --doc` | Passed | 13 examples: one ordinary doctest and 12 compile-fail tests, including nine new safety trust-boundary examples; `/tmp/m6-05-doc-tests.log`. |
| Same doctest command after the reviewer corrected the Markdown mutation example | Passed | The example now mutates the canonical `Vec` through an immutable borrow, avoiding a nonexistent slice method. |
| `rustc --crate-type=lib --edition=2024 --extern diplodocus=<current rlib> -L dependency=target/debug/deps /tmp/m6_05_mutation_{positive,negative}.rs -o /tmp/m6_05_mutation_{positive,negative}.rlib` | Passed | Positive `&mut DecodedMarkdown` control compiled; the identical `Vec::clear` operation through `ValidatedMarkdown::canonical_content()` failed with E0596, as required. |
| `env RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` | Passed | Rustdoc warnings denied; `/tmp/m6-05-rustdoc.log`. |

The final safety tests cover token-level rejection before HTML parsing can erase
unsupported tags, decoded and repeatedly encoded URLs, authored-anchor checks,
canonical markup, active nested SVG rejection, and fatal missing, escaping, or
symlinked image sources. Restore tests use only actively validated staged bytes,
reject altered metadata and foreign page namespaces, and succeed after the
original generated image files are deleted.

Markdown coverage includes parser provenance and span checks, inert fences and
raw HTML, typed warnings retained on later rejection and failure, complete image
addresses through lists, emphasis, captions, table cells, and nested image alt
content, and repeated equal spans and digests. Ragged body rows with missing or
surplus cells preserve their complete parser tree and images on live validation
and restore. An async test holds both wrappers across an await in a `Send`
future. Compile-fail tests check private construction, mutation, serialization,
and deserialization. The private-construction examples use correctly typed
fields so the privacy boundary is the reason compilation fails.

The unchanged complete artifact fixture is an independent oracle. The new test
checks the accepted canonical HTML bytes and digest, projects actual validated
Markdown bindings into the exact frozen fragment content, roundtrips restoration,
and retains both the repeated visible image and the distinct hidden image. No
identity, cache-storage, reducer, or session implementation enters this module.

## Review and required coordinator validation

The coordinator identified the dropped-warning path in
`cm-01M36GCM1WVHZ2XDYBDK7785CD`; the worker confirmed its failing regression before
fixing it. The independent reviewer identified the ragged-table restriction,
relayed in `cm-01M36GR04J5Q93FFX8H2JAHNVF`; the worker also observed that failure
while adding traversal tests. Both corrections are covered above.
The reviewer also corrected the Markdown mutation example in
`cm-01M36GYKAM592JW30W4HW6WECH`; positive and negative compiler controls now
confirm that the failure demonstrates the immutable borrow, not a missing method.

The coordinator reported final independent approval and passing required checks
in `cm-01M36HBEY1N6RP7K2MVHQR44EB`. The command ran in the exact assigned
worktree above, under non-login Bash with separately approved
`require_escalated` access for the documented devenv. Worker permissions stayed
unchanged.

```console
devenv shell -- bash -c 'exec > /tmp/diplodocus-m6-05-validation.log 2>&1; cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

| Required check | Outcome | Evidence |
| --- | --- | --- |
| Formatting | Passed | No Rust formatting changes. |
| All-target/all-feature Clippy | Passed | Warnings denied. |
| All-target tests | Passed | 476 tests across 37 result reports, none failed or ignored. Includes the declared real kernels and the existing Jupyter startup identity/authentication test. |
| Explicit doctests | Passed | 13 tests across two result reports, none failed or ignored. |
| Rustdoc | Passed | Warnings denied. |

The complete command exited 0. Temporary Cargo package-cache lock waits resolved
normally; the coordinator run had no Nix daemon, fetcher-lock, or `.devenv`
failure. The coordinator checked all ten frozen path hashes before and after,
confirmed the exact intended path set and empty index, and reported independent
review approval with no material findings. The worker read the final result
reports in `/tmp/diplodocus-m6-05-validation.log` to corroborate the validation
message. Only this ledger changed after that validation.

The coordinator owns the final Git commit handoff. This assignment does not
claim accepted M6-05 closure: the coordinator also owns the shared-record
checkpoint using the accepted safety wrappers and identity types before
releasing engine/cache work.
