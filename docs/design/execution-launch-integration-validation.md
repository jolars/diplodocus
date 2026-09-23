# Execution launch integration validation

This checkpoint continues the timeout and watched-site request in `TODO.md`.
It binds the internal Jupyter supervisor to the existing immutable identity
observation. The public production engine and watched publication path remain
unfinished, so the requested roadmap item stays open.

## Behavior

Discovery fingerprints the selected kernelspec bytes. Resolution checks that
observation before and after constructing the launch plan, checks the authored
page's repository and working directory, and preserves discovery warnings on
failure. A changed observation produces `execution-input-changed` before launch.

The supervisor uses `LaunchIdentityInput` for the executable, argument
substitution, explicit environment, working directory, language, and interrupt
mode. It revalidates the spec, executable bytes, captured PATH resolution, and
working directory immediately before spawning. Revalidation may reject changed
facts; it cannot replace the captured executable with a newly selected program.
The process adapter's separate executable resolver has been removed.

Internal startup uses one absolute monotonic deadline through launch resolution,
pre-spawn revalidation, and readiness. Resolution cannot restart the budget.
Cancellation remains supervised, private connection resources remain owned
through cleanup, and discovery warnings survive startup and cleanup failures
without duplication. A caller with all configured repository roots can resolve
once, snapshot inputs using the plan's identity view, and pass that same plan
to the supervisor.

These checks detect ordinary input changes. They do not provide an operating
system snapshot of interpreter dependencies or undeclared files.

## Validation

All commands below ran from `/home/jola/projects/diplodocus`, in a non-login
shell, with selected policy `use_default`. The documented environment entry was
`devenv shell --`. No Nix daemon or `.devenv` write blocker occurred. Git metadata
access remains a separate permission boundary.

| Command | Result and diagnostic |
| --- | --- |
| `devenv shell -- cargo test --locked --lib execution::jupyter::tests::launch` before implementation | Expected red regression: two existing tests passed, while the new changed-spec test failed because the original runner still launched the captured kernel. The test awaited shutdown before reporting failure. |
| Same focused command during implementation | The entry Clippy hook caught a removed resolver's unused import; that import was removed. A later run exposed that hashing the 220 MB debug test executable could exhaust the protocol fixture's startup limit. The fixture now uses the system shell as a small launcher with fixed `exec "$@"` and positional arguments, preserving the protocol process and exact argv. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::` during integration | 91 passed and one failed in test setup: copying a Nix-store executable preserved its read-only mode. Only the temporary mutation fixture is made owner-writable before changing its bytes. |
| Focused launch command after corrections and warning coverage | Eight tests passed, including six new launch tests and two existing tests matched by the filter. |
| Full check command below | Passed: formatting, all-target/all-feature Clippy with warnings denied, 553 all-target tests, 27 documentation tests, and rustdoc with warnings denied. |
| `git diff --check` | Passed. |

```sh
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

The new tests reject spec changes after discovery, spec and executable changes
after resolution, and a PATH symlink retargeted to a byte-identical executable.
They also check cancellation before spawn, warning retention on preflight
failure, reuse of an already-expired startup deadline, and successful startup,
cleanup, and subsequent revalidation of the same identity. Existing protocol,
process-group, output-failure, drop, and declared Python/R tests exercise this
same launch path.

The next engine integration must combine protocol and reducer warnings in
receipt order, build portable provenance from actual observations, and revalidate
source and declared inputs after cleanup before retaining output assets. Real
watched-site acceptance still requires publication, continued serving after an
execution failure, unchanged published files, and recovery on the next valid
edit.
