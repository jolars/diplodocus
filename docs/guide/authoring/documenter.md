# Julia Documenter pages

Use `format: documenter` for Julia Markdown `.md` pages that use Documenter
references and API assembly directives:

```yaml
content:
  - id: julia-manual
    owner: julia
    repository: core
    path: julia/docs/src
    mount: ""
    format: documenter
```

The owner names a configured package, or `project`. A package-owned collection
with an empty mount uses its authored `index.md` as the package overview.
Diplodocus parses Julia Markdown with Fatou and retains heading levels, inline
formatting, lists, admonitions, tables, code, and source ranges. Titles come
from page headings. This profile does not read `docs/make.jl`.

References use Documenter's link syntax. `` [`euler`](@ref) `` and
`` [fit a diagram](@ref `Eunoia.euler`) `` resolve canonical API items.
Legacy `[fit a diagram](@ref Eunoia.euler)` also works.

`[API Reference](@ref)` infers a heading title from the visible label.
`[gallery](@ref "A gallery of Euler and Venn diagrams")` supplies a title
explicitly. Heading references search the containing collection, including
other pages, and require a unique match.

`## [Installation](@id install)` assigns a heading anchor. Link to it with
`[setup](@ref install)`. Ordinary file and fragment links also work.

API names use the owning package before requiring a unique workspace match.
References can address private items and exported type fields, regardless of
which items an assembly directive selects. Plain labels select headings;
backticked labels select API items and fall back to headings when no API item
matches. Explicit quoted targets select heading titles. Legacy unquoted
references prefer a matching heading over an API item. See
[Documenter's syntax reference](https://documenter.juliadocs.org/stable/man/syntax/)
for the upstream forms.

An `@docs` block embeds selected Julia documentation in the order you list it:

````markdown
## Model Fitting

```@docs
SLOPE.slope
SLOPE.SlopeFit
SLOPE.predict(::SlopeFit)
```
````

Selectors accept binding names, maintained aliases, and exact static method
selectors. Bare names use the owning package before requiring a unique workspace
match. A callable name includes its supplied methods; a method selector includes
only that method. Types include their explicit constructors. Documented private
items and type fields may be selected explicitly. A module's self-binding, such
as `SLOPE.SLOPE`, selects the module. Missing, ambiguous, undocumented, and
unsupported selectors produce source-attributed errors. Selection never
evaluates Julia expressions.

An `@autodocs` block embeds documented Julia bindings from the named modules:

````markdown
```@autodocs
Modules = [Eunoia]
Private = false
```
````

`Modules` is required and accepts a literal vector of distinct, optionally
qualified module names. Each name must identify one extracted Julia module.
Submodules must be listed explicitly. `Private` and `Public` accept literal
booleans and both default to `true`. `Private = false` selects exported bindings
and bindings marked with Julia's `public` keyword; `Public = false` selects
private bindings. These filters affect embedded documentation only. Entries
follow module order, then modules, constants, types, functions, and macros, with
names sorted within each category. Callable entries retain their methods and
constructor documentation. Fields remain addressable through the canonical API
surface.

Add `Pages` to filter documentation by its Julia source file:

````markdown
```@autodocs
Modules = [SLOPE]
Pages = ["plots.jl", "utilities.jl"]
```
````

`Pages` accepts a literal vector of distinct, nonempty ordinary strings. Paths
use `/`; `.` and `..` components are normalized before matching. Each string
matches the end of a retained docstring's repository-relative source path, so
`plots.jl` also matches `src/more_plots.jl`. The first matching suffix
determines page order, followed by declaration category and name within each
module. Matching methods and constructors stay grouped under their binding,
ordered by the first matching page. Documentation from other files is excluded
even when it belongs to the same callable family. Omitting `Pages` or supplying
`[]` leaves documentation unfiltered. This option names Julia source files,
rather than authored Markdown pages.

An empty `@index` block lists entries assembled by either directive in the same
collection, sorted by qualified name, and links to their page anchors. It may
precede those blocks and includes entries assembled on other pages. An item or
its documentation may appear in only one selection per collection, including
overlap between a callable family and a selected method. Unsupported options,
dynamic values, duplicate options, and unknown modules produce source-attributed
diagnostics. The supported options are `Modules`, `Pages`, `Private`, and
`Public` for `@autodocs`; `@docs` and `@index` take no options.
`canonical=false`, `Order`, `Filter`, and `@meta CurrentModule` are outside this
authoring subset.

These directives control the contents of authored pages. They do not change
which API items Diplodocus extracts or advertises in generated navigation,
search, or fragment metadata.

Snapshots retain the assembled item references, original API documents, resolved
links, and local assets. Standalone HTML and HTML and Markdown fragments render
them without the checkout or Julia, including when the host maps their URLs.
Execution defaults to `never`, which displays `@example`, `@repl`, and
`jldoctest` fences as Julia code. To execute `@example` blocks, select a
prepared Julia environment explicitly:

```yaml
    execution:
      mode: execute
      engine: julia
      julia-project: julia/docs/Project.toml
```

The project file is relative to the collection's repository. Prepare its
dependencies before extraction; Diplodocus does not install them. Each page uses
a fresh Julia process, and examples run in source order. `@example gallery`
shares a module with other examples named `gallery` on that page. Unnamed
examples each get a separate module. The page's source directory is the working
directory. A package-owned collection verifies that its prepared environment
loads the declared Julia checkout.

Lines ending in a Julia `# hide` comment execute but disappear from the rendered
source. Hiding every line leaves only the output. The last expression becomes
the result, even with a trailing semicolon, and updates `ans` for later
expressions. When it returns `nothing`, captured stdout and stderr become
literal output instead. Explicit displays retain their order. Supported image
results are PNG, inert SVG, and JPEG, selected in that order before a
`text/plain` fallback. Renderers escape text and retain validated image bytes as
local assets.

This execution profile accepts an optional example session name. It rejects
example keyword options and executable `@meta`, `@setup`, `@repl`, and `@eval`
directives. `jldoctest` and ordinary Julia fences remain illustrative on
authored pages. Other unsupported directives retain their diagnostics. See
[native Documenter execution](../execution.md#native-documenter-execution) for
publication and environment evidence.

