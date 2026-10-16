# Python, R, Julia, and Rust monorepo example

Tiny Stats is a working prediction-evaluation toolkit with Python, R, Julia, and Rust
packages. All four expose six operations: mean squared error, mean absolute error,
root mean squared error, mean error, R squared, and residuals.

The site combines installation and quick-start pages, three shared guides, a
Python-specific workflow, an executable R vignette, and all four package
references in one left sidebar.
Python functions nest under their module; the current package and API ancestors
open automatically. A separate **On this page** area follows the page's headings.
Each operation links to its equivalent in the other languages.

Rust uses Cargo and the separate nightly documentation toolchain supplied by
devenv. Its `Score` type demonstrates grouped fields and methods. Native Rust
doctests execute during extraction, while `check` compiles and resolves the API
without executing them. Rust compilation and build scripts run offline against
the supplied lockfile and cached dependencies.

The navbar's home link displays `docs/assets/logo.svg` before the site title.
The `[presentation.logo]` table selects this image from the `tinystats` repository.

The R package has a landing page built from its README and DESCRIPTION. Package
information links to release notes, authors and citation information, the
license, and community guidelines. These pages require no content collection.
Diplodocus also discovers `r/vignettes/evaluating-predictions.Rmd` automatically.
Its R chunks compute scores and a table, and draw a residual plot.

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
  julia/
    Project.toml
    src/TinyStats.jl
    src/metrics.jl
  r/
    README.md
    NEWS.md
    DESCRIPTION
    LICENSE
    LICENSE.md
    CONTRIBUTING.md
    CODE_OF_CONDUCT.md
    inst/CITATION
    NAMESPACE
    R/
    man/
    tests/metrics.R
    vignettes/evaluating-predictions.Rmd
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

The shared and Python collections explicitly set `mode = "never"`. Building
requires R on `PATH`, with `pkgload`, `knitr`, and `evaluate` available; devenv
provides these packages. Extraction loads the R checkout to execute Rd examples
and vignette chunks, and collects author and citation information. This example
needs Python and Julia on `PATH` to verify API doctests, alongside R. It needs
no package installation or Jupyter kernel. Python
documentation comes from NumPy-style docstrings. R documentation comes from
the checked-in `man/*.Rd` files; Diplodocus does not run roxygen2. Julia documentation comes from Julia Markdown
docstrings. Function pages include method signatures and prose, with anchors for
links to individual methods. Extraction verifies Python and Julia doctest output.

The current R parser emits an `r-rd-source-attribution` warning because it tracks
Rd content at file granularity. This warning does not prevent the site from
building.

The R README's code fences remain display-only. The vignette's chunks and inline
R expressions execute with knitr, and Diplodocus renders the collected Markdown,
table, and figure. Extraction also evaluates `Authors@R`
and `inst/CITATION` with R and stores the results in the snapshot. `check` validates
their syntax without evaluating them. Generation from a snapshot needs no R.

## Generate from a snapshot

Extraction and generation can run separately:

```console
cargo run --locked -- extract --config examples/monorepo/diplodocus.toml
cargo run --locked -- generate --input examples/monorepo/.diplodocus/documentation.sqlite --output site/monorepo
```

You can copy `documentation.sqlite` elsewhere and pass it to an already-built
`diplodocus generate` binary. The snapshot contains the documentation and local
images and evaluated vignette; generation needs neither this example's sources
nor language runtimes.

The [integration test](../../tests/monorepo_example.rs) copies the source example
into a temporary directory, checks the API links, vignette, and assets, and
compares the built site with one generated after the temporary sources have been
removed. Generation runs with an empty `PATH`.

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

Copy this directory into your project and edit `diplodocus.toml`. All packages
refer to the same repository declaration, whose path is `.`. Package paths are
relative to that repository, and extraction targets are relative to each
package. Shared pages mount at the site root; package references live under
`packages/python/`, `packages/r/`, and `packages/julia/`. The Python-owned collection mounts its
guide at `packages/python/guides/`. The R overview lives at `packages/r/`, with
`news.html`, `authors.html`, `license.html`, and `community.html` beneath it.
The executable vignette lives at `packages/r/vignettes/evaluating-predictions.html`.
Links between collections and automatically discovered package pages use ordinary
relative source paths and are rewritten to the generated page routes.

Update the package metadata, source paths, semantic references, and concept
members together when renaming the packages or functions. Keep `mode = "never"`
for display-only guides; R Markdown vignettes execute during extraction.
