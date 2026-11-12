# Design

Diplodocus brings related packages into one documentation site. A library might
have a C++ core, Python bindings, and an R package, each with its own
conventions and release cycle. Readers should be able to move between those
packages without learning a different documentation system for each one.

That goal shapes the architecture. Diplodocus extracts APIs and authored pages
into a shared documentation model, resolves their relationships, and renders
them together. Navigation, search, links, and presentation belong to the whole
site. Each language adapter contributes structured documentation rather than
pages from a separate documentation generator.

## A workspace describes the documentation

A workspace explicitly names its source repositories, packages, and authored
content collections. These can live in one monorepo or several local checkouts.
The caller supplies those checkouts and chooses their revisions. Diplodocus does
not fetch repositories or resolve a set of package releases.

A package represents a documented API, so its language follows that API. A
Python package implemented with a Rust extension is usually one Python
documentation target. The Rust interface can be documented separately when it is
useful to readers. Packages can also be internal or hidden while remaining
available as semantic link targets.

Content ownership is independent of file location. A project-level tutorial can
live beside one package's source, and a package guide can live in a shared
documentation repository. Ownership gives pages their navigation and reference
context; collection mounts determine where they appear on the site.

## One model preserves language differences

The intermediate representation, or IR, describes packages, API items,
signatures, documents, and relationships. API documentation and authored guides
share document blocks and inline elements, so both participate in the same
reference resolution, search, and rendering.

The common model leaves room for language-specific meaning. A Julia generic
function has methods; a Python callable can have overloads; an R generic has
dispatch relationships; a Rust trait has associated items. Adapters retain those
distinctions in structured signatures and language records. The renderer can
present them appropriately within a common visual system.

Semantic identities also stay separate from names and URLs. An API item has a
stable identity scoped to its package. Reexports can name the same item, and
overloads remain individually addressable. Rendering chooses routes later, so a
presentation change does not redefine what a reference means. Authors can group
corresponding APIs into explicit concepts, marking them as equivalent,
analogous, or related without assuming that similar names mean the same thing.

Source evidence accompanies the model. Locations identify a repository and a
relative path, with ranges where the input supports them. Runtime observations
remain distinguishable from source declarations. Adapters report unsupported or
ambiguous constructs rather than silently inventing an interpretation.

## Extraction works in a prepared environment

The caller prepares the runtimes, dependencies, and build artifacts needed to
use the documented packages. Diplodocus combines source analysis with native
tools where they help establish API semantics or check examples. Python source
and stubs supply declarations and documentation; native Python verifies the
loaded checkout and doctests. R combines maintained source and Rd with native
package loading and example execution. Julia combines source parsing with
runtime type completion and doctests. Rust uses rustdoc JSON, and C++ uses
Doxygen XML.

These integrations stop short of delegating the site to another generator.
Rustdoc and Doxygen supply API structure; their HTML does not become the site.
Parser and native-tool details stay inside adapters that return Diplodocus-owned
records and diagnostics.

Diplodocus does not install dependencies or kernels. Rust is an exception to the
usual requirement for prepared build artifacts: its native documentation tools
may compile dependencies and run build scripts, using a prepared nightly
toolchain and Cargo's locked, offline mode. The language guides describe the
[requirements and supported behavior](../guide/extraction.md).

## Authored pages join the same pipeline

Markdown parsing runs in-process through Panache and translates directly into
the document IR. Named authoring profiles define the supported GFM, QMD, R
Markdown, and Documenter surfaces. Compatibility with a profile does not imply
support for every extension of its upstream tool.

Includes compose source before document preparation. The composed page keeps a
map back to the original files, so a diagnostic in a partial identifies that
partial, and its relative images and links resolve from its own directory.
Including a file does not create another page or change the owning page's
metadata or execution environment.

Execution follows the input format. API doctests and Rd examples run during
extraction. R Markdown articles use native knitr execution. QMD and Documenter
pages currently require explicit collection execution settings. A QMD page uses
one Jupyter session, allowing later cells to use earlier state. Separate pages
have separate sessions. Expected-output mismatches, unexpected errors, missing
prerequisites, and cleanup failures prevent publication.

Execution produces structured results and retained assets. Ordinary output stays
text; supported rich output passes validation before it can render. Generated
Markdown is an inert fragment and cannot introduce more executable cells. Stored
HTML must pass the active sanitizer again when loaded. These boundaries protect
published content; package code and examples still run with the caller's
privileges and can change files or use the network.

The QMD cache stores successful whole-page results because cells can depend on
earlier state. Its identity covers source, effective options, kernel and
toolchain identity, validation policy, and declared environment inputs. A hit
still verifies kernel readiness and revalidates the retained outputs. The cache
is disposable local state, separate from the portable snapshot.

## A snapshot separates extraction from generation

`extract` assembles and validates the documentation, completes execution, and
publishes a standalone SQLite snapshot. It contains the structured records,
resolved references, presentation defaults, provenance, and local asset bytes.
Source locations remain evidence rather than files that generation must open.

`generate` reads that completed snapshot without the checkouts, configuration,
language runtimes, or execution caches. This lets extraction run in a prepared
CI environment and generation run elsewhere. It also makes the snapshot useful
to other frontends: consumers can query its records or use the exported HTML and
Markdown fragments. The [snapshot guide](../guide/snapshots.md) describes those
workflows, and the [format reference](../reference/snapshots.md) covers direct
SQL access.

Snapshot compatibility has separate versions for storage, semantic IR, and
canonical encoding. Readers reject unsupported versions. Content fingerprints
describe logical records and assets, independent of SQLite's physical layout.
Refreshing a snapshot replaces the complete result, preserving retained semantic
identities and removing deleted entities and unused assets.

A snapshot preserves the results obtained, including execution provenance. It
cannot make arbitrary code deterministic: reproducibility still depends on
controlling randomness, time, network access, and other external inputs.

## Rendering and publication finish the build

Generation validates the snapshot and constructs a site model with routes,
navigation, search entries, and presentation settings. The renderer consumes
that model without parsing language source or querying SQLite. The default
output is a static site with complete HTML pages, local assets, and Markdown
representations. Browser navigation and search enhance those pages.

Authored browser components are a separate explicit capability. Authors supply
prebuilt bundles and a Markdown fallback; Diplodocus retains the bundles but
does not compile them. Generating from a snapshot requires the caller to enable
the components. Notebook output does not acquire this ability.

Publication stages completed artifacts before replacing their destinations.
Failed extraction preserves the previous snapshot, and failed generation
preserves the previous site. `build` runs extraction and generation in sequence;
`serve` watches inputs and keeps the last successful site available after a
failed rebuild. Publication cannot undo side effects from executed code.

`check` validates sources and references without running documentation examples
or publishing artifacts. It can still invoke native extraction tools, including
Rust compilation and Doxygen filters. It does not prove that example execution
will succeed.

The boundaries between adapters, assembly, snapshots, the site model, and
rendering also organize the code in `src/`. They allow changes to one language
or frontend without making every other part understand its implementation.
External plugin interfaces, incremental generation, and historical release
assembly remain future work; the current design centers on one complete
documentation snapshot of the supplied sources.
