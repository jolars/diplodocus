# Static-extraction failure modes

## Outcome

The Python and R spikes expose enough information to distinguish malformed
input from syntax that is valid but unsupported, dynamic semantics that static
extraction must not evaluate, and conversions that cannot retain exact source
information. They do not report all of those conditions themselves. The
production adapters must translate native parser output and add semantic
diagnostics at the boundary described below.

These observations apply to the exact dependency versions pinned in
`Cargo.toml`. Native messages and types are implementation details; they must
not become Diplodocus's public diagnostic codes. The acceptance matrix already
fixes two public outcomes:

- a computed Python export list emits warning `python-dynamic-export`; and
- an unevaluated `\Sexpr` emits warning `unsupported-rd` and a visible
  placeholder.

The corresponding version, mode, capability, and provenance declarations are
defined in
[`static-extractor-contract.md`](static-extractor-contract.md).

The diagnostics milestone will assign stable codes to the other classes. This
spike fixes their detection, severity rule, location quality, recovery policy,
and information-preservation requirements.

## Common adapter contract

Every parser report becomes a Diplodocus diagnostic with a stable class, a
repository-relative file, the strongest available source range, and the native
message as detail. Diagnostics are sorted by file and range. An error in one
declared input prevents that input from contributing an authoritative package
fragment, but does not prevent independent inputs from being inspected.

Valid syntax is not proof of supported semantics. Each adapter must inspect the
semantic cases listed below and report uncertainty itself. A warning may retain
an item when its identity and visibility remain known. An error is required
when uncertainty can change a package's identity, public surface, or signature.
Neither case authorizes runtime evaluation.

When translation cannot faithfully represent a construct, the adapter retains
the raw source and emits a visible unsupported placeholder where a range is
known. If it cannot prove an exact range, it uses the smallest enclosing range
it can prove—possibly the file—and also reports an incomplete-source-location
diagnostic. It never silently drops public information.

## Python extractor

| Condition | Selected stack behavior | Diplodocus adapter behavior |
| --- | --- | --- |
| Malformed `pyproject.toml` | `PyProjectToml::new` returns one `toml::de::Error` with a byte span and no partial typed value. Invalid PEP 440 and PEP 508 values arrive through this path as deserialization errors. | Report an error at the TOML span and reject the metadata fragment. Retain the source file for context; never invoke the build backend to repair or complete it. |
| Malformed `.py` or `.pyi` | Ruff's unchecked parse returns a recovered AST, `ParseError` values, and byte ranges. | Report every parse error and do not derive public items from the malformed file. Recovery is useful for diagnostics, not authority. |
| Syntax outside the declared Python version | Ruff reports `UnsupportedSyntaxError` separately from parse errors, with a typed kind, target version, and byte range. The probe uses a Python 3.12 `type` statement under a Python 3.11 target. | Report an error naming the declared version and required syntax version. Configure Ruff from `requires-python`; do not use its newer default implicitly. |
| Valid syntax outside the supported extraction subset | Ruff accepts arbitrary decorators, calls, imports, and expressions without an extraction diagnostic because they are ordinary Python syntax. | The semantic pass reports the unsupported decorator, unresolved re-export, conflicting stub, or other unsupported public construct at its AST range. |
| Dynamic project metadata | `project.dynamic` is a typed list. A declared dynamic field has no value and produces no parser diagnostic. | Report an error when the package IR requires that field. Report a warning for a recognized but unused dynamic field. Do not call a PEP 517 metadata hook. |
| Dynamic exports or values | Ruff represents a computed `__all__`, such as `_exported_names()`, as an ordinary call expression and emits no diagnostic. | Apply only the documented constant-expression subset. For the acceptance fixture, retain the module and visible definition, emit `python-dynamic-export` at the expression, and do not promote the uncertain package export. Other dynamic public values follow the same warning-or-error rule based on whether identity or signature is uncertain. |
| Incomplete metadata locations | Successfully deserialized `pyproject-toml` fields have no field-level ranges. | Use the metadata file as provenance. A later requirement for field-level source links needs a TOML syntax layer; it must not be inferred from a text search. |
| Incomplete docstring locations | Ruff locates the raw literal. `pydocstring` ranges address the decoded string given to it. The offsets coincide for the acceptance corpus, but escapes, continuations, and implicit concatenation make direct offset addition incorrect. | Build a raw-to-decoded map. If one decoded region cannot be mapped exactly, retain the enclosing literal range and report incomplete source location. |
| Incomplete docstring structure | `pydocstring` is total and emits no diagnostic stream. Missing expected parts appear as zero-length CST placeholders. | Scan the source-backed CST for missing placeholders and report incomplete syntax before translating the typed `Document` view. Preserve usable prose. |
| Conversion loss | Ruff's `Parsed` value retains tokens, whereas the AST alone does not retain all trivia. `pydocstring::Parsed::to_model` normalizes text and deliberately drops byte positions. Typed project metadata also omits irrelevant raw TOML structure. | Keep each raw declared input, retain Ruff tokens while adapting, and use `pydocstring`'s typed view plus CST rather than `to_model`. Report any unsupported public syntax before replacing it with an opaque IR node. |

## R extractor

| Condition | Selected stack behavior | Diplodocus adapter behavior |
| --- | --- | --- |
| Malformed `DESCRIPTION` grammar | Arity's DCF parser returns a total, byte-for-byte lossless CST. Its `ParseDiagnostic` side channel contains a message and byte start/end; malformed lines remain in the tree, and later valid fields remain readable. | Report each DCF error at its range and reject the metadata fragment. Do not hide an error merely because the required fields can still be recovered. |
| Malformed dependency metadata | DCF syntax can be valid while a dependency constraint is not. `dependency_entries` retains the package name and source ranges and marks `malformed_constraint()` without adding a parse diagnostic. | Emit a semantic metadata error over the constraint or entry range. Keep the dependency name for diagnostic context, but do not treat the malformed constraint as unconstrained. |
| Malformed maintained R source | Arity returns a recovered, lossless CST plus ranged `ParseDiagnostic` values. | Report all syntax errors and exclude definitions from the malformed file from the authoritative surface. Never source the file to obtain a second opinion. |
| Unsupported `NAMESPACE` syntax | Ordinary R syntax errors remain parser diagnostics. A named but unknown directive also produces `unsupported NAMESPACE directive`; its full call survives as `DirectiveKind::Unsupported` with ranges. | Treat an unsupported directive as an error because it may alter visibility or registration. Retain its exact source as an opaque declaration. |
| Dynamic namespace conditions | Arity deliberately does not evaluate conditions. Its convenience iterator flattens directives from both branches without a diagnostic; the lossless CST still contains the condition and branch structure. | Detect conditional containers from the CST, report that the namespace surface is dynamic, and do not treat both branch exports as unconditional. No R process may evaluate the condition. |
| Dynamic source construction | Calls such as `assign` and computed names are valid R syntax and have no native diagnostic. | Reconcile the static named definitions against `NAMESPACE` and `Rd`. If an exported identity lacks a supported static definition, report the unresolved or dynamic definition instead of sourcing the file. |
| Dynamic Rd markup | `rd-source` parses `\Sexpr`; `rd-ast` exposes its code and options and always reports its state as `Unresolved`. Valid dynamic markup has no native parse diagnostic. | Emit `unsupported-rd`, preserve surrounding sections, create a visible placeholder, and never evaluate the expression. |
| Unknown or malformed Rd | Recoverable Rd problems carry a severity, typed `DiagnosticCode`, byte span, and one-based line/column positions. An unknown tag is a native error and survives as `RdTag::Unknown` with its children. Hard input failures, including invalid UTF-8, NUL, unsupported encoding, excessive size, and excessive nesting, return `ParseError`. | Map every native diagnostic or hard failure. Preserve recoverable unknown markup as an opaque node; reject a document on hard failure. |
| Incomplete Rd locations | `rd-source` gives exact ranges for diagnostics, but its successful semantic `RdDocument` and `RdNode` values carry no byte ranges. `RdPath` identifies tree structure, not source. | The production adapter needs an upstream range API or its own source map before it can satisfy exact node and `\Sexpr` provenance. Until then, use file-level provenance and emit an incomplete-source-location diagnostic; do not claim an exact range. |
| Conversion loss | Arity's DCF `folded_value` normalizes continuation whitespace, and the namespace directive iterator removes conditional context. `rd-ast` convenience accessors such as `description()` are explicitly lossy and choose the first duplicate. Strict `inspect_*` accessors instead return `RdShapeError`, but only with an `RdPath`. | Retain the lossless DCF and R CSTs, use raw field ranges as well as folded values, inspect namespace containers before flattening, and use strict Rd accessors. Convert every shape error into an information-loss diagnostic. Never use `text_contents` or a lossy accessor as the sole representation of supported markup. |

## Executable evidence

[`tests/python_extraction_spike.rs`](../../tests/python_extraction_spike.rs)
locks in malformed and dynamic metadata, Ruff's separate parse and
unsupported-version channels, `pydocstring` missing placeholders, and the
raw-to-decoded range mismatch.

[`tests/r_extraction_spike.rs`](../../tests/r_extraction_spike.rs) locks in
lossless DCF and R recovery, semantic dependency failures, opaque unsupported
namespace directives, unevaluated namespace branches, unresolved `\Sexpr`,
unknown Rd nodes with ranged parser diagnostics, and strict Rd shape errors
whose only semantic location is an `RdPath`.
