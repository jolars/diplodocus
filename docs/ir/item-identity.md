# Static item and identity contract

The version 1 workspace IR accepts optional typed `Item.language_data` and
`Item.aliases`. Both are omitted when absent, preserving existing version 1
snapshots. The map key remains the authoritative item ID. These records define
extractor output; they do not implement extraction or validate a whole item
graph on deserialization.

## Canonical entities and lookup names

Python implementations and maintained stubs contribute to one canonical item.
Use the defining module and lexical containers for its identity, such as
`foo.model.fit`. Record `foo.fit` as a `python-reexport` alias on that item,
with evidence from each contributing import declaration. A proven assignment
of the same object is likewise a `python-assignment` alias. Neither introduces
another item. Aliases are package-scoped lookup names, independent of URLs.

A named Python type alias is a distinct declaration with
`PythonDeclaration::TypeAlias` data and a `PythonIdentityKind::TypeAlias` key.
Its type expression may reference another item without sharing that item's
identity. A class, function, property, or field at the same qualified name also
has a distinct identity role. Arbitrary duplicate definitions with the same
canonical name and role are errors until the extractor reconciles them.

Every Python callable has one family item. An ordinary callable has an empty
overload list. An overloaded callable has separately addressable overload items
and an ordered list of their references; each member links back to its family.
Public lookup names and concepts address the family. The family carries the
reconciled public signatures, each with its own source evidence, while each
overload carries its individual signature. Implementation prose stays on the
family. There is no extra implementation entity merely because a stub exists.

An R S3 generic is its own callable family. Its methods have separate IDs and
link to the generic and dispatch class. An imported generic can remain an
external R package/name coordinate, such as `stats::predict`, without creating
an unsupplied package or generic item. Namespace registration evidence stays
separate from source definitions. The `exported` flag records a name export;
an unexported S3 method can still be public through its registration.

Rd aliases are apportioned to their actual canonical declarations. The three
aliases in `fit.Rd` address the generic and its two methods, respectively, even
though those declarations share documentation. Sharing a topic does not merge
those entities, and an Rd alias does not create a fourth entity.

## `sid1` identity construction

`SemanticIdentity` constructors produce an opaque package-local key.
`in_package` and `IdentityRegistry` qualify it with the stable workspace package
ID. Package slugs, public alias names, source paths, source ranges, declaration
ordinals, and absolute checkout roots are not identity inputs.

| Constructor | Identity inputs |
| --- | --- |
| `python` | Canonical qualified name and declaration role |
| `python_overload` | Canonical family name and role, plus normalized callable signature |
| `r_function` | Maintained function name, including an S3 constructor function |
| `r_s3_generic` | Canonical generic binding name |
| `r_s3_method` | Maintained method binding name, qualified generic, and dispatch class |

The wire key starts with `sid1:<language>:<role>:<name>`. Variable atoms retain
ASCII letters, digits, `_`, `.`, and `-`; every other UTF-8 byte is encoded as
uppercase `%HH`, including `%` itself. Nested signature expressions have
explicit node tags, option markers, delimiters, and ordered children. This
encoding is injective over the supported normalized inputs and uses no hash.
Consumers should treat keys as opaque and use typed item data for display.
The task-specific ID golden fixes the exact encoding for future changes.

Overload signatures include parameter names, order, calling conventions,
annotations, defaults, and returns. Missing defaults differ from explicit
`None`, `NULL`, or `...` literals. Resolved `Name.target` links do not enter the
key: resolving a reference must not rename its containing overload. Diagnostic
`LanguageSpecific.source` spelling is also excluded. The producer must encode
all semantic operators and operands in the node name and children.

Producers normalize literal spellings and identifier qualification before
constructing an overload ID. For example, quote style and insignificant
whitespace cannot be copied into a literal key when they change no semantic
value. This helper does not parse or evaluate language syntax. A
language-specific expression with diagnostic source but no semantic children
is rejected as opaque, so two unsupported expressions cannot silently collapse
to the same overload. A producer must diagnose unsupported identity inputs
instead of assigning a source-offset or sequence-number fallback.

Moving a checkout, changing a package slug, inserting comments, moving a
declaration, or reordering overload declarations leaves IDs unchanged when the
normalized semantics remain unchanged. Changing an overload's public signature
changes that overload's ID while preserving its family's ID. Changing a
canonical module/name, declaration role, package ID, or R dispatch identity can
change the corresponding reference.

## Duplicate and conflicting identities

`IdentityRegistry` is a package-local construction guard. Register all
reconciled canonical items before binding aliases. Its errors serialize with
stable codes and portable structured fields, ready for translation into the
common diagnostic layer with producer-supplied source evidence.

| Code | Behavior |
| --- | --- |
| `invalid-identity` | Missing semantic inputs, a noncallable overload, or opaque signature syntax cannot establish a key. Rejected raw values are not echoed. |
| `duplicate-item-identity` | Registering the same key twice fails without replacing the first registration. Reconcile compatible stub/source evidence before registration; diagnose incompatible definitions. Equal normalized overload signatures are duplicates, independent of declaration order. |
| `unknown-alias-target` | An alias to an unregistered item or another package fails without modifying the alias table. This registry resolves names within one supplied package. |
| `conflicting-item-alias` | Binding the same spelling to distinct items retains all candidates, sorted by package and item ID. Binding and subsequent lookup return ambiguity; neither order chooses a winner. Rebinding an unambiguous name to the same target succeeds. |

After a third or later conflicting binding, the error contains the complete
candidate set currently known. Producers should finish collecting candidates
and resolve names before emitting final diagnostics. This yields identical
diagnostics regardless of discovery order. No aliases count toward the number
of registered entities.

## Coverage and deferred producers

`tests/language_items.rs` hand-assembles the acceptance callable-family and S3
graphs. Its golden records four Python overloads, source/stub attribution,
canonical aliases, local and imported generics, and method registration and Rd
alias evidence. A separate variant golden covers modules, static/dynamic
exports, stub-only declarations, decorators, dataclasses, constructors,
properties, constants, fields, type aliases, and R constructors.

`tests/item_identity.rs` covers same-name roles, alias/entity boundaries,
duplicate and conflicting identities, delimiter escaping, signature structure,
and relocation, source-range shifts, and URL changes. These tests establish
representation and construction contracts. They do not claim production
source/stub/Rd reconciliation, public-surface extraction, or graph validation.
Those remain extractor work. Package-level Python metadata, including the
`py.typed` marker, also remains Milestone 4 work. S4 extraction is outside the
MVP S3 contract.
