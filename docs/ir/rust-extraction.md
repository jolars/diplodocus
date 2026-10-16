# Rust extraction contract

The `rust` adapter translates native rustdoc JSON into shared API items,
structured signatures, Markdown documents, aliases, and implementation
relationships. Cargo and rustdoc run against the supplied checkout. The adapter
does not reconstruct Rust visibility, macros, or conditional compilation from
source text.

## Native boundary

Rust uses `nightly-2026-09-28` and `rustdoc-types` 0.61.0, matching rustdoc JSON
format 61. Doctest discovery uses format 2. The separately prepared toolchain
root comes from `DIPLODOCUS_RUST_TOOLCHAIN`; child Cargo, rustc, and rustdoc come
from that same root. Diplodocus's development compiler remains stable.

Cargo metadata establishes package identity, workspace-inherited versions,
library entry points, dependency versions, and features. The configured target
must name that package's library source file. All invocations use `--offline
--locked`, explicit package selection, and the host target. Feature defaults and
additional features come from the target's optional `rust` settings.

Rust targets permit local compilation and build scripts under the prepared
environment decision. Compilation artifacts live in generated cache storage.
Native preparation has a ten-minute deadline. Cancellation and timeout terminate
the supervised process group, and incomplete cleanup prevents publication.

The adapter inventories maintained local package inputs, workspace and package
manifests, Cargo.lock, repository Cargo configuration, and toolchain files.
A separate rustdoc dependency-info invocation adds compiler-tracked module and
include inputs. Dependency-info emission suppresses API JSON on this nightly,
so the JSON invocation runs separately. Inputs and inventory membership are
checked again after native operations. This includes private and doctest-only
sources. Arbitrary undeclared files read by build scripts are outside this
source observation contract.

## Portable API semantics

Public reachability is computed before canonical items are created. This retains
public reexports of declarations in private modules. Each declaration has one
semantic identity based on its defining path, declaration role, and any owning
implementation context. Temporary rustdoc IDs and source offsets never define
portable item identity. Public aliases remain lookup names; the shortest public
path is preferred for presentation, with lexical tie-breaking.

`ItemLanguageData::Rust` retains declaration kinds, public paths, owners,
attributes, and implementation relationships. Rust refines the shared item
categories for structs, enums, traits, variants, methods, fields, associated
types, aliases, constants, statics, and macros. Signatures use structured
language-specific nodes with semantic children for types and declarations.
Source evidence remains separate from signature structure.

Fields, variants, and methods stay independently addressable in SQLite while
rendering on their owning type or trait page. Trait implementation context
distinguishes methods with the same name. Documented implementation blocks have
embedded item records so their prose, links, and anchors use the same resolved
document pipeline as ordinary items. Synthetic and blanket implementations
retain their native classifications without importing dependency APIs as
additional local reference pages.

Documentation uses the `rustdoc-markdown` source format. The native link map
resolves local shortcut links and paths such as `Self` and `crate` to canonical
item references. External crate references use external documentation links.
Original definition spans are retained only when the native coordinates can be
proved against maintained UTF-8 source. Compiler-generated documentation never
acquires invented source ranges.

## Native examples and publication

Discovery runs through `cargo test --doc` with rustdoc's structured doctest
output. This preserves dev-dependency context and native `cfg(doc)` and
`cfg(doctest)` behavior. A separate native execution uses JSON test events with
merging disabled and one test thread. Each discovered example must receive one
matching terminal outcome. The existing cell deadline applies to an active
example.

Native setup lines, editions, panic expectations, compile-failure expectations,
compile-only examples, and explicit ignores retain rustdoc's meaning.
`RustDoctests` provenance belongs to the extraction target because examples on
private or doctest-only declarations may have no public API item. Outcomes
record code, attributes, source attribution when available, native output, tool
versions, features, dependencies, and input fingerprints. Successful stdout is
not guaranteed by rustdoc and is not invented by the adapter.

`check` resolves the compiler API and links without executing doctests.
Extraction requires completed native examples, including an explicit completed
record when no examples exist. Snapshot validation checks Rust ownership,
implementation membership, semantic references, and execution evidence before
publication. IR version 6 carries these additions; storage version 2 and canonical
encoding version 1 remain unchanged. Generation reads completed snapshots without
Cargo, rustdoc, dependencies, or the checkout.
