# Julia extraction contract

Julia supplies bindings, dispatch, type information, and registered
documentation from the loaded checkout. Diplodocus builds the portable API
records, resolves references, and renders pages. Fatou supplies Julia Markdown,
authored signatures, and source maps.

## Native discovery

Diplodocus starts Julia 1.10 through 1.13 with startup and history files
disabled and activates the configured package project. The caller prepares
Julia, the package's dependencies, and any compiled artifacts. Julia loads the
package by its project name and UUID. The helper verifies the name, UUID,
available version, and entry file against the supplied checkout.

The private `julia-native-v1` TOML exporter reports modules, bindings, aliases,
method dispatch, fields, registered documentation, actual input files, and Julia
and dependency versions. Julia's binding and method introspection supplies the
surface. A small compatibility layer reads exact `Base.Docs` attachments, method
metadata, and loader source observations. The GitHub Actions version matrix
checks this interface on every supported Julia minor. Discovery uses the
standard library. Authored Documenter pages and example execution use
Diplodocus's [Documenter support](../guide/authoring/documenter.md).

The CLI commands and Rust library workspace assembly share native discovery.
`check` loads the package and validates references and presentation. Extraction commands also
verify the retained doctests. Package loading, unsupported native declarations,
input changes, cancellation, and timeouts prevent publication. Package
initialization can write to the checkout.

The loader reports which files it included, even for computed paths, and the
exporter verifies the source bytes for precompiled checkouts. Files are resolved
inside the configured package. Portable evidence contains repository-relative
paths and uses byte ranges only for verified material. Cache reuse additionally
requires coverage by the prepared environment and package input inventories.
Untracked external reads and environment variables can be supplied through the
incremental settings.

## Configuration and inputs

```yaml
packages:
  - id: "tinystats"
    name: "Tiny Stats for Julia"
    slug: "julia"
    ecosystem: "julia"
    repository: "repo"
    path: "julia"
    metadata-path: "Project.toml"
    targets:
      - id: "api"
        extractor: "julia"
        path: "src/TinyStats.jl"
        role: "public-api"
```

The target names the package's explicit entry file. Package metadata requires a
name and UUID. A supplied version must be SemVer. Authors, dependency UUIDs,
compatibility strings, and original top-level metadata fields retain their TOML
values and source ranges. Julia resolves the prepared environment using its own
project and manifest rules.

### Explicit extension sources

Supply extension entry files on the same target as the package entry:

```yaml
targets:
  - id: api
    extractor: julia
    path: src/Eunoia.jl
    role: public-api
    julia:
      extensions:
        - ext/EunoiaMakieExt.jl
```

Each entry names a unique `.jl` file relative to the package root. Both
`ext/NameExt.jl` and `ext/NameExt/NameExt.jl` layouts work. The adapter reads
each selected file and its unconditional literal includes with Fatou. Lexical
paths and resolved symlinks must stay inside the package. Missing files, cycles,
transformed includes, and unknown include paths in extension sources produce
diagnostics.

Imports such as `import Eunoia: eunoiaplot` join the selected methods to the
loaded generic. Its core fallback methods and generic prose remain available.
Method docstrings keep their own attribution. Conflicting dispatch identities
produce errors. This selection describes the authored extension surface and
works with an environment that omits weak dependencies. Removing an entry
removes its contributed methods at the next discovery.

## Maintained surface

The adapter retains exported and documented bindings, as well as those marked
`public` on Julia 1.11 and later. It includes the modules and fields needed to
represent those items. Callable families retain the loaded package's methods,
including methods introduced by metaprogramming. Exported or documented aliases
of an external generic require at least one package-owned method. Private
extensions of external generics can also be retained through registered method
documentation.

Module, function, type, macro, and constant bindings have canonical identities.
Imports and aliases retain their lookup names. Type information includes fields,
type parameters, and supertypes. An unsupported public binding or a
documentation attachment with ambiguous ownership produces a diagnostic.
Undocumented private helpers stay outside the retained surface.

## Automatic API discovery

Retention and discovery serve different purposes. A docstring retains a private
declaration for direct references and authored documentation; it does not make
that declaration part of automatic API navigation, search results, or indexes.
Exported bindings and bindings marked with `public` are discoverable, including
undocumented bindings. An exported or public alias makes its canonical item
discoverable. Aliases keep the canonical identity, display name, and route;
search includes the canonical name and retained alias spellings without creating
duplicate results.

Methods inherit discovery from their callable family, and explicit constructors
inherit it from their type. They remain grouped on the family or type page, with
searchable method prose and stable method anchors. An extension of an external
generic is private unless a supplied export or `public` declaration exposes its
binding or an alias. Mentioning a public type in a method signature does not
make the external family discoverable.

The package root and retained enclosing modules of discoverable items remain
discoverable as containers. Explicitly exported or public modules are also
discoverable, but their private children are not exposed automatically. Fields
never receive automatic index entries, including fields of public types. Fields
appear in declaration order in a Fields section on their owning type's page,
with annotations, retained documentation, and stable `field-<name>` anchors.
HTML and Markdown use the same grouping. Field references and host URL maps
target the type page and field anchor; separate field pages are not generated.
The type's fragment descriptor retains field identities and headings, and its
search entry includes field names and documentation.

Retained private items keep their HTML and Markdown content, semantic
identities, routes, and direct references. Fragment descriptors include private
pages with `visible: false`, including private types with their fields.

`Package::discoverable_items` derives this policy from retained declaration and
ownership facts. `Item::discoverable` describes an item's intrinsic eligibility;
the package query also accounts for method ownership and required module
containers. Site generation applies package visibility separately and shares the
resulting page visibility across navigation, HTML and Markdown indexes, search,
sitemaps, and fragment descriptors. Existing snapshots acquire these defaults
when regenerated, without a schema change.

Authored `@docs`, `@autodocs`, and `@index` selections remain independent of
automatic discovery. An authored page may embed private documentation and remain
searchable without promoting those items into automatic API indexes.

## Families, methods, and identities

Functions have a family item and separate method items. A type serves as the
family for its constructor methods. Macro definitions remain directly
addressable bindings. The Julia language data records defining modules,
visibility, declaration kind, family membership, constructor membership, type
parameters, and supertypes. Signatures preserve annotations, defaults, keyword
arguments, varargs, return annotations, and `where` clauses.

Family identity uses the canonical qualified binding and declaration kind.
Method identity adds normalized positional dispatch. Matching authored methods
retain their source-derived dispatch expressions and stable IDs.
Positional-default forwarding methods join their verified authored declaration.
Constructor methods require a matching authored declaration or an exact native
method docstring. Compiler-supplied default constructors are omitted.

Runtime-only methods use the reflected constructor head, qualified type names,
positional annotations, varargs, and bound type variables. Dispatch keys exclude
argument names, keyword arguments, defaults, return annotations, and source
coordinates. A generated method's exact selector follows the exported dispatch
spelling, such as `generated(::Int64)`. Reflected signatures display the keyword
names Julia reports. Supplemental authored evidence provides defaults, keyword
annotations, and return annotations.

## Julia Markdown and references

Generic documentation belongs to the binding. An exact method attachment belongs
to the live method. Julia supplies registered string and Markdown documentation,
including interpolated text and `@doc` attachments. An unsupported documentation
object produces a diagnostic.

Prose, headings, emphasis, inline code, links, images, lists, block quotes,
tables, common admonitions, thematic breaks, and code blocks become portable
document nodes. Docstring headings begin below the API page title. Fatou
recognizes inline markup across wrapped lines and fences in list items and
admonitions. The adapter preserves exact decoded source spans and code
indentation, with a separate source segment for each contributing code token.
Source parsing retains code blocks without execution. The command pipeline
executes `jldoctest` fences and verifies their expected output. Ordinary Julia
fences remain illustrative. Unsupported executable directives fail extraction.
See the [extraction guide](../guide/extraction.md) for the supported native
subset. Math, footnotes, unsupported admonitions, and other Documenter
directives retain their source visibly with warnings. The source inspection API
retains opaque literals with a warning.

API references support explicit names, method selectors, and names inferred from
code labels:

```markdown
[`mean_squared_error`](@ref)
[vector method](@ref mean_squared_error(::AbstractVector, ::AbstractVector))
```

Names resolve in the module that registered the docstring, including maintained
aliases. Labels remain intact. A bare name resolves to the family. A selector
resolves to a retained method using the same dispatch normalization as
extraction. Unresolved API references fail workspace validation.

Document-node spans address decoded Markdown. When Fatou matches a decoded
literal to the registered text, it maps documentation and reference diagnostics
back to original bytes, including indentation, escapes, Unicode, and newline
normalization. Runtime-generated prose carries a verified repository-relative
registration file when one is available. Doctests need such a file for portable
execution evidence. The snapshot retains the raw Markdown and the resulting
documentation nodes.

## Integration and verification

The workspace records native discovery and Julia tool versions under the
existing `JuliaRuntime` provenance activity. Portable snapshots validate family
and constructor references. Snapshots produced with the earlier source and
runtime completion policy remain readable. Detached generation uses the retained
records and execution evidence.

Refresh replaces the target's API surface, including removed methods and
includes. Error-bearing extraction cannot replace a successful publication. The
[Julia tests](../../tests/julia_extraction.rs), [acceptance
fixture](../../tests/fixtures/acceptance/julia/), and [four-language
example](../../examples/monorepo/README.md) cover the pipeline.
