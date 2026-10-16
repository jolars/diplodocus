# Quick start

Follow [installation](installation.md) to get the CLI and project checkout. Then
build the small four-language monorepo in `examples/monorepo/`. It has four
package APIs, a shared guide, search, and links between the Python, R, Julia,
and Rust versions of the same function. Devenv supplies the native runtimes and
the separate nightly Rust documentation toolchain needed to extract APIs and
verify their examples. Authored GFM pages remain display-only.

From the Diplodocus repository root:

```console
devenv shell
task preview
```

`task preview` builds Diplodocus and serves the example. Open the local URL
printed in the terminal. Edit a page under `examples/monorepo/docs` to try a
watched rebuild; successful rebuilds refresh the browser. Stop the server with
Ctrl-C. The equivalent direct commands are:

```console
cargo run --locked -- check --config examples/monorepo/diplodocus.toml
cargo run --locked -- build --config examples/monorepo/diplodocus.toml --output site/monorepo
cargo run --locked -- serve --config examples/monorepo/diplodocus.toml --output site/monorepo --port 0 --live-reload
```

`check` validates sources without writing a site. `build` writes the site, and
`serve` builds and watches it. See the [command reference](cli.md) for the
snapshot commands and defaults. Diplodocus does not install dependencies or
kernels for a workspace. To adapt the example, read the [workspace
configuration](configuration.md) and [authored-pages](authoring.md) guides.

## Build Diplodocus's own documentation

The root `diplodocus.toml` builds this guide as the homepage and an executable
Python example. The devenv shell supplies its `python3` kernel. From the
repository root:

```console
cargo run --locked -- check
cargo run --locked -- build
cargo run --locked -- serve
```

The generated homepage is `site/index.html`, and the preview listens at
`http://127.0.0.1:8000/` by default. `check` never executes cells; `build` and
`serve` execute cells only in collections that the configuration authorizes.

## Exercise the full acceptance workspace

The acceptance workspace at
`tests/fixtures/acceptance/workspace/diplodocus.toml` uses sibling core, Python,
and R repositories, plus executable Python and R pages. The devenv shell
supplies both kernels. This workspace is for broader validation after the small
example and project site:

```console
cargo run --locked -- check --config tests/fixtures/acceptance/workspace/diplodocus.toml
cargo run --locked -- build --config tests/fixtures/acceptance/workspace/diplodocus.toml --output site/acceptance
```

Authorized cells run with your user permissions. Review a workspace's sources
and [execution settings](execution.md) before building it. Document metadata
cannot grant execution authority.
