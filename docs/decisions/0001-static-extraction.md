# 0001: Extract Python and R APIs statically in Rust

Status: Accepted. Date: 2026-09-09.

## Context

Diplodocus must document the configured Python and R packages without importing
them, loading a namespace, installing dependencies, or running package code. The
acceptance workspace includes a Python extension represented only by a stub,
computed exports, R S3 methods, and dynamic `Rd`. Runtime inspection cannot be a
prerequisite or an implicit recovery path.

Extraction must preserve language-specific structure and the evidence for each
fact. Syntax recognition alone does not settle visibility, canonical identity,
stub precedence, or whether a documentation expression can be evaluated. The
spikes separate those responsibilities from parsing.

## Decision

Use the following exact dependencies behind Diplodocus-owned adapters. The
versions match [Cargo.toml](../../Cargo.toml) and the [static-extractor
contract](../spikes/static-extractor-contract.md).

  | Input                             | Selected components                                                     | Adapter responsibility                                                                                                                                      |
  | --------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | Python metadata                   | `pyproject-toml` 0.13.7                                                 | Read static PEP 621 values and typed packaging constraints; diagnose required dynamic fields.                                                               |
  | Python source and stubs           | `ruff_python_parser`, `ruff_python_ast`, and `ruff_text_size` 0.0.12    | Select Python/stub mode and the package's target grammar, then build the configured module graph, exports, re-exports, declarations, and callable families. |
  | Python documentation              | `pydocstring` 0.4.1 in NumPy mode                                       | Convert section structure and inline markup into document IR, retaining decoded and original-source attribution.                                            |
  | R metadata, namespace, and source | `arity-parser` 0.6.0 with its DCF, namespace, and R parsers             | Retain source and ranges, validate dependency constraints and namespace context, and reconcile named functions, formals, exports, and S3 registrations.     |
  | Maintained R documentation        | `rd-source` and `rd-ast` 0.4.0, with `rd-ast` default features disabled | Parse checked-in `Rd` directly; consume strict semantic views and preserve markup structure and unresolved dynamic nodes.                                   |

Both extractors have mode `static`. Their version is the Diplodocus crate
version; parser versions and capability lists remain separate provenance. Native
parser types stay inside the adapter. Public extraction results, diagnostics,
and serialized IR use Diplodocus-owned types.

For Python, literal supported `__all__` declarations govern the public surface.
Resolve relative imports and re-exports to canonical package-scoped identities.
Use maintained `.pyi` annotations and signatures in preference to corresponding
implementation declarations, retain implementation documentation, and group
overloads without duplicating the public item. Stub-only native declarations are
valid inputs. Decorator semantics and constant evaluation remain an explicitly
supported subset, never arbitrary Python execution.

For R, use maintained `DESCRIPTION`, `NAMESPACE`, R source, and `Rd` together.
Reconcile S3 registrations, generic/method definitions, and usage documentation;
do not infer the public surface from a flattened namespace iterator when
conditional context matters. `Authors@R`, defaults, examples, and `\Sexpr`
remain source or structured unevaluated content. Documentation generation from
roxygen and installed help databases are outside the selected input boundary.

### Failure and provenance rules

The [failure-mode report](../spikes/static-extraction-failure-modes.md) defines
native evidence, severity, recovery, and information-loss cases. A malformed
input cannot contribute an authoritative fragment merely because the parser
recovers some nodes. Independent inputs may still be inspected. Unknown or
dynamic semantics produce visible diagnostics instead of guessed facts.

In particular, computed Python exports produce `python-dynamic-export` and
unevaluated `\Sexpr` produces `unsupported-rd` with a visible placeholder.
Unsupported namespace directives and unknown conditional visibility require
errors because the public surface is uncertain. Strict `Rd` shape failures must
not silently select the first duplicate section.

Preserve separate provenance for a stub signature, implementation docstring,
namespace registration, source definition, and `Rd` documentation. Paths are
repository-relative, and ranges are zero-based, half-open UTF-8 byte ranges.
Metadata may have only file-level provenance. Decoded Python docstrings require
a source map when escapes or concatenation change offsets. Successful `Rd` nodes
currently have structural paths rather than byte ranges: use file-level
provenance and diagnose incomplete locations until a source map exists. Do not
invent exact ranges from a structural path or substring search.

## Alternatives

The [Python comparison](../spikes/python-static-extraction.md) records the
candidate investigation and acceptance results. These are decisions for the
pinned spike, not a claim that every rejected parser lacks useful capabilities.

  | Alternative                                                                           | Decision and reason                                                                                                                                                                                                                                                    |
  | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | Griffe or another Python helper, even in static mode                                  | Reject as a dependency or fallback. It introduces a Python runtime while leaving Diplodocus's identity and provenance adaptation necessary. It can be a behavioral reference for fixtures.                                                                             |
  | Import/inspection, PEP 517 metadata hooks, or package builds                          | Reject. They can execute documented or backend code and fail the static boundary.                                                                                                                                                                                      |
  | `rustpython-parser`                                                                   | Reject in favor of Ruff, following the earlier candidate comparison's maintenance assessment.                                                                                                                                                                          |
  | `tree-sitter-python` or generic concrete-syntax traversal                             | Reject for the Python adapter. Ruff supplies the required typed AST, parser diagnostics, target-version checks, and ranges with less custom interpretation.                                                                                                            |
  | `python-parser` 0.2.0                                                                 | Reject for the declared Python 3.11 fixture because the compared grammar is too old.                                                                                                                                                                                   |
  | Generic TOML decoding                                                                 | Reject for package metadata. The selected crate already models the packaging schema and constraints.                                                                                                                                                                   |
  | Ruff lexical docstring helpers, indentation-only `docstring`, or a new section parser | Reject the first two as the NumPy section parser; defer a new implementation because `pydocstring` exposes the required sections and ranges.                                                                                                                           |
  | `Rscript`, package/namespace loading, `tools::parse_Rd()`, or roxygen execution       | Reject. They require a language runtime or execution to obtain facts available from maintained source.                                                                                                                                                                 |
  | Regexes, generic text parsing, or a new R/Rd parser                                   | Reject as the primary path. The selected native grammars already preserve formal/default structure, namespace declarations, Rd markup, and diagnostic evidence. This is an assessment against the boundary and corpus, not a performance benchmark of other R parsers. |
  | Lossy `Rd` convenience views or Rd-to-Markdown conversion as the extraction boundary  | Reject. They can flatten markup, hide duplicate sections, and discard the distinctions needed for API identity and source attribution.                                                                                                                                 |

## Evidence and consequences

[Python spike tests](../../tests/python_extraction_spike.rs) and [R spike
tests](../../tests/r_extraction_spike.rs) obtain these observations inside the
Rust process. The Python fixture's unavailable native module makes an
import-based approach unsuitable, yet its stub declarations remain readable.
Focused tests exercise malformed metadata and syntax, unsupported Python
versions, namespace conditions, unresolved Rd expressions, and source-location
gaps. The [contract tests](../../tests/static_extractor_contract.rs) check exact
pins, capability lists, and provenance requirements.

The [Python golden](../../tests/snapshots/spikes/python.json) preserves every
fixture module's declarations, exports, aliases, annotations, documentation, and
the spike's stub/implementation reconciliation. The [R
golden](../../tests/snapshots/spikes/r.json) preserves metadata, dependency
constraints, namespace declarations, maintained functions/formals, and all four
Rd trees without flattening markup. Their [capture
guide](../spikes/golden-fixtures.md) distinguishes observations from the future
portable API schema.

This accepts the parsing and semantic boundaries; it does not claim production
extractors exist. Milestone 3 defines the serialized model and diagnostics, and
Milestones 4 and 5 implement adapters and semantic passes. Keep the spike
goldens during that replacement, compare the same facts through the production
adapter, and retire a spike only after its evidence has equivalent coverage.
Upstream changes or newly encountered constructs require focused regression
fixtures and an explicit capability/diagnostic decision, not a runtime fallback.
