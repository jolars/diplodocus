# R extraction

`check`, `extract`, `build`, `serve`, and the Rust library's
`assembly::assemble_workspace()` share package-level documentation selection,
optional roxygen2 generation, and verified checkout loading. The library
assembly function keeps documentation examples inert, as `check` does. The
[R workflow](../guide/extraction/r.md) describes configuration and checkout
writes.

## Native command discovery

The loaded checkout supplies exports, function formals, and S3 registration
evidence. Documented generated functions no longer need a direct static source
assignment. Rd topics still supply prose, aliases, shared usage, and argument
documentation. Source parsers retain proven locations and supported supplemental
facts; runtime observations do not invent source declarations.

Completed generation records `RDocumentation` with the effective loader,
roclets, R and roxygen2 versions, dependency versions, and portable input/output
fingerprints. Native discovery records `RRuntime` with the verified package
version, R and dependency versions, and observed inputs. These package-level
records accompany target extraction in `native` mode. They contain no checkout
paths, process IDs, timestamps, or execution claims. Maintained Rd mode omits
generation evidence. IR version 16 introduced these records.

Generation errors, changed maintained inputs, invalid helper results, timeout,
cancellation, or failed cleanup reject the attempt. The adapter never accepts
preexisting generated Rd after generation fails. The incremental cache verifies
inputs and environment observations before reusing generation or discovery.

The package's complete `R/`, `man/`, and `data/` trees are native input
locations, regardless of filename extension. Normal commands reject recognized
Diplodocus storage there, and CLI publication rejects snapshots and generated
sites in those trees. `DESCRIPTION`, `NAMESPACE`, and `.Rbuildignore` are also
protected native inputs. Store outputs in reserved `.diplodocus/` paths or
outside those locations so language tools consume their normal package inputs.

Workspace assembly retains the complete DESCRIPTION metadata on its package
record and discovers conventional package documents. During extraction, a
separate native helper evaluates author and citation information and converts
NEWS.Rd. See the [package landing page workflow](../guide/extraction.md).

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

Native R interprets the checkout's NAMESPACE and loads required packages and
prepared native libraries. Diplodocus reads unconditional `export`, `import`,
`importFrom`, and `S3method` entries with literal names for source attribution
and supplemental import evidence. Export patterns and conditional directives
can contribute to the observed native API.

A separate `R --vanilla` probe discovers library paths, base bindings, and known
S3 generic coordinates. The probe sets `R_DEFAULT_PACKAGES=NULL`. Diplodocus
also reads installed `Meta/nsInfo.rds` with `rd-rds` for supplemental import and
help evidence. Expose prepared libraries through `R_LIBS`, `R_LIBS_USER`, or
the R launcher on `PATH`.

Maintained source supplies direct, statically named function assignments and
assignment aliases. Signatures preserve formal order, required parameters,
defaults, `...`, and named parameters following `...`. Defaults become
structured expressions without evaluation. Non-syntactic names retain their
spelling. Source parsing errors produce supplemental warnings. Generated
bindings can use native discovery evidence when a direct source definition is
unavailable.

A direct terminal `UseMethod` call with a literal generic name establishes an
S3 generic. A terminal `structure` call with a literal class, or literal vector
of classes, establishes a constructor. The adapter checks for local, package,
and explicit import bindings that shadow these unqualified builtins. Qualified
`base::` calls also work. Nested closures do not establish their enclosing
function's dispatch. Other class systems and dynamic construction remain outside
this supplemental subset; native discovery handles supported generated bindings.

## Reconciliation and identity

Native exports, S3 registrations, and valid Rd aliases select observed
functions. Rd can document a private binding without exporting it.
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
content. Rd links retain their labels independently of their targets. `\link{topic}`,
`\link[=topic]{label}`, `\link[pkg]{topic}`, and `\link[pkg:topic]{label}` become
links with `@ref` targets. Links nested inside `\code` retain code styling.
Resolution prefers the owning package and then a unique R workspace match;
package qualifiers accept published DESCRIPTION names and workspace IDs.
A qualifier naming a workspace package never falls back to an installed release.
Otherwise, installed help aliases from declared dependencies and standard R
packages supply rdrr.io destinations. Missing and ambiguous topics produce
errors. Help-index fingerprints and candidate destinations travel with snapshots,
so generation needs neither R nor installed help databases.

`\eqn` and `\deqn` retain verbatim LaTeX and optional text alternatives. The
native `katex-rs` renderer emits HTML and MathML with bundled CSS and fonts;
math works with JavaScript disabled. Unsupported LaTeX falls back to the text
alternative or LaTeX source. `\tabular` retains column alignment and all rows
without inferring headers. Custom `\section` and `\subsection` content becomes
headings, and named `\value` entries retain their labels and descriptions.
`\doi` becomes an HTTPS link to doi.org.

Format conditionals select HTML content, including comma-separated formats,
`TRUE`, and `FALSE`. Platform `#ifdef` and `#ifndef` blocks select `unix` or
`windows` using R's platform observation. Selection precedes semantic section
validation, and unselected branches contribute no references or unsupported-node
warnings. The selected content and platform evidence remain in the snapshot.

Unsupported markup, including `\Sexpr`, becomes a compact visible marker such as
`[Unsupported Rd: \Sexpr]` and produces an `unsupported-rd` warning. Unsupported
nodes have empty raw fragments because the parser provides no exact node ranges.
The enclosing document and extraction inputs retain the complete original Rd
source; rendering never repeats that source for each unsupported construct.

Each item on a shared topic receives the complete prose, usage, and argument
listings. Grouped argument labels remain intact, including names used only by
another method. In argument labels, `\dots` and `\ldots` normalize to the
maintained `...` formal. Shared topic coordinates retain the topic name,
primary declaration, and all members independently of their semantic identities.
Usage supports ordinary calls and typed `\method` or `\S3method` syntax. Native
formals establish signatures. Diplodocus preserves maintained spellings when it
can verify them against those formals. Conflicting usage, unknown declarations,
and arguments with no matching formal produce errors.
Additional help aliases name the topic's primary declaration. Selection prefers
a declaration matching the topic name, then a local generic, then the first
documented usage, and finally the first canonical identity. Declaration aliases
still resolve to their own items. Aliases claimed by different topics remain
errors. Reference pages link shared declarations and group S3 methods by class,
including registrations whose generic lives in an external R package.
Package topics with `\docType{package}` create a documented namespace item
without a maintained function or signature. Their aliases remain separate
from callable aliases, and conflicting package topics or aliases are errors.

Standalone topics named `*-deprecated` or `*_deprecated` without `\usage`
retain argument documentation without requiring callable formals. Their return
values, prose, and help links use the same lowering and resolution rules as other
Rd topics, and they have no function signature. If the topic attaches to a
maintained declaration, its arguments must still match that declaration's formals.

Dataset usage supports bare object names, including backtick names. Calls,
assignments, and other expressions produce errors. The `\docType{data}` marker
sets the declaration kind, while `\format` and `\source` become headed sections
with structured content. Duplicate sections, conflicting topics, and collisions
with callable names or aliases produce errors. Dataset aliases, internal flags,
and explicit name exports retain their usual evidence and discovery behavior.
The acceptance fixtures use eulerr's `fruits`, `organisms`, `pain`, and `plants`
topics unchanged. They also supply a local topic for links to R data structures;
installed help links and custom sections use the same Rd lowering and resolution
rules as callable topics.

Callable topics marked `\keyword{internal}` set `RItemData.internal` independently
of `exported`. Their signatures, documentation, identities, and source evidence
remain in snapshots, and generation writes addressable pages. Public navigation
and search omit these items, including registered internal grid hooks. Package
topics remain discoverable even when they carry the conventional internal keyword.

Examples, including `\dontrun`, `\donttest`, and `\dontdiff` content, are
display-only `CodeBlock` nodes. They never become executable `CodeCell` nodes.
Unsupported example or usage forms retain a placeholder and the original
file in the enclosing document when lowering succeeds.

`rd-source` provides exact native diagnostic ranges and a source map for its
parsed AST. The adapter selects conditional branches and normalizes tables
without carrying that map through the transformations, so each successfully
parsed Rd file produces one `r-rd-source-attribution` warning. Documents and Rd
aliases carry file-level source locations with no span. Required document-node
spans cover the entire retained Rd input, and code blocks have no exact source
segments. These coarse ranges identify the enclosing file, not a proven
fragment. Structural `RdAstPath` values in diagnostics never substitute for byte
offsets.

## Diagnostics and provenance

| Code | Meaning |
| --- | --- |
| `r-metadata` | Invalid DESCRIPTION grammar, fields, or dependency constraints |
| `r-source-read` | An input cannot be read or decoded |
| `r-library-unavailable` | The supplemental R library probe or installed dependency metadata is unavailable |
| `r-syntax` | Native maintained-source parse failure |
| `r-unresolved-definition` | An Rd topic or usage lacks a matching declaration |
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

Extraction provenance records native discovery, source fingerprints, and the
parsers used to read each input. The build uses `arity-parser` for DCF, NAMESPACE,
R source, and supported Rd usage. `rd-source` provides Rd syntax and `rd-ast`
supplies strict semantic views.

The [R integration tests](../../tests/r_extraction.rs) cover native identities,
signatures, source evidence, shared topics, and malformed inputs.
[The roxygen2 tests](../../tests/r_roxygen.rs) verify package loading and
generation through both library assembly and CLI commands.
[The Rd example tests](../../tests/rd_examples.rs) exercise extraction-time
execution, shared sessions, figures, failure preservation, and detached
generation.

## Native Rd example execution

`WorkspaceSources::execute_rd_examples` runs after native metadata completion
and before QMD execution and snapshot publication. It uses `pkgload::load_all`
with compilation disabled, public exports only, and test helpers disabled.
The loaded namespace must belong to the supplied checkout and match DESCRIPTION.
Every requested DLL must already exist and load from that checkout.

A fresh R process handles each distinct documented Rd source. `tools::Rd2ex`
comments out `dontrun` and retains executable `donttest`. `evaluate` runs the
converted expressions in one session with one graphics device and captures
ordered output and recorded plots. Explicit false branches record their predicates
and reasons; failed dependency probes are fatal. Unsupported executable Rd syntax
is an extraction error even when source assembly retains a visible placeholder.

Output text becomes escaped display blocks. Recorded plots are replayed to PNG,
validated with the existing image policy, and copied into the snapshot's content
asset store. File-level Rd attribution covers source, outputs, and skip evidence;
the adapter does not invent node ranges. `RdExamples` provenance records the input
fingerprint, runtime and namespace versions, executed expressions, illustrative
regions, and skips. This adapter does not reuse the authored-page cache.
