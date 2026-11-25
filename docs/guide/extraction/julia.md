# Julia API extraction

Declare `ecosystem: "julia"`, `metadata-path: "Project.toml"`, and a target with
`extractor: "julia"` and the package entry file, such as
`path: "src/MyPackage.jl"`. Prepare Julia 1.10 through 1.13 on `PATH` and the
package's dependencies in its project. Diplodocus uses Julia's standard library
for discovery. The caller prepares dependencies and compiled artifacts.

`check`, `extract`, `build`, and `serve` load and verify that checkout to
discover its API and registered documentation. This includes methods defined by
macros and documentation attached at runtime. Package initialization runs when
Julia loads the checkout. `check` validates discovery, links, and presentation
with documentation examples kept inert. `generate` reads completed snapshots
without Julia or the checkout.

Exported and `public` bindings are discoverable. The `public` keyword requires
Julia 1.11 or later. Documented private declarations remain available through
direct references and authored selections. An alias resolves to its canonical
item. Functions group methods on the family page, and types retain their fields
and explicit constructors. Fatou supplies authored signatures and byte ranges
where they agree with a runtime declaration. A runtime-only method can be
selected by its reflected dispatch, for example `generated(::Int64)`.

To document optional extension implementations, list the extension entry files
in `julia.extensions`, such as `[ext/MyPackageMakieExt.jl]`. Diplodocus reads
those files and their literal includes as source. Their methods join the loaded
package's generics, even when the weak dependencies are unavailable. See
[explicit extension
sources](https://github.com/jolars/diplodocus/blob/main/docs/ir/julia-extraction.md#explicit-extension-sources).

During extraction, the native doctest adapter runs Julia `jldoctest` fences in
the package project. It supports REPL transcripts, scripts with `# output`,
expected errors, named blocks that share state within a document, and
block-level `setup`, `teardown`, and `output` options. Anonymous blocks get
fresh modules and separate documents use separate processes. REPL semicolons
suppress display. Multiline input uses seven spaces for continuation lines.
Hidden output still participates in comparison. Unsupported options, including
filters and `@meta` setup, fail extraction. Ordinary Julia fences are
illustrative. The [Julia extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/julia-extraction.md)
defines the supported documentation and dispatch syntax.

Discovery and doctests reuse completed results independently when package
sources and the prepared Julia environment still agree. Checks cover observed
source files, project files, manifests, preferences, resolved dependencies,
artifacts, and runtime libraries. Declare other reads and environment choices
through the [incremental settings](../../reference/configuration.md). Use
`incremental.cache: off` for uncontrolled external state, or `--no-cache` to
force fresh discovery and execution.

## Documenter pages {#documenter-pages}

To publish Julia Markdown pages with Documenter syntax, declare a collection
with `format: documenter` and point its `path` to the directory of `.md` files.
Use top-level `content` for shared guides or the package's `content` list for package guides. Use `@ref` for links, `@docs` to insert documentation for selected
bindings or methods, `@autodocs` to select documented items from named modules,
and `@index` to list the entries assembled in the collection.

For example, `[setup](@ref install)` links to a heading with the
`@id install` anchor. An `@autodocs` block requires `Modules` and can filter
by `Pages`, `Public`, and `Private`.

The collection displays Julia example fences as code by default. To run
`@example` blocks during extraction, set `mode: execute`, `engine: julia`, and
`julia-project` under the collection's
[`execution` settings](../../reference/configuration.md#collection-execution).
Point `julia-project` to a prepared project file in the repository. Each page
uses a fresh Julia process with the builder's permissions. Blocks with the same
name share state on that page. A trailing `# hide` runs the line while hiding it
from the displayed source. `diplodocus check` validates examples without running
them. API docstring doctests follow the separate extraction workflow above.
