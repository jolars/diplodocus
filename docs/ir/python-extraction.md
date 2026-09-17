# Static Python extraction

`extractors::python::extract_target` consumes one resolved repository, package,
and extraction target. It returns a schema-versioned `PythonExtraction`
fragment containing metadata, canonical items, diagnostics, and provenance.
The caller must reject error-bearing fragments before building a site.
Workspace merging, reference resolution, routes, and CLI integration remain
later pipeline stages.

The implementation composes three independently testable passes:

- `parse_target` reads static PEP 621 metadata and maintained `.py`/`.pyi`
  inputs into Diplodocus-owned source observations.
- `surface::reconcile` establishes public items, source/stub precedence,
  callable families, overload identities, and aliases.
- `docstrings::parse_docstring` translates decoded PEP 257 and NumPy-style
  documentation into inert document IR with original-source diagnostics.

No pass starts Python, imports a documented package, installs dependencies, or
invokes a build backend. Source discovery stays inside the configured target
and package boundaries. Directory symlinks are diagnosed; file symlinks must
remain inside the package. A directory target names its importable package or
namespace-package root. A file target derives qualification from enclosing
`__init__` directories within the package boundary.

## Public identities and signatures

A statically resolvable `__all__` is authoritative. Supported operations include
literal lists and tuples, supported helper bindings and concatenation,
`+=`, `append` of a literal name, and `extend` of a literal sequence. Unknown
computation produces `python-dynamic-export` and leaves visible definitions
with unknown visibility.

Without `__all__`, public definitions and explicitly aliased imports are
exposed. Redundant aliases such as `from .model import fit as fit` count as
explicit. Unaliased imports used for annotations do not become public by
default. Underscore-prefixed modules require public reexports to expose their
items. A public reexport retains the canonical defining module without
promoting its unrelated private declarations.

Maintained stubs select the public declaration surface and signatures.
Implementation prose and constant values retain separate evidence. Stub-only
native modules are valid inputs. Reexports and proven assignment aliases use
the defining item's identity. Every callable has a family, and overloads also
have individually addressable signature-based identities. The
[item identity contract](item-identity.md) defines the wire keys.

The supported surface includes functions, classes, methods, constructors,
properties, constants, fields, explicit type aliases, and the supported
overload, property, static/class method, and dataclass decorators. Signatures
retain parameters, calling conventions, annotations, defaults, returns, and
async state. Unsupported decorators, conditional declarations, generic type
parameters, and opaque signature expressions produce diagnostics.

## Documentation and evidence

Parameters, Returns, Raises, Notes, References, and Examples become structured
headings, entries, prose, and display code. Simple `:func:` roles become semantic
references, including the acceptance corpus's references to `fit`. Other
unsupported roles, directives, sections, and raw HTML remain visible
placeholders. Neither docstring examples nor their hashpipe options authorize
execution.

Document ranges address decoded UTF-8 text. Proven unchanged-byte segments map
diagnostics back to original Python source. Escapes, missing mappings, and
ranges crossing concatenation gaps use the enclosing source range with an
explicit attribution warning. The adapter never guesses offsets.

Provenance records static mode, implemented capabilities, actual parser use,
grammar and source/stub settings, and hashes of contributing inputs, including
`py.typed`. Unknown or invalid grammar permits diagnostic parsing only. Invalid
UTF-8 retains its original bytes and hash without claiming successful parsing.
Portable paths are repository-relative, and no timestamp or checkout root is
introduced by extraction.

`tests/milestone_four.rs` combines the passes against the acceptance workspace:
26 canonical items, four overload members, 16 attached documents, and no
baseline diagnostics. It compares relocated workspaces, checks the isolated
dynamic-export warning, and verifies that import failures and build-backend
hooks are not executed. Component tests cover malformed inputs, conflicting
identities, visibility, source attribution, and inert examples.
