# Static R extraction

`extractors::r::extract_target(repository, package, target)` reads a resolved R
package directory and returns a portable `RExtraction`. It never installs,
attaches, loads, or sources a package, starts R, or evaluates documentation.
CLI pipeline integration, workspace merging, and rendering remain later
milestones.

The result contains validated `DESCRIPTION` metadata, canonical items with
independent signatures and documentation, ordered diagnostics, and extraction
provenance. Its `inputs` map retains the original text of every consumed file,
including unsupported or malformed input. Invalid UTF-8 retains its bytes
instead. Callers must reject error-bearing results before building a site;
independent valid inputs remain available for inspection.

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
`importFrom`, and two- or three-argument `S3method` declarations with static
names. A package-wide `import` is retained in the original input, but does not
establish which names it imports. Resolving an external S3 generic therefore
requires `importFrom` or a qualified registration such as
`S3method(stats::predict, foo_model)`. Conditional directives, computed names,
`exportPattern`, and other directives produce errors. An invalid namespace
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

Exports and S3 registrations select the public surface. Assignment aliases use
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
markup, including `\Sexpr`, becomes a visible placeholder with retained raw
input and an `unsupported-rd` warning.

Each item on a shared topic receives the common prose and its matching usage
and arguments. Grouped argument labels are filtered to that item's formals.
Usage supports ordinary calls and typed `\method` or `\S3method` syntax;
maintained source remains authoritative for signatures. Conflicting usage,
unknown declarations, and arguments with no matching formal produce errors.
Additional help aliases can refer to a single unambiguous topic target.

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
relocated workspaces and run extraction with no runtime on `PATH`. Component
cases cover malformed input, conflicts, source boundaries, structured markup,
and inert source and examples.
