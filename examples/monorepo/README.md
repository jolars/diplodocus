# R/Python monorepo example

Tiny Stats is a small documentation site for two packages in one repository.
Each package exposes `mean_squared_error`. A shared guide links to both API
references, and an equivalent concept adds **Same API in** links between them.

```text
monorepo/
  diplodocus.toml
  docs/
    index.md
    comparing-predictions.md
    assets/errors.svg
  python/
    pyproject.toml
    src/tinystats/__init__.py
  r/
    DESCRIPTION
    LICENSE
    NAMESPACE
    R/mean_squared_error.R
    man/mean_squared_error.Rd
```

## Build and preview

From the **Diplodocus repository root**, enter `devenv shell` and run:

```console
task preview
```

This builds Diplodocus, builds the example site at `site/monorepo`, and serves it
on an available local port. Open the `Serving http://127.0.0.1:...` URL printed
in the terminal. Edit a guide page to try watched rebuilds; the browser refreshes
after a successful rebuild. Stop the server with Ctrl-C. To choose a port, run
`DIPLODOCUS_SITE_PORT=8001 task preview`.

The equivalent commands without go-task are:

```console
cargo run --locked -- check --config examples/monorepo/diplodocus.toml
cargo run --locked -- build --config examples/monorepo/diplodocus.toml --output site/monorepo
cargo run --locked -- serve --config examples/monorepo/diplodocus.toml --output site/monorepo --port 0 --live-reload
```

Browse the guide and both package references, then search for
`mean_squared_error` or follow **Same API in** on a function page.

The collection explicitly sets `mode = "never"`. Once Diplodocus is built, these
commands need no Python, R, Jupyter kernels, or package installation. Python
documentation comes from NumPy-style docstrings. R documentation comes from
the checked-in `man/*.Rd` files; Diplodocus does not run roxygen2.

The current R parser emits an `r-rd-source-attribution` warning because it tracks
Rd content at file granularity. This warning does not prevent the site from
building.

## Generate from a snapshot

Extraction and generation can run separately:

```console
cargo run --locked -- extract --config examples/monorepo/diplodocus.toml
cargo run --locked -- generate --input examples/monorepo/.diplodocus/documentation.sqlite --output site/monorepo
```

You can copy `documentation.sqlite` elsewhere and pass it to an already-built
`diplodocus generate` binary. The snapshot contains the documentation and local
image; generation needs neither this example's sources nor language runtimes.

The [integration test](../../tests/monorepo_example.rs) copies the source example
into a temporary directory, runs the commands with an empty `PATH`, checks the
API links and asset, and compares the built site with one generated after the
temporary sources have been removed.

## Adapt the example

Copy this directory into your project and edit `diplodocus.toml`. Both packages
refer to the same repository declaration, whose path is `.`. Package paths are
relative to that repository, and extraction targets are relative to each
package. The shared guide mounts at the site root; package references live
under `packages/python/` and `packages/r/`.

Update the package metadata, source paths, semantic references, and concept
members together when renaming the packages or functions. Keep execution
disabled for a static site like this one.
