# Design

A project might have a C++ library, Python bindings, and an R package. Each API
has its own language conventions, but readers often need to move between them.
Diplodocus brings their API references and authored guides into one site with
shared navigation, links, and search.

## Describe the workspace

The workspace configuration tells Diplodocus which checkouts to read, which
packages to document, and where to find authored pages. The packages can share
a repository or come from different checkouts. You prepare the checkouts and
choose their revisions.

A package represents an API. For example, a Python module that calls C++ can
have its own Python reference. If you also publish the C++ interface, declare
it as a separate package. Shared guides live in the configuration checkout.
Packages can also publish guides from their own sources. The owner gives a
guide its context for navigation and links, and the collection's mount
determines its URL.

## Extract APIs in a prepared environment {#extraction-works-in-a-prepared-environment}

Each language adapter reads sources and uses the tools that understand that
language. In this example, Doxygen supplies C++ declarations and comment text,
Python helps discover the loaded package's public API, and R supplies exports
and help topics from the prepared checkout. Julia uses its runtime to discover
methods and docstrings, and Rust uses rustdoc to obtain the compiler-selected
API.

The adapters hand structured documentation, source locations, and diagnostics
to Diplodocus. You supply their runtimes, dependencies, and any required build
artifacts. Some tools run code even during discovery. Python and R can load
packages, Rust can compile dependencies and run build scripts, and Doxygen can
invoke configured filters. The [language guides](../guide/extraction.md)
describe how to prepare each environment.

## Put guides and APIs in one model

Diplodocus stores API declarations and authored pages in a common documentation
model. This lets a Python tutorial link to a C++ class or a package guide link
to an R help topic. Each declaration keeps its own identity within its package.
A reexport can refer to the same underlying item, and individual overloads
remain addressable. Diplodocus resolves those references before choosing site
URLs, so changes to navigation or routes can keep the links intact.

The model keeps the information each language needs. A C++ callable can have
overloads with different signatures, a Julia function can have several methods,
and an R generic can dispatch to methods for different classes. All three can
share site navigation and search while presenting their API on its own terms.

`check` validates sources and resolves links without running documentation
examples. `extract` also runs the examples required by the configured sources.
For example, extraction checks Python doctests and evaluates R vignettes.
Authored QMD cells run when their collection enables execution. Code that runs
during discovery or execution has the permissions of the process building the
site.

## Save a snapshot and generate the site

`extract` writes a [portable snapshot](../guide/snapshots.md), a SQLite file
with the assembled documentation, resolved references, execution results, and
local assets. You can move that file to another machine and run `generate`
using only the snapshot. This lets CI extract documentation in a prepared
environment and generate it elsewhere. Other frontends can also use exported
HTML and Markdown fragments or read the [snapshot format](../reference/snapshots.md)
directly.

`generate` uses the snapshot to assign page URLs and build navigation and
search. It renders a complete static site. `build` runs extraction and
generation in sequence for the common case where you want the site immediately.

## Rebuild during preview {#incremental-work-retains-verified-results}

`serve` watches sources and rebuilds the site when you save. Diplodocus can
reuse completed work for a package or page when the tool, source, and observed
inputs still match. It checks links again and regenerates navigation and search
for the whole site. A failed build keeps the last successful site available so
you can fix the source and save again.

See [Contributing](../../CONTRIBUTING.md) for development commands and browser
checks.
