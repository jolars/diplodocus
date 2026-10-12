# Static R extraction

This page describes the current source adapter. The [prepared-environment
decision](../decisions/0003-prepared-environment.md) supersedes its
package-loading prohibition and inert-example contract; those changes are not
yet implemented.

`extractors::r::extract_target(repository, package, target)` reads a resolved R
package directory and returns a portable `RExtraction`. The API adapter never
installs, attaches, loads, or sources a package, or evaluates documentation. R must be
available on `PATH` for an isolated library-discovery probe.
Workspace assembly, reference resolution, and CLI commands consume this fragment;
their boundaries are described in the [implementation map](../design/execution.md).

The result contains validated `DESCRIPTION` metadata, canonical items with
independent signatures and documentation, ordered diagnostics, and extraction
provenance. Its `inputs` map retains the original text of every configured source file,
including unsupported or malformed input. Invalid UTF-8 retains its bytes
instead. Callers must reject error-bearing results before building a site;
independent valid inputs remain available for inspection.

Workspace assembly retains the complete DESCRIPTION metadata on its package
record and discovers conventional package documents. During extraction, a
separate native helper evaluates author and citation information and converts
NEWS.Rd. These completed results are distinct from the static API adapter's
observations. See the [package landing page workflow](../guide/extraction.md).

## Inputs and public declarations

The configured metadata path supplies `DESCRIPTION`. The target supplies
`NAMESPACE`, `R/`, and `man/`. Source discovery sorts paths and recursively
reads `.R` and `.Rd` files, ignoring hidden paths. Missing source and manual
directories are allowed. Reads stay within the package and target boundaries,
and directory symlinks are rejected. Portable output contains repository IDs
and relative paths, never checkout roots.

`DESCRIPTION` must contain one DCF record with nonempty Package, Version, and
Title fields. Metadata retains all fields, including unevaluated `Authors@R`,
and dependency declarations with their original version constraints. Duplicate
fields, malformed dependencies, unsupported encoding, and parser recovery
produce errors and exclude the metadata fragment. Folded values and exact
field, dependency, and constraint ranges retain separate evidence.

Supported namespace directives are unconditional `export`, `import`,
`importFrom`, two- or three-argument `S3method` declarations with static names,
and literal `useDynLib` declarations. For `useDynLib`, the library and any
named or positional symbols must have static names. `.registration` accepts
`TRUE` or `FALSE`, and `.fixes` accepts a string literal. Extraction retains the
namespace input without loading a native library or inferring registered symbols.

R extraction requires an installed R and the packages named in package-wide
imports. A single `R --vanilla` probe discovers `.libPaths()`, base bindings,
and R's known S3 generic coordinates. The probe sets `R_DEFAULT_PACKAGES=NULL`
and skips profiles and environment files, so it never loads project code or
attaches dependency packages. It respects library environment variables and
launcher-supplied paths, including Nix wrappers. Profiles that configure `renv`
or other project libraries are not executed; expose those libraries through
`R_LIBS` when needed.

Rust reads installed `Meta/nsInfo.rds` with `rd-rds` to resolve package-wide
imports, including `import(grid)`. Installed export patterns expand against
names in the package's `.rdx` index using supported regular expressions; dependency
code records and package hooks are never read or run. Missing packages, malformed
metadata, and unsupported patterns produce `r-library-unavailable` errors.
Explicit `importFrom` declarations and qualified registrations retain their
source-declared coordinates without loading the dependency.

S3 registrations resolve through local generics, explicit imports, and the
usual default R packages, in that order. Base takes precedence over the default
search path. Installed exports and R's known-generic map determine external
coordinates; ambiguous imports and uncertain local bindings produce errors.
There is no embedded table of generic names. Provenance records the R version
and fingerprints of the effective export sets and known-generic map, without
local library paths. Generation from a snapshot does not require R.

Conditional directives, computed names, `exportPattern`, and other directives
in the documented package's NAMESPACE produce errors. An invalid namespace
cannot establish a public surface.

Maintained source supplies direct, statically named function assignments and
assignment aliases. Signatures preserve formal order, required parameters,
defaults, `...`, and named parameters following `...`. Defaults become
structured expressions without evaluation. Non-syntactic names retain their
spelling. Source parsing errors exclude that file's definitions. Ambiguous,
conditional, computed, cyclic, or unsupported bindings cannot supply an
exported definition.

A direct terminal `UseMethod` call with a literal generic name establishes an
S3 generic. A terminal `structure` call with a literal class, or literal vector
of classes, establishes a constructor. The adapter checks for local, package,
and explicit import bindings that shadow these unqualified builtins. Qualified
`base::` calls also work. Nested closures do not establish their enclosing
function's dispatch. Other class systems, roxygen generation, and dynamic
construction remain outside this subset.

## Reconciliation and identity

Exports, S3 registrations, and valid Rd aliases select maintained functions.
Documented helpers do not require exports or S3 registration, but documentation
cannot invent a definition or disambiguate uncertain source bindings.
Dataset topics marked `\docType{data}` establish data objects directly from Rd,
without requiring maintained assignments or namespace exports. They carry
`RDeclaration::Dataset`, `ItemKind::Constant`, a value signature without an
inferred type or value, and documentation evidence. Their identity uses
`SemanticIdentity::r_dataset` and the topic's object name.
Assignment aliases use
the defining function's canonical identity and carry `r-assignment` evidence.
Rd aliases carry `rd-alias` evidence. Neither kind creates another item.

An S3 generic is its own callable family, with references to its separately
addressable methods. A method points to either a workspace generic or an
external package/name coordinate. External references do not invent an item
or package. Namespace registration evidence remains separate from source
definitions, and a registered method can be public without a name export.
Conflicting definitions, registrations, or topics produce errors instead of
choosing a winner. The [identity contract](item-identity.md) defines the keys.

## Rd documents and source attribution

Strict Rd views validate topic names, aliases, section shapes, and arguments.
Supported sections become headings, prose, usage blocks, argument entries,
lists, links, inline formatting, and display code. Value, details, references,
examples, notes, authors, see-also entries, keywords, and concepts retain their
content. Unqualified `\link` nodes become semantic references. Unsupported
markup, including `\Sexpr`, becomes a compact visible marker such as
`[Unsupported Rd: \Sexpr]` and produces an `unsupported-rd` warning. Unsupported
nodes have empty raw fragments because the parser provides no exact node ranges.
The enclosing document and extraction inputs retain the complete original Rd
source; rendering never repeats that source for each unsupported construct.

Each item on a shared topic receives the common prose and its matching usage
and arguments. Grouped argument labels are filtered to that item's formals.
Usage supports ordinary calls and typed `\method` or `\S3method` syntax;
maintained source remains authoritative for signatures. Conflicting usage,
unknown declarations, and arguments with no matching formal produce errors.
Additional help aliases can refer to a single unambiguous topic target.
Package topics with `\docType{package}` create a documented namespace item
without a maintained function or signature. Their aliases remain separate
from callable aliases, and conflicting package topics or aliases are errors.

Dataset usage supports bare object names, including backtick names. Calls,
assignments, and other expressions produce errors. The `\docType{data}` marker
sets the declaration kind, while `\format` and `\source` become headed sections
with structured content. Duplicate sections, conflicting topics, and collisions
with callable names or aliases produce errors. Dataset aliases, internal flags,
and explicit name exports retain their usual evidence and discovery behavior.
The acceptance fixtures use eulerr's `fruits`, `organisms`, `pain`, and `plants`
topics unchanged. They also supply a local topic for links to R data structures;
installed help links and custom sections remain separate compatibility work.

Callable topics marked `\keyword{internal}` set `RItemData.internal` independently
of `exported`. Their signatures, documentation, identities, and source evidence
remain in snapshots, and generation writes addressable pages. Public navigation
and search omit these items, including registered internal grid hooks. Package
topics remain discoverable even when they carry the conventional internal keyword.

Examples, including `\dontrun`, `\donttest`, and `\dontdiff` content, are
display-only `CodeBlock` nodes. They never become executable `CodeCell` nodes.
Unsupported example or usage forms retain a placeholder and the original
file. Malformed files remain in `inputs`, even when their documentation cannot
be attached.

`rd-source` provides exact native diagnostic ranges. Its successful semantic
nodes lack byte ranges, so each successfully parsed Rd file produces exactly
one `r-rd-source-attribution` warning. Documents and Rd aliases carry file-level
source locations with no span. Required document-node spans cover the entire
retained Rd input, and code blocks have no exact source segments. These coarse
ranges identify the enclosing file, not a proven fragment. Structural `RdPath`
values in diagnostics never substitute for byte offsets.

## Diagnostics and provenance

| Code | Meaning |
| --- | --- |
| `r-metadata` | Invalid DESCRIPTION grammar, fields, or dependency constraints |
| `r-source-read` | An input cannot be read or decoded |
| `r-library-unavailable` | R discovery or installed dependency metadata is unavailable |
| `r-syntax` | Native maintained-source parse failure |
| `r-unsupported-namespace` | Invalid, conditional, or unsupported namespace syntax |
| `r-unresolved-definition` | A public name, generic, or Rd usage lacks an unambiguous static declaration |
| `r-conflicting-surface` | Incompatible identities, registrations, topics, or signatures |
| `r-missing-documented-alias` | Warning for a public item without valid Rd documentation |
| `r-unsupported-surface` | Invalid or unsupported formal structure |
| `r-rd-syntax` | Native Rd diagnostic or hard input failure |
| `r-rd-information-loss` | Invalid semantic shape or documentation without matching formals |
| `r-rd-source-attribution` | Warning for file-level Rd attribution |
| `unsupported-rd` | Warning with a visible unsupported-markup placeholder |

Filesystem boundary failures use the shared source-path diagnostic codes.
Diagnostics sort in the common deterministic order and retain exact ranges
only when a parser supplies them.

Provenance records static mode, the seven implemented R capabilities, content
fingerprints, and actual parser use. `arity-parser` 0.6.0 handles DCF, namespace,
R source, and supported Rd usage calls. `rd-source` 0.4.0 handles Rd syntax, and
`rd-ast` 0.4.0 supplies strict semantic views with default features disabled.
Parser records are present only when used; undecodable input claims no parser
use. See the [extractor contract](../spikes/static-extractor-contract.md) for
the capability manifest.

The [integrated tests](../../tests/r_extraction.rs) lock the seven acceptance
items, four location warnings, shared documentation, alias identities, local
and imported S3 generics, and the extra dynamic-Rd warning. They compare
relocated workspaces, diagnose a missing R executable, and verify installed
dependency imports without package loading or profile execution. Component
cases cover malformed input, conflicts, source boundaries, structured markup,
and inert source and examples.
