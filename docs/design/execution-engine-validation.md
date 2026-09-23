# Public execution engine validation

This checkpoint continues the timeout and watched-site request in `TODO.md`.
The Linux `JupyterEngine` now implements the public `ExecutionEngine` trait
without cache storage. The watched-site checkbox remains open because real
workspace assembly, site publication, and watching are still missing.

## Behavior

Construction receives canonical repository roots and independently configured
environment-file declarations and performs no I/O. Execution rejects disabled,
vetoed, non-QMD, and candidate-free requests before reading source or discovering
a kernel. It reparses the current source and checks the complete prepared
request and authored anchors. A discovered language mismatch returns checked
skipped cells without resolving an executable or adding execution provenance.

For an executable page, the engine captures the running build, declared input
bytes, and one immutable launch observation. The supervisor uses that same
observation, including its pre-spawn identity checks. Startup's monotonic budget
covers pre-spawn revalidation through readiness. The preceding source,
discovery, launch-resolution, and input-snapshot work is cancelable preflight.
Cell and terminal deadlines, interruption, shutdown escalation, process-group
reaping, and connection-file ownership reuse the existing supervisor.

Each completed cell goes through live output validation before the next cell
can be submitted. The engine owns the reducer and asset store while the
supervisor owns the process. Fatal validation prevents further submission and
rolls back staging. Dropping the engine future drops staging ownership and wakes
supervised kernel cleanup. Successful return waits for cleanup, checks final
figure options before applying visibility, rereads source and declared inputs,
revalidates launch identity, constructs checked output records, and retains only
the complete final referenced asset set. Failures never return a page result.

Provenance uses observed build/component/platform, launch/spec, runtime,
environment-input, and deadline values. It contains no connection data or local
checkout paths. These checks detect ordinary file changes; they do not provide
an operating-system snapshot of interpreter dependencies or undeclared inputs.

## Warning ownership

Ignored kernel messages enter the private event stream as typed diagnostics.
The reducer emits validator warnings at the producing event, so its append-only
ledger preserves receipt order without parsing or sorting displayed messages.
Ignored messages neither allocate output slots nor trigger deferred clearing.
They also do not split adjacent as-is stdout chunks into different Markdown
fragments. Fragment warnings occupy the first chunk's event position.

Discovery warnings precede this ledger. Finalization offsets every output
diagnostic index exactly once. Structured session failures distinguish warnings
already consumed by the reducer from a pending transport tail. A fatal validator
keeps later protocol warnings from the already-received cell, and transport,
cancellation, cleanup, input-change, and retention failures preserve accumulated
warnings once. Cleanup and rollback errors remain separate from the primary
failure.

## Validation

Every validation command below ran from `/home/jola/projects/diplodocus` in a
non-login shell with selected policy `use_default`, through the documented
`devenv shell --` entry point. No Nix daemon or `.devenv` write blocker occurred.
Git metadata writes require separate authorization and do not establish or
change development-environment access.

The focused engine command is:

```sh
devenv shell -- cargo test --locked --lib execution::jupyter::tests::engine
```

The Jupyter suite command is:

```sh
devenv shell -- cargo test --locked --lib execution::jupyter::
```

| Command/run | Result and diagnostic |
| --- | --- |
| Focused engine command before implementation | Expected failure: the public `JupyterEngine` export did not exist. |
| Focused engine command after the first implementation | Two tests passed. The entry Clippy hook rejected the large structured error variant; the report was boxed. |
| Jupyter suite after error boxing | 96 tests passed. The entry Clippy hook rejected unnecessarily boxed method receivers; those methods now consume the report value. |
| Focused engine command with warning/input/cancellation cases | Eight tests passed. The entry Clippy hook passed; rustfmt corrected formatting. |
| Focused engine command while extending real-kernel tests | Failed to compile because a provenance assertion compared a component record to a version string. It now checks the record's `version`. |
| Jupyter suite after removing the module-wide dead-code allowance | 104 tests passed; one new terminal-timeout test exhausted its unnecessarily shortened startup limit under parallel load. Only the startup-specific case now shortens startup. Clippy identified test-only helpers and an intentionally unused metadata field; those are scoped explicitly. |
| Focused engine command during helper scoping | Failed to compile because a reducer test depended on a removed parent import. The test now imports `AssetReference` directly. |
| Jupyter suite with public shutdown/authority cases | 106 tests passed; one new assertion incorrectly required a wire interrupt before startup had established channels. The startup case checks cleanup/reaping; ready-session terminal failures check interrupt and shutdown requests. Entry hooks passed. |
| Full command below, before the final figure case | Passed formatting, all-target/all-feature Clippy, 566 all-target tests, 27 documentation tests, and rustdoc. |
| `devenv shell -- cargo test --locked --lib execution::jupyter::tests::engine::public_engine_validates_final_updated_figures_before_hiding_output` | Passed the real-Python cross-cell update/clear and hidden-figure cases. Entry hooks passed. |
| Full command below, final source | Passed formatting, all-target/all-feature Clippy with warnings denied, 567 all-target tests, 27 documentation tests, and rustdoc with warnings denied. |
| `git diff --check` | Passed. |

```sh
devenv shell -- bash -c 'cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-targets && cargo test --locked --doc && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps'
```

Public-engine tests cover validated SVG retention only after reaping, fatal
nested-image failure before the next cell, exact warning order and diagnostic
offsets, warning retention on output failure and cell timeout, forged preparation
and undeclared digest claims, source/environment changes during execution,
language mismatch, explicit cancellation and dropped futures, startup and both
terminal-order timeouts, and shutdown escalation. Ineligible requests fail
before source reading or discovery. Real `python3` and `ir` tests run
unconditionally and prove state retention plus safe HTML and Markdown wrappers.
The real Python update test validates final figures even when their owner is
hidden and verifies that cleared and superseded output does not retain assets.

Existing lifecycle, process-group, asset, reducer, identity, and checked-record
tests remain part of the full run. Neither those tests nor this checkpoint prove
cache restoration, command-level no-execution guarantees, or a watched site's
survival and recovery after an execution failure.
