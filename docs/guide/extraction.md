# Python, R, and Julia API extraction

Diplodocus reads configured API targets statically and uses R to complete package
author and citation information and execute Rd examples. It does not run a build
backend or install dependencies. Targets must point to maintained files inside
their declared package and repository boundaries. The [workspace guide](configuration.md) shows
how to declare a target.

## Python

Python extraction reads `pyproject.toml` metadata and maintained `.py` and
`.pyi` files. It recognizes statically visible functions, classes, methods,
properties, constants, fields, type aliases, overloads, and supported
decorators. A statically resolvable `__all__` controls exports; otherwise,
public definitions and explicit aliases form the visible surface. Maintained
stubs supply public signatures, while implementation docstrings supply prose.
PEP 257 and NumPy-style docstrings provide structured documentation, and simple
`:func:` roles can become cross-references.

Overloaded callables have one reference page containing their public signatures
and implementation docstring. Navigation and search list the callable once.
Individual overload identities remain in snapshots, and references to them link
to the corresponding signature on the callable's page.

Python pages prefer public reexport names over private implementation paths.
For example, a function defined as `datamonger._api.fetch_data` and exported as
`datamonger.fetch_data` uses the latter name in its heading, navigation, and
search result. Both names remain searchable and resolve to the same page.
Snapshots retain the defining name, source location, and export evidence.

Computed exports, conditional definitions, opaque signatures, unsupported
decorators, and other dynamic behavior cannot be inferred safely. Diplodocus
reports the limit instead of executing package code or guessing an API. For a
public function decorated with `contextlib.contextmanager`, it preserves the
parameters and renders a generator's `Iterator[T]` or `Generator[T, ...]` return
annotation as `AbstractContextManager[T]`. An unsupported decorator on a public
declaration produces a diagnostic, and its callable signature is omitted because
the wrapper may change it. Unsupported decorators on internal declarations do
not prevent extraction of the public surface unless those declarations are
reexported.

## R

R extraction reads `DESCRIPTION`, `NAMESPACE`, maintained `R/` source, and
checked-in `man/` Rd files. Supported static namespace declarations include
`export`, `import`, `importFrom`, `S3method`, and literal `useDynLib`
declarations. It connects exports and S3 registrations to direct source
definitions, then attaches supported Rd topics, aliases, usage, arguments, and
prose. `extract`, `build`, and `serve` execute Rd examples as a required step.
`check` reads their source without loading the package or executing examples.
R Markdown vignettes are also discovered and executed; see
[authored articles](authoring.md).

Prepare a working checkout with R, `pkgload`, `evaluate`, the package dependencies,
and the dependencies used by its examples. Compile native extensions beforehand;
Diplodocus never compiles or installs the package. Each help topic runs once in a
fresh R process, including topics shared by several declarations. Expressions
within a topic share state and a graphics device. The package loads from the
configured checkout, and extraction verifies its namespace path and version.

`\dontrun` regions remain explicitly illustrative. `\donttest` regions execute.
False `if` conditions without an `else`, including `interactive()` and platform
checks, record a skip reason. Conditions run once. Missing dependencies requested
through `require()` or `requireNamespace()` cause an error rather than a skip.
Normal R error handling such as `tryCatch()` remains available to examples.

Results, messages, and warnings render as literal text, and recorded plots become
validated PNG assets. Snapshots retain executed expressions, illustrative regions,
skip reasons, the Rd source fingerprint, R and package versions, and loaded
namespace versions. Each extraction executes examples afresh; the QMD cache does
not cover Rd examples. Generation uses the completed snapshot without R or the
checkout. Errors, missing prerequisites, timeouts, invalid figures, and cleanup
failures prevent publication and preserve the previous snapshot and site.

Shared help topics retain all documented usages and arguments on each
declaration's page. Topic aliases resolve to a primary declaration, while
method aliases resolve to their own pages. Links between declarations and
methods grouped by S3 class make the complete topic navigable.

R reference pages use the Rd title as their heading and show the description
before usage. URLs use readable API names, or the topic name for a shared
topic's primary declaration. The reference overview lists each topic with its
title and introductory summary. Code blocks have syntax highlighting and copy
buttons. Usage appears once, including the complete shared topic's signatures.
Source links point to the recorded R and Rd files when the repository has a
revision and a GitHub URL or a supported `source-link-template`. These links
remain available when generating from a snapshot without the checkout.

Rd content supports labeled local and package-qualified help links, equations,
aligned tables, custom sections, named return entries, HTML and platform
conditionals, and DOI links. Equations render natively with `katex-rs` and bundled
fonts, so they work with JavaScript disabled. External help links use installed
help aliases to select rdrr.io destinations; snapshots retain the results so
generation needs neither R nor installed help databases.

Dataset topics marked `\docType{data}` create documented data objects without
requiring source bindings or exports. Their usage lists bare object names, and
their Format and Source sections retain structured prose, lists, and links.

Documented constants retain literal values and assignment aliases from maintained
source bindings. Their usage lists bare object names. Standalone help topics
named `*_deprecated` retain prose and aliases without requiring a function or
inventing a callable signature. Nested Rd lists and prose between described
entries retain their content and order.

Standard S3 generics from default R packages resolve without explicit imports.
R must be available on `PATH`. Package-wide imports use the installed
dependencies' metadata without loading their namespaces. Documented helpers
retain their maintained signatures without requiring exports. Topics marked
`\keyword{internal}` have addressable pages but stay out of navigation and search.
See the R extraction contract below for discovery and import limits.

### Package landing pages

Diplodocus automatically reads `README.md` from the declared R package root and
uses it as the package overview. Without a README, DESCRIPTION supplies the
introduction. The page shows package metadata and links to available release
notes, authors and citation information, licenses, and community guidelines.

Release notes use `NEWS.md`, then `NEWS.Rd`, then plain `NEWS`. Licenses include
the DESCRIPTION declaration, its local `file` references, and conventional
`LICENSE`, `LICENSE.md`, `LICENCE`, and `LICENCE.md` files. Contribution guidelines
and a code of conduct use `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and `CONDUCT.md`, preferring
package files over files at the repository root. Missing optional files do not
create empty pages. A missing file explicitly named by License is an error.

Extraction evaluates `Authors@R` and `inst/CITATION` with native R helpers. When
CITATION is absent, R generates a citation from DESCRIPTION. Author supplies a
fallback when Authors@R is absent. R also converts NEWS.Rd to readable text. These
operations use the checkout's files and supplied environment, and do not install
the documented package. Native errors and timeouts prevent publication. `check`
validates source syntax and links without evaluating this information.

An explicitly authored page at the package root or a supporting page's route
takes precedence. Diplodocus resolves these overrides before parsing the
automatically discovered documents, so replaced content does not produce syntax
diagnostics. Links to the original files resolve to the authored replacement.
Files explicitly included by authored collections still receive diagnostics and
are not duplicated automatically. README links, heading anchors, and local
images use the normal reference and asset pipeline. Snapshots retain the
metadata, native results, documents, and assets so `generate` works without the
checkout or R.
README.Rmd rendering remains outside this workflow.

Automatically discovered package pages can also link to existing repository
files outside the declared content, such as `spec/README.md`, `TRUST.md`, and
`OPERATIONS.md` in a root contributor guide. Declared pages and their source
aliases resolve to site pages first. Other local documents and directories use
a repository URL, as do files outside package and content asset boundaries.
This fallback requires a [repository URL or source-link template](configuration.md)
and a declared or observed revision. Missing targets, invalid site anchors,
and paths that leave the source repository remain errors, including symlinks
that leave and reenter it. Diplodocus checks the target's existence and type
without parsing it, reading its contents, or including it in the site.
Fragments on repository links pass to the host without local anchor validation.
Images still require local asset validation. Explicitly authored pages,
including package overrides and R Markdown articles, keep the usual link rules.

Computed names, conditional namespace directives, dynamic construction, roxygen
generation, and unsupported Rd markup are not evaluated. Unsupported markup
stays visible with a diagnostic. For exact parser and identity rules, see the
[Python extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/python-extraction.md)
or [R extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/r-extraction.md)
in the source repository.

## Julia

Julia extraction reads `Project.toml`, an explicit `.jl` entry file, and its
unconditional literal includes. Declare `ecosystem = "julia"`,
`metadata-path = "Project.toml"`, and a target with `extractor = "julia"` and
`path = "src/MyPackage.jl"`. Source directories are not valid Julia targets.

Exported, `public`, and documented declarations form the maintained surface.
Functions have a family page and separate pages for their supplied methods.
Types retain fields and explicit constructors. Static imports and aliases
resolve to maintained declarations. Julia Markdown docstrings supply prose,
lists, tables, admonitions, and inert code examples. API `@ref` links can name a
family or select a method, and they preserve their labels.

During extraction, unresolved public bindings trigger Julia type reflection in
the configured package project. Prepare Julia and the package dependencies
before building. The helper verifies the supplied checkout and completes
exported types generated by macros, retaining runtime provenance separately from
source facts. `check` stays static and reports unresolved generated declarations.

Computed includes, conditional declarations, unsupported macros, unresolved
functions, and unavailable reexports still produce diagnostics. Extraction does
not infer runtime methods or execute doctests. Unsupported documentation syntax
remains visible with warnings. The
[Julia extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/julia-extraction.md)
defines the supported subset and method identity rules.
