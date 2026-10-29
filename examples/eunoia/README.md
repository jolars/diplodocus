# Package documentation inside Eunoia

## Rust acceptance run

The Rust acceptance recipe documents the core `eunoia` crate with its default
features, executes native doctests, checks representative public APIs, removes
the disposable source copy, and generates both a standalone site and HTML and
Markdown fragments from the SQLite snapshot:

```console
devenv shell -- python examples/eunoia/rust.py --source ../eunoia
```

The prepared environment needs Eunoia's locked Cargo dependencies in its cache.
Devenv supplies the separate pinned nightly documentation toolchain. The recipe
prints its temporary artifact directory and coverage counts. Use `--output` to
select a fresh directory or `--diplodocus-bin` to select an existing executable.
It does not modify Eunoia or integrate the generated pages into its website.

## Julia acceptance run

The Julia recipe checks Eunoia.jl's 23 exported bindings, including
`eunoiaplot!`, result fields, callable relationships, API references, and list
formatting. It copies the package into a disposable directory and extracts a
SQLite snapshot. It then relocates the snapshot, removes the staged sources and
configuration, and generates a standalone site and HTML and Markdown fragments
with an empty `PATH` and Julia environment settings removed.

```console
devenv shell -- compute run -- python examples/eunoia/julia.py --source ../Eunoia.jl
```

Prepare Julia and the package's dependencies before running the recipe. The
native library must be available through an installed Eunoia artifact or an
absolute `EUNOIA_CAPI_LIB` path. Extraction loads the staged package; the recipe
does not install or resolve dependencies. Python 3.11 or later runs the recipe,
and Cargo builds Diplodocus unless `--diplodocus-bin` selects an executable. Use
`--output` to select a fresh directory outside the Eunoia.jl checkout.

The output includes `summary.json`, full snapshot diagnostics in
`diagnostics.json`, and a `detached/` directory containing the relocated
snapshot, site, original fragments, URL map, and mapped fragments. The recipe
maps every document and asset, checks local links and matching HTML and Markdown
anchors, and reports coverage counts. Its numbered routes under
`/docs/reference/julia/` are acceptance URLs; Eunoia can assign its own routes
when integrating the fragments.

The current Fatou parser leaves Eunoia's two fenced examples inside `euler`'s
list items as literal fence text. The summary reports list items containing
literal fences separately from extraction diagnostics. Indented code blocks
inside lists retain their structure.

This run covers the core API in Eunoia.jl 1.0.1. A recipe-owned GFM overview
replaces README discovery in the disposable copy. Documenter guides, gallery
execution and figures, Makie extension methods, public-only navigation, and
field grouping remain follow-up work. The recipe leaves the original package and
Eunoia's website untouched.

## Python and R embedding experiment

Eunoia now integrates the complete Python API through the supported
`generate --format fragments` command, including mapped HTML and Markdown
links. See Eunoia's `web/PYTHON-DOCS.md` for that build. This directory retains
the original disposable Python/R experiment and its custom SQL example.

This experiment uses a portable Diplodocus SQLite snapshot in a disposable copy
of Eunoia's existing SvelteKit website. Eunoia supplies its layout, navigation,
routes, and interactive components. Diplodocus supplies package documentation.
The experiment does not modify or publish the Eunoia checkout.

It exercises two independent ways to consume a snapshot:

- A custom package index queries package names, versions, and API item counts
  directly through Python's standard SQLite library. It also offers the original
  database as a download.
- Reference pages reuse `render_fragments`, an experimental Rust API that emits
  document bodies, semantic identity and heading metadata, search records,
  syntax colors, and retained assets. The small host adapter maps document and
  asset URLs into Eunoia's existing routes.

Neither path requires a runtime database server. SvelteKit prerenders the pages.
Consumers can instead render the documented SQL records themselves when they
need more control than the default fragments provide.

## Run the experiment

From the Diplodocus checkout, enter `devenv shell`. Supply sibling checkouts of
`eunoia`, `eunoia-py`, and `eulerr`, or specify their paths with `--eunoia`,
`--python`, and `--r`. Eunoia must already have its web dependencies and generated
`npm/` package. Eulerr must have its compiled checkout extension in `src/`.
The shell supplies `pkgload` and `evaluate` for its Rd examples.

```console
python examples/eunoia/experiment.py
```

The script uses temporary directories, builds Diplodocus, extracts the selected
package documentation, creates the host pages, and runs Eunoia's `pnpm check`
and `pnpm build`. Use `--pnpm /path/to/pnpm` to select an installed pnpm 10
executable when the system default has a different major version. The script
uses existing dependencies and disables pnpm's automatic executable download.
Choose a fresh output directory on each run. Keep source staging outside
`.diplodocus`, whose contents source discovery intentionally excludes.

The script prints its output directory and preview command. With the preview
running, visit:

- `/docs/reference/packages/`: the custom SQL package index.
- `/docs/reference/python/euler/`: the Python callable and its five overloads.
- `/docs/reference/r/euler/`: the R generic, methods, and examples.
- `/docs/reference/r/plot.euler/`: the plotting reference and generated figures.
- `/docs/concepts/fitter-pipeline/`: Eunoia's existing interactive Svelte example.

To consume an already extracted database without preparing or executing packages:

```console
python examples/eunoia/experiment.py --snapshot /path/to/documentation.sqlite
```

The fragment export can also run independently:

```console
cargo run --locked --example export-fragments -- \
  --input documentation.sqlite --output fragments
```

## Verification

The host adapter checks route collisions and rejects local links it cannot map.
Its focused tests run with:

```console
python -m unittest discover -s examples/eunoia -p 'test_*.py'
```

With the generated preview running, use the devenv browser environment to verify
desktop and mobile layouts, cross-language navigation, figures, source links,
the existing Svelte component, rendering without JavaScript, and the downloadable
database. Screenshots go to the specified directory:

```console
node examples/eunoia/browser.mjs http://127.0.0.1:4173 /tmp/eunoia-screenshots
```

The Rust `fragments` integration test renders from a database after deleting its
source copy. It checks the absence of site chrome, preservation of overloads and
highlighted code, and exact retained asset bytes.

## Scope and findings

The initial real-package trial produced 99 API records, 103 rendered documents,
and 13 retained PNG figures. The Python records cover one callable, five
overloads, and two modules. The R records cover the package API, including its
native help-topic and method distinctions. An explicit `analogous` concept links
the two `euler` implementations.

The Python source slice contains the unchanged `_fit.py` and a generated
`__init__.py` that exports only `euler`. This is a documentation slice, not a
complete runnable Python package. The initial trial exposed unsupported
type-checking imports and dataclass options; the Python adapter now handles
those constructs and extracts the untouched package. The fitting
docstring has no executable doctest syntax, so this experiment provides no
Python execution coverage.

The R copy retains its native sources, extension, and Rd topics. It excludes
vignettes and the README to keep the trial on API references. The README's remote
badge images currently fail reference validation. Rd examples execute through
the normal extraction pipeline, and their validated figures travel in SQLite.

The snapshot is sufficient for the host build after the staged package sources
become unavailable. Building Eunoia itself still requires its own frontend
dependencies and WASM package.

The experiment supports keeping SQLite as an independent consumption contract.
The supported fragment exporter now provides URL mapping, scoped syntax styles,
and Markdown representations. This experiment's adapter still remaps its own
search records and does not add a search UI or Markdown representations to its
disposable website. It does not establish a framework plugin API.
