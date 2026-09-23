# Execution output integration validation

This checkpoint advances the compound timeout and watched-site request in
`TODO.md`. It joins the existing live output validators to incremental Jupyter
reduction. The public production engine and real snapshot, site publication,
and watched command paths remain unfinished, so the roadmap item stays open.

## Behavior

The reducer validates every supported Markdown and HTML alternative through the
live safety boundary while the session still owns the kernel. All alternatives
share the page's asset store. A missing nested image is fatal even when another
representation is preferred or the cell is hidden. Validation finishes before
the next cell can overwrite or remove a local image.

Each surviving output slot retains immutable validated representations together
with its portable projection. Updates replace every registered owning slot;
clears remove the corresponding evidence. Fragment ordinals distinguish repeated
updates that reuse one producer slot. Final asset retention includes nested
images in every surviving alternative and excludes cleared or replaced images.

The adapter uses the shared canonical representation hashes and retains typed
diagnostics through the validated-page checkpoint. Nested image rejections keep
their Markdown or HTML attribution when a safe fallback survives. Preparation
captures authored identifiers and cell labels. `PreparedExecution::checked`
reconstructs that complete set from the original source, rejecting both injected
targets and omitted targets.

## Validation

All commands ran from `/home/jola/projects/diplodocus` in a non-login shell with
the selected `use_default` policy. The repository's documented environment entry
was `devenv shell --`. No Nix daemon or `.devenv` write blocker occurred during
these checks. Formatting hooks changed intermediate edits on some entries;
only the final explicit formatting command below establishes the formatting
gate.

| Command | Result and diagnostic |
| --- | --- |
| `devenv shell -- cargo test --locked --lib execution::jupyter::output::tests::images` before implementation | Expected red regression: four passed, two failed. Safe HTML fell back to plain text, and nested Markdown assets were absent from retention. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::output::tests::validated` during integration | The nested-image warning test initially failed with an association error. After the diagnostic binding correction, all three tests passed through `ValidatedPage::checked`. |
| `devenv shell -- cargo test --locked --lib execution::validated::tests` | The new anchor test initially failed because an injected target was accepted. After source-derived anchor comparison, all 27 tests passed. |
| `devenv shell -- cargo test --locked --lib nested_asset_failure_in_an_unselected_alternative_stops_execution_and_rolls_back` | Passed for both Markdown and HTML protocol modes. Exactly one cell was submitted; the failure was `AssetMissing`, the reducer could not finish successfully, cleanup reported no errors, the process and connection directory were removed, and staging was empty. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::` | All 88 focused tests passed. |
| Full check command below | Passed: formatting, all-target/all-feature Clippy with warnings denied, 547 all-target tests, 27 documentation tests, and rustdoc with warnings denied. |
| `git diff --check` | Passed. Git metadata access is separate from development-environment access. |

```sh
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The seven new tests cover safe HTML selection, early nested-image staging,
repeated display updates, typed nested-image warnings, final asset retention,
fatal-output cleanup, and authored anchor integrity. Existing fragment tests now
check typed diagnostic attribution and a fixed canonical Markdown digest. That
digest was independently checked with Python SHA-256 over the domain-separated,
sorted, compact UTF-8 JSON representation.

These checks establish the internal adapter and supervised callback behavior.
The production engine must still bind the resolved launch plan to identity and
spawn, construct portable provenance, and revalidate inputs after cleanup.
Watched-site acceptance requires a real successful publication followed by
failed execution, unchanged published files, continued serving, and recovery on
the next valid edit.
