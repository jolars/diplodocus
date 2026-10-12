# Python, R, and Julia API extraction

Diplodocus reads configured API targets statically and uses R to complete package
author and citation information. It does not run a build backend or install
dependencies. Targets must point to maintained files inside their declared
package and repository boundaries. The [workspace guide](configuration.md) shows
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
prose. Rd examples remain display-only.

Dataset topics marked `\docType{data}` create documented data objects without
requiring source bindings or exports. Their usage lists bare object names, and
their Format and Source sections retain structured prose, lists, and links.

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
and a code of conduct use `CONTRIBUTING.md` and `CODE_OF_CONDUCT.md`, preferring
package files over files at the repository root. Missing optional files do not
create empty pages. A missing file explicitly named by License is an error.

Extraction evaluates `Authors@R` and `inst/CITATION` with native R helpers. When
CITATION is absent, R generates a citation from DESCRIPTION. Author supplies a
fallback when Authors@R is absent. R also converts NEWS.Rd to readable text. These
operations use the checkout's files and supplied environment, and do not install
the documented package. Native errors and timeouts prevent publication. `check`
validates source syntax and links without evaluating this information.

An explicitly authored page at the package root or a supporting page's route
takes precedence. Files already included by authored collections are not
duplicated automatically. README links, heading anchors, and local images use
the normal reference and asset pipeline. Snapshots retain the metadata, native
results, documents, and assets so `generate` works without the checkout or R.
README.Rmd rendering remains outside this workflow.

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

Computed includes, conditional declarations, generated definitions, unsupported
macros, and unavailable reexports produce diagnostics. Extraction does not
expand macros, consult a Julia depot, infer runtime methods, or execute
doctests. Unsupported documentation syntax remains visible with warnings. The
[Julia extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/julia-extraction.md)
defines the supported subset and method identity rules.
