# Julia extraction contract

The `julia` adapter reads an explicit `.jl` entry file and `Project.toml` with
Rust libraries. Fatou provides Julia syntax, documentation attachments, string
decoding, source maps, and Julia Markdown. Diplodocus determines the maintained
API, assigns identities, and resolves references. Extraction never starts
Julia, expands macros, loads a package, reads a depot, or installs dependencies.

## Configuration and inputs

```toml
[[package]]
id = "tinystats"
name = "Tiny Stats for Julia"
slug = "julia"
ecosystem = "julia"
repository = "repo"
path = "julia"
metadata-path = "Project.toml"
targets = [
  { id = "api", extractor = "julia", path = "src/TinyStats.jl", role = "public-api" },
]
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
files and package extensions are not discovered.

Metadata requires a nonempty `name` and a UUID. A supplied `version` must be
SemVer; omitting it leaves the package version absent. Authors, dependency
UUIDs, and compatibility strings are retained. Compatibility strings use
Julia/Pkg syntax and are not interpreted as Cargo constraints. Every original
top-level metadata field retains its TOML value and source range. Extraction
does not read `Manifest.toml` or resolve dependencies.

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
document nodes. Docstring headings begin below the API page title. Julia examples, REPL examples, `jldoctest`, `@example`, and
`@repl` fences remain inert code blocks. Extraction does not execute doctests.
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
[three-language example](../../examples/monorepo/README.md) exercise the adapter
and its build pipeline.
