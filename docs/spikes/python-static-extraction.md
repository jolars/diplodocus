# Python static-extraction spike

## Outcome

The MVP Python extractor will use a Rust-native stack. API extraction does not
use a Python interpreter, a Python helper process, or package imports.

Use these components behind a small Polydoc-owned adapter:

- [`pyproject-toml` 0.13.7](https://docs.rs/pyproject-toml/0.13.7/) for PEP 621
  project metadata and its typed PEP 440 and PEP 508 values;
- [`ruff_python_parser` 0.0.12](https://docs.rs/ruff_python_parser/0.0.12/),
  [`ruff_python_ast` 0.0.12](https://docs.rs/ruff_python_ast/0.0.12/), and
  [`ruff_text_size` 0.0.12](https://docs.rs/ruff_text_size/0.0.12/) for Python
  and stub syntax, tokens, and byte ranges; and
- [`pydocstring` 0.4.1](https://docs.rs/pydocstring/0.4.1/) for NumPy-style
  docstring structure and byte ranges.

The exact versions are intentional. Ruff describes these crates as internal
components with unstable Rust APIs. `pydocstring` is also young. An adapter
keeps either dependency from shaping Polydoc's extractor interface or portable
IR.

This report selects parsing infrastructure; it does not move the Milestone 4
extractor implementation into the spike. The production extractor still owns
the semantic passes that turn syntax into a public API.

## Boundary

```text
pyproject.toml ── pyproject-toml ───────────────┐
                                                │
.py/.pyi ─────── Ruff AST ── Polydoc semantics ├── portable API IR
                      │                         │
                      └── pydocstring ──────────┘
```

The Polydoc semantic layer must:

- build the configured module graph;
- evaluate only the supported static subset of `__all__`;
- apply default visibility when `__all__` is absent;
- resolve imports and re-exports to canonical identities;
- recognize supported decorators and callable families;
- reconcile `.py` definitions with `.pyi` declarations using explicit field
  precedence; and
- translate structured docstring sections and supported inline markup into
  document IR.

None of those operations inherently requires Python. They are package semantics,
not parser behavior.

Python remains necessary later for explicitly authorized authored-cell execution
through Jupyter. That execution path is separate from API extraction. A static
extractor must never start a kernel or import the documented package.

## Candidate comparison

### Python syntax

| Candidate | Result | Reason |
| --- | --- | --- |
| Ruff parser and AST | Select | Typed current-Python AST, distinct Python and stub source types, tokens, recoverable errors, unsupported-version errors, and byte ranges. All acceptance sources parse successfully. |
| `rustpython-parser` 0.4.0 | Reject | It provides a typed AST and optional ranges, but its maintained repository says that Ruff's parser supersedes it. |
| `tree-sitter-python` 0.25.0 | Reject | Its concrete syntax tree and error recovery are useful for editor tooling, but Polydoc would need a larger typed-AST adapter and more validation to distinguish malformed or unsupported syntax. It offers no API semantics. |
| `python-parser` 0.2.0 | Reject | Its documented grammar stops at Python 3.8-era syntax and is unsuitable for the declared Python 3.11 package. |

Ruff's existing semantic-analysis crates are implementation details of Ruff and
its type checker; they do not expose a stable, package-documentation model.
Depending on them would not remove Polydoc's need to define export, identity,
and stub-merging rules.

### Package metadata

| Candidate | Result | Reason |
| --- | --- | --- |
| `pyproject-toml` | Select | Models PEP 621, distinguishes dynamic fields, and parses versions, Python requirements, and dependencies into packaging-aware types without invoking the configured build backend. |
| Generic TOML deserialization | Reject | It would leave Polydoc responsible for PEP 440, PEP 508, normalized package names, and the evolving `pyproject.toml` schema. |
| Build-backend metadata hooks | Reject | They may execute backend code and violate the static-extraction contract. Dynamic required metadata must instead produce a diagnostic. |

`pyproject-toml` reports TOML parse spans but does not retain a source range for
each successfully decoded field. The current IR requires the metadata file as
provenance, not a field-level span, so this is sufficient for the MVP.

### Docstrings

| Candidate | Result | Reason |
| --- | --- | --- |
| `pydocstring` | Select | Zero dependencies, explicit NumPy parsing, a typed unified view, a source-backed concrete tree, and byte ranges. It recognizes all required fixture sections and entries. |
| Ruff docstring utilities | Reject as the section parser | Ruff provides useful lexical helpers but no NumPy-section model. |
| `docstring` 0.2.4 | Reject | Normalizes indentation only; it does not model Python or NumPy docstring sections. |
| Polydoc-specific parser | Defer | The selected crate already supplies the required section grammar and ranges. Polydoc still needs a narrow adapter for inline markup and document IR. |

`pydocstring` ranges address the decoded string passed to it. For the acceptance
corpus, the decoded bytes equal the bytes inside each triple-quoted literal, and
the spike verifies conversion back to file offsets. The production adapter must
add a raw-to-decoded source map for escapes, line continuations, and implicitly
concatenated strings. If exact mapping is impossible for a construct, extraction
must retain the enclosing docstring span and emit an information-loss diagnostic.

### Excluded hybrid alternative

[Griffe](https://mkdocstrings.github.io/griffe/) is the strongest hybrid
alternative. It already performs static API collection, alias resolution, stub
merging, overload collection, and NumPy docstring parsing, and it can serialize
the result as JSON while inspection is disabled.

Do not use Griffe as an extractor dependency or fallback. The Rust spike exposes
all syntax and documentation information needed by the acceptance matrix, and
the remaining work encodes Polydoc-specific identity and provenance rules that
would still need an adapter around Griffe. Griffe may serve as a behavioral
reference while developing fixtures, but it is not a supported extraction mode.

## Acceptance-matrix results

The executable probe is
[`tests/python_extraction_spike.rs`](../../tests/python_extraction_spike.rs).
It runs entirely in the Rust test process.

| Acceptance construct | Available from the selected stack | Polydoc-owned work |
| --- | --- | --- |
| PEP 621 name, version, description, Python requirement, and dependency | Typed values from `pyproject-toml` | Provenance and diagnostics for dynamic required fields |
| Module docstring, literal `__version__`, and literal `__all__` | Ruff string and collection expressions with ranges | Supported constant-expression evaluator |
| Relative imports and explicit re-exports | Ruff import level, module, names, aliases, and ranges | Module graph and canonical identity resolution |
| Package `.pyi` surface | Ruff stub parse mode exposes the same typed nodes | Merge with the implementation surface |
| `py.typed` | Direct file presence | Record typed-package status and provenance |
| Annotated constants, values, and following attribute docstrings | Ruff annotated assignments, expressions, and adjacent string statements | Associate attribute docstrings and classify safe literal values |
| Dataclass and fields | Ruff decorators, call arguments, class bodies, and annotations | Recognize supported decorator semantics |
| Constructor, properties, methods, and private helpers | Ruff function nodes, parameters, decorators, names, and ranges | Visibility and callable-family construction |
| PEP 257 prose and NumPy sections | Ruff string values plus `pydocstring` summary, sections, entries, citations, and ranges | Inline markup and document-IR translation |
| Maintained `.pyi` signatures | Ruff annotations, parameters, defaults, and return values | Field-by-field stub precedence with separate provenance |
| Function and method overloads | Repeated function nodes with `overload` decorators; positional-only and keyword-only groups remain distinct | Group into stable, addressable callable families |
| Implementation documentation plus stub signature | Both sources retain independent ranges | Reconcile them without duplicate items |
| Stub-only extension module | Normal Ruff stub module; no binary or import is needed | Include it in the configured module graph |
| Computed `__all__` | Ruff exposes a call expression rather than a literal collection | Emit `python-dynamic-export` without evaluating the call |

## Static semantic verification

The `package_surface_is_reconciled_statically_without_importing` probe goes
beyond checking for isolated AST nodes. It constructs the acceptance package's
public surface directly from source text and verifies these rules:

- the literal `__all__` values in `__init__.py` and `__init__.pyi` agree and are
  available as the authoritative export set;
- relative imports in both files resolve to identical package-qualified targets,
  including aliases into the stub-only `foo._native` module;
- a matching `.pyi` declaration supplies annotations and callable signatures,
  while the implementation continues to supply its docstring;
- repeated `@overload` declarations form the selected signatures for the `fit`
  and `FooModel.predict` families instead of the broader implementation
  signature;
- positional-only parameters, return annotations, property, overload, and
  dataclass decorators remain available as typed syntax; and
- re-export declarations, annotations, signatures, and docstrings retain byte
  ranges that select the original source exactly.

The fixture deliberately has no `_native.py` or compiled extension. Importing
`foo` would therefore fail when `__init__.py` reaches its `_native` re-export,
yet the Rust-only probe obtains `NativeWorkspace`, `native_mean`, and their
canonical targets from `_native.pyi`. The test calls no Python executable,
import machinery, build backend, or package code.

This is a proof of representability, not the production extractor. The compact
semantic helpers remain test-local; Milestone 4 will replace them with
Polydoc-owned adapters, diagnostics, IR, and golden tests.

## Consequences for implementation

The production extractor should introduce one internal adapter per selected
crate and convert immediately into Polydoc-owned types. Tests should never match
large Ruff debug representations or expose Ruff types through public APIs.

The next Python extractor work should proceed in this order:

1. Define golden Polydoc IR for the fixture before implementing extraction.
2. Implement metadata conversion and the configured module graph.
3. Implement a deliberately small static-expression evaluator for exports and
   literal values.
4. Resolve re-exports and establish canonical identities.
5. Reconcile stubs, implementation definitions, overloads, and provenance.
6. Adapt docstrings into document IR, including raw-to-decoded source mapping.

If a later real-world fixture cannot be represented by this stack, add a focused
fixture and diagnostic first. Do not introduce a Python parser helper; Python is
reserved for explicitly authorized code execution.
