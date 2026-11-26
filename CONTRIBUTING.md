# Contributing

Diplodocus is a Rust CLI that builds documentation sites for Python, R, Julia,
Rust, and C++ packages. The [configuration guide](docs/guide/configuration.md)
and [authoring guide](docs/guide/authoring.md) describe its current behavior.
See the [changelog](CHANGELOG.md) for released changes and the
[design](docs/development/design.md) for an overview of the architecture.

## Development environment

Run commands from the repository root. Enter `devenv shell` for the pinned Rust
toolchain, language runtimes, Jupyter kernels, Node, and Chromium.

```sh
cargo build --locked
cargo run --locked -- check
cargo run --locked -- build --output site/docs
```

Use `task preview` to watch generator changes and serve the four-language
monorepo example with live reload.

Julia API discovery requires Julia 1.10 through 1.13 and a prepared package
project. The development shell provides Julia. The compatibility job runs the
standard-library exporter test and a CLI acceptance case on each supported
minor:

```sh
julia --startup-file=no --history-file=no tests/julia_runtime.jl
cargo test --locked --test native_adapter_contract \
  generated_julia_methods_are_discovered_and_doctests_run_only_during_extraction
```

## Checks

Add focused regression tests for behavior changes, and review any changes to
golden files in `tests/snapshots/`. Run the relevant integration suite while
developing, then run the Rust checks:

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Slow incremental acceptance tests that invoke R, Python, Julia, Doxygen, or
Rustdoc are ignored by default. The **Extended tests** workflow runs them
nightly. Run the workflow manually with `suite: incremental-native` or use
these local commands when changing native extraction or cache boundaries:

```sh
cargo test --locked --lib \
  persistent_discovery_and_doctests_have_independent_cache_entries -- --ignored
cargo test --locked \
  --test incremental_dependencies --test incremental_execution \
  --test incremental_preparation --test incremental_source_paths \
  --test r_incremental --test python_native -- --ignored
```

Regular CI splits Cargo integration targets into four shards. To see which
targets a shard runs, use
`python scripts/test-integration-shard.py integration-1 --list`.
Omit `--list` to run the shard.

Regular CI checks canonical snapshot fingerprints with
`serde_json/preserve_order`. The **Extended tests** workflow runs the full
alternate-feature suite nightly or on demand with `suite: snapshots`.

Keep fast regression tests in the default suite. New slow native acceptance
tests should remain opt-in rather than adding minutes to routine testing.
Run [pipeline benchmarks](https://github.com/jolars/diplodocus/blob/main/docs/development/pipeline-baseline.md)
manually when comparing changes to build or preview performance.

For presentation changes, install browser tools with `npm ci --ignore-scripts`,
run `npm run site-test`, and inspect screenshots from `npm run site-capture`.
Run `npm run vitepress-test` for changes to the external frontend integration.
The repository's [browser development guide](https://github.com/jolars/diplodocus/blob/main/docs/development/browser.md)
describes setup, preview, and capture options.

## Compare site generator performance

In the development shell, install the locked npm dependencies and build the
release binary before running the benchmark:

```sh
npm ci --ignore-scripts
cargo build --locked --release --bin diplodocus
npm run site-bench
```

Both tools build the same 50-page and 500-page Markdown sites with local search
enabled. The benchmark measures production build time, preview startup until a
guide paragraph is visible in Chromium, and the time from saving an edit until
the open tab shows the new text. Builds and preview startups use fresh project
workspaces. The trials share warmed system caches, and saves reuse the running
server and tab. The runner pauses 300 ms between saves so each watcher can settle.
Each tool runs once as a warm-up before five measured trials
per site size, with three measured saves per preview trial.

Run on an idle machine for comparable measurements. The report contains raw
timings, medians, ranges, corpus fingerprints, and tool and host details. It
goes under `artifacts/site-benchmark/` unless you supply `--output PATH`.
Set `DIPLODOCUS_BIN` to compare a different release binary, or use a short run to
check the setup:

```sh
npm run site-bench -- --pages 10 --samples 1 --saves 1
```

## Pull requests

Follow the existing Rust and JavaScript styles, document public Rust APIs, and
update the guides when behavior changes. Use short Conventional Commit titles.
Describe the resulting behavior in pull requests, link relevant issues, report
validation, and include screenshots for visual changes. Keep generated sites and
browser artifacts out of commits.
