# Julia extraction contract

This page describes the source adapter and its runtime type completion. The
[prepared-environment decision](../decisions/0003-prepared-environment.md)
also requires Julia example execution, implemented by the native doctest adapter
in the extraction command pipeline.

The `julia` adapter reads an explicit `.jl` entry file and `Project.toml` with
Rust libraries. Fatou provides Julia syntax, documentation attachments, string
decoding, source maps, and Julia Markdown. Diplodocus determines the maintained
API, assigns identities, and resolves references. `check` uses this static
adapter without starting Julia. During `extract`, `build`, and `serve`, every
checkout is loaded and validated, even without doctests or unresolved public
bindings. Unresolved public bindings additionally trigger native type completion.

## Runtime type completion

Diplodocus starts Julia with startup and history files disabled and activates
the configured package project. The caller must provide Julia and the package's
dependencies. Diplodocus never installs or resolves dependencies. The helper
loads the package by its Project.toml name and UUID and verifies the loaded
module name, UUID, version, and entry file against the supplied checkout.
Loading another release, missing prerequisites, errors, cancellation, and
timeouts prevent snapshot publication.

The helper observes exported types defined by the root module, their type
parameters, supertypes, mutability, and fields. This completes macro-generated
exception types such as Datamonger's 15 `@define_error` exports. It does not
synthesize constructor methods or source locations. Existing source declarations
retain their source evidence; incompatible observed type flavors produce errors.
Unresolved functions, external types, dynamic includes, and unsupported source
syntax remain errors.

Runtime-derived types and fields carry `julia-runtime` provenance with the helper
policy, Julia version, verified package identity and version, and loaded
dependency UUIDs and versions. These records contain no local paths or invented
source ranges. Static parser extraction evidence remains separate. Completed
snapshots can be generated without the checkout or Julia. `check` continues to
diagnose unresolved generated declarations and does not prove that runtime
extraction will succeed.

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

The target must be an explicit entry file, rather than a source directory.
Unconditional `include("relative.jl")` calls extend that target. Paths are
relative to the including file, and both lexical paths and symlink destinations
must remain inside the package and repository. Includes retain their enclosing
module. A file included in two modules contributes declarations to both, while
its bytes and parser input are recorded once. Missing files, cycles, computed
paths, include transforms, qualified includes, and conditional includes produce
errors. A locally declared or imported `include` binding also produces an error
because its semantics cannot be established from a call alone. Unreferenced
files and package extensions are not discovered automatically.

Metadata requires a nonempty `name` and a UUID. A supplied `version` must be
SemVer; omitting it leaves the package version absent. Authors, dependency
UUIDs, and compatibility strings are retained. Compatibility strings use
Julia/Pkg syntax and are not interpreted as Cargo constraints. Every original
top-level metadata field retains its TOML value and source range. Extraction
does not read `Manifest.toml` or resolve dependencies.

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

Each entry must name a unique `.jl` file relative to the package root. Missing
files, directories, and paths or symlinks outside the package fail validation.
Both `ext/NameExt.jl` and `ext/NameExt/NameExt.jl` layouts work when supplied
explicitly. The adapter does not select files from `[extensions]`, inspect a
depot, or load weak dependencies.

Extension files start in their own top-level scope and follow the same static
include and declaration rules as the core entry. Diplodocus assembles all
supplied declarations together. For example, `import Eunoia: eunoiaplot` in the
extension binds its methods to the existing `Eunoia.eunoiaplot` family. The
family retains its generic documentation and core fallback methods, while its
signatures also include the supplied extension implementations. Method
documentation stays with its method. Duplicate dispatch identities still
produce conflicts rather than simulating method replacement.

Only authored methods from the selected files and their literal includes are
retained. Imports from unavailable dependencies do not invent their modules,
types, exports, or other methods. Macro-generated recipes remain subject to
the existing unsupported-syntax policy. Selecting an extension describes a
static documentation surface; it does not claim that Julia activated it.
Runtime checkout validation continues to load the core package and requires
its prepared dependencies, without explicitly activating selected extensions.

Extension sources and includes retain repository-relative locations, original
input bytes, and extraction fingerprints. Their methods share the core
family's identities, discovery policy, and rendered page, including after
detached snapshot generation. Removing an extension from the selection removes
its methods on the next extraction without removing the core generic.

## Maintained surface

The adapter retains exported and `public` bindings, documented declarations,
and the containers and members needed to describe them. A visible callable
retains all supplied methods. A visible type retains its fields and explicit
constructors. Private, undocumented helpers are omitted.

Supported declarations include modules and bare modules, long and short
functions, empty generic declarations, structs and mutable structs, abstract
and primitive types, fields, constants, macros, and explicit constructors.
`@inline`, `@noinline`, and `@propagate_inbounds`, including their `Base`
qualifications, preserve an underlying declaration and its wrapper provenance.
Other declaration-generating macros and definitions inside conditional or
runtime scopes produce errors. Function bodies and constant values are retained
as syntax where needed; they are never evaluated.

Supplied imports, local `using` exports, import aliases, and assignment aliases
resolve to maintained canonical declarations. Constant aliases of supplied
functions, types, or modules also retain the canonical identity. Reexporting a
binding whose definition is unavailable produces an error. An explicitly
supplied extension such as `Base.show` has a local callable family with an
external owner; extraction does not invent the dependency's other methods or
module declaration. Julia's implicit `Base` exports are not discovered.

## Automatic API discovery

Retention and discovery serve different purposes. A docstring retains a private
declaration for direct references and authored documentation; it does not make
that declaration part of automatic API navigation, search results, or indexes.
Exported bindings and bindings marked with `public` are discoverable, including
undocumented bindings. An exported or public alias makes its canonical item
discoverable. Aliases keep the canonical identity, display name, and route;
search includes the canonical name and retained alias spellings without
creating duplicate results.

Methods inherit discovery from their callable family, and explicit constructors
inherit it from their type. They remain grouped on the family or type page,
with searchable method prose and stable method anchors. An extension of an
external generic is private unless a supplied export or `public` declaration
exposes its binding or an alias. Mentioning a public type in a method signature
does not make the external family discoverable.

The package root and retained enclosing modules of discoverable items remain
discoverable as containers. Explicitly exported or public modules are also
discoverable, but their private children are not exposed automatically. Fields
never receive automatic index entries, including fields of public types.
Fields appear in declaration order in a Fields section on their owning type's
page, with annotations, retained documentation, and stable `field-<name>` anchors.
HTML and Markdown use the same grouping. Field references
and host URL maps target the type page and field anchor; separate field pages
are not generated. The type's fragment descriptor retains field identities and
headings, and its search entry includes field names and documentation.

Retained private items keep their HTML and Markdown content, semantic identities,
routes, and direct references. Fragment descriptors include private pages with
`visible: false`, including private types with their fields.

`Package::discoverable_items` derives this policy from retained declaration and
ownership facts. `Item::discoverable` describes an item's intrinsic eligibility;
the package query also accounts for method ownership and required module
containers. Site generation applies package visibility separately and shares
the resulting page visibility across navigation, HTML and Markdown indexes,
search, sitemaps, and fragment descriptors. Existing snapshots acquire these
defaults when regenerated, without a schema change.

Authored `@docs`, `@autodocs`, and `@index` selections remain independent of
automatic discovery. An authored page may embed private documentation and
remain searchable without promoting those items into automatic API indexes.

## Families, methods, and identities

Functions have a family item and separate method items. A type serves as the
family for its constructor methods. Macro definitions remain directly
addressable bindings. The Julia language data records defining modules,
visibility, declaration kind, family membership, constructor membership, type
parameters, and supertypes. Signatures preserve annotations, defaults, keyword
arguments, varargs, return annotations, and `where` clauses.

Family identity uses the canonical qualified binding and declaration kind.
Method identity adds a structured dispatch expression: the constructor head,
positional annotations, varargs, and `where` constraints. Bound type variables
are normalized by binding order. Argument names, keyword arguments, defaults,
return annotations, file paths, and declaration order do not change identity.
This defines the adapter's static identity subset; it does not attempt Julia's
full semantic equivalence of types or constraints. Two declarations with the
same dispatch identity produce a conflict diagnostic instead of simulating
runtime method replacement.

Optional arguments and runtime-generated constructors do not create synthetic
method items. Callable objects, generated methods, arbitrary macro expansion,
and runtime reflection are outside this contract.

## Julia Markdown and references

Static ordinary, triple-quoted, and raw docstrings become Julia Markdown
through Fatou. Generic documentation belongs to the family; a docstring on a
method belongs to that method. Supported `@doc` attachments can refer to an
existing family or method. Conflicting attachments produce errors.

Prose, headings, emphasis, inline code, links, images, lists, block quotes,
tables, common admonitions, thematic breaks, and code blocks become portable
document nodes. Docstring headings begin below the API page title. Fatou
recognizes inline markup across wrapped lines and fences in list items and
admonitions. The adapter preserves exact decoded source spans and code
indentation, with a separate source segment for each contributing code token.
Source parsing retains code blocks without execution. The command pipeline
executes `jldoctest` fences with expected-output verification; ordinary Julia
fences remain illustrative.
Unsupported executable directives fail extraction. See the
[extraction guide](../guide/extraction.md) for the supported native subset.
Math, footnotes, interpolation, unsupported admonitions, and other Documenter
directives retain their source visibly with warnings. A docstring that requires
evaluation remains an opaque document with a warning.

API references support explicit names, method selectors, and names inferred
from code labels:

```markdown
[`mean_squared_error`](@ref)
[vector method](@ref mean_squared_error(::AbstractVector, ::AbstractVector))
```

Names resolve in the defining module, including maintained aliases. Labels
remain intact. A bare name resolves to the family; a selector resolves to a
supplied method using the same dispatch normalization as extraction. Unresolved
API references fail workspace validation. Documenter heading references and
page assembly are outside this API-reference subset.

Document-node spans address decoded Markdown. Fatou maps documentation
provenance, extraction diagnostics, and unresolved reference diagnostics back to the original Julia literal,
including indentation removal, escapes, Unicode, and newline normalization.
Raw input and repository-relative source locations remain in the extraction
result. No absolute checkout paths or runtime state enter the snapshot.
Decoded documentation provenance records pair each node's decoded range with
its original source range, preserving this attribution in portable snapshots.

## Integration and verification

`extract_target` returns `JuliaExtraction`, including metadata, items, retained
inputs, diagnostics, and the static extraction observation. Workspace assembly
uses this result, and portable snapshots validate Julia family and constructor
references. Generated sites provide family and method pages, Julia signatures,
navigation, search, and shared concepts without the original sources or Julia.

Error-bearing extraction remains inspectable but cannot be published. Refresh
replaces the selected package's extracted surface rather than retaining deleted
methods or includes. Existing IR, SQLite, and encoding versions remain unchanged
because the Julia records extend the existing language data, signatures, and
documentation provenance.

The [static extractor contract](../spikes/static-extractor-contract.md) records
parser pins and capabilities. The [Julia tests](../../tests/julia_extraction.rs),
[acceptance fixture](../../tests/fixtures/acceptance/julia/), and
[four-language example](../../examples/monorepo/README.md) exercise the adapter
and its build pipeline.
