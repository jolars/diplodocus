# R/Python monorepo example

Tiny Stats is a working prediction-evaluation toolkit with Python and R
packages. Both expose six operations: mean squared error, mean absolute error,
root mean squared error, mean error, R squared, and residuals.

The site combines installation and quick-start pages, three shared guides, a
Python-specific workflow, and both package references in one left sidebar.
Python functions nest under their module; the current package and API ancestors
open automatically. A separate **On this page** area follows the page's headings.
Each operation links to its equivalent in the other language.

```text
monorepo/
  diplodocus.toml
  docs/
    index.md
    getting-started/
      index.md
      installation.md
      quick-start.md
    guides/
      index.md
      comparing-predictions.md
      choosing-a-metric.md
      inspecting-residuals.md
    assets/errors.svg
  python/
    pyproject.toml
    src/tinystats/__init__.py
    docs/evaluating-models.md
    tests/test_metrics.py
  r/
    DESCRIPTION
    LICENSE
    NAMESPACE
    R/
    man/
    tests/metrics.R
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

Start with the quick start, compare the metrics in the shared guides, and open
a function under **Reference**. Then search for
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

## Check the package calculations

From the repository root inside `devenv shell`, run the Python tests and examples:

```console
PYTHONPATH=examples/monorepo/python/src python -m unittest discover -s examples/monorepo/python/tests
python -m doctest examples/monorepo/python/src/tinystats/__init__.py
```

After installing the R package into your R library, run:

```console
Rscript examples/monorepo/r/tests/metrics.R
```

These tests check known scores, perfect predictions, invalid input lengths,
constant observations for R squared, and missing-value propagation.

## Adapt the example

Copy this directory into your project and edit `diplodocus.toml`. Both packages
refer to the same repository declaration, whose path is `.`. Package paths are
relative to that repository, and extraction targets are relative to each
package. Shared pages mount at the site root; package references live under
`packages/python/` and `packages/r/`. The Python-owned collection mounts its
guide at `packages/python/guides/`. Links between collections use ordinary
relative source paths and are rewritten to the generated page routes.

Update the package metadata, source paths, semantic references, and concept
members together when renaming the packages or functions. Keep execution
disabled for a static site like this one.
