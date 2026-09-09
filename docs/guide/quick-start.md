# Quick start

The build, check, and preview commands below describe the intended MVP workflow.
They currently return a not-implemented error. Corpus checks and parser/kernel
tests can already be run from this checkout.

## Prepare the declared environment

Start in the repository root with the declared tools already installed. The
devenv environment supplies the pinned Rust toolchain, Python with ipykernel,
R with IRkernel, and the `python3` and `ir` kernelspecs. Provisioning that
environment may require network access before beginning this workflow.

```console
devenv shell
cargo test --locked --all-targets
```

Once the environment and Cargo dependencies are available, the documentation
commands and deterministic examples require no network access. Diplodocus
never installs a dependency or kernel for you.

## Build the acceptance site

The acceptance workspace contains sibling core, Python, and R repositories.
Its default configuration selects supported inputs. Diagnostic cases are
separate overlays used only in disposable test workspaces.

```console
cargo run --locked -- check --config tests/fixtures/acceptance/workspace/diplodocus.toml
cargo run --locked -- build --config tests/fixtures/acceptance/workspace/diplodocus.toml --output site/acceptance
cargo run --locked -- serve --config tests/fixtures/acceptance/workspace/diplodocus.toml --output site/acceptance --host 127.0.0.1 --port 8000
```

Open `http://127.0.0.1:8000/`. The site combines the project guide, both package
APIs, shared concepts, and executable Python and R pages. Stop the preview with
Ctrl-C before starting the next one.

## Build the project site

The root configuration describes Diplodocus's own authored documentation. It
has no API extraction targets. From the repository root:

```console
cargo run --locked -- check
cargo run --locked -- build
cargo run --locked -- serve
```

The default output directory is `site`, and the default preview address is
`http://127.0.0.1:8000/`. Edit a page in this guide to exercise watched rebuilds.
A failed rebuild must leave the previous successful site available and print
the new diagnostics.

## Understand execution authority

`check` never executes cells. `build` and `serve` execute only QMD collections
enabled in configuration. GFM fences and collections without execution settings
remain display content.

Authorized code runs as arbitrary, unsandboxed code with your user permissions.
Review source and configuration before building an executing collection.
Document metadata cannot grant execution permission. The included examples use
deterministic local inputs and require no documented package to be installed.

See [configuration](configuration.md) for collection settings and
[commands](cli.md) for defaults and failure behavior.
