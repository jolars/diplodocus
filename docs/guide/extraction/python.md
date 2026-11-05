# Python API extraction

Python extraction reads `pyproject.toml` metadata and maintained `.py` and
`.pyi` files. It recognizes statically visible functions, classes, methods,
properties, constants, fields, type aliases, overloads, and supported
decorators. A statically resolvable `__all__` controls exports; otherwise,
public definitions and explicit aliases form the visible surface. Maintained
stubs supply public signatures, while implementation docstrings supply prose.
PEP 257 and NumPy-style docstrings provide structured documentation, and simple
`:func:` roles can become cross-references.

Overloaded callables have one reference page containing their public signatures
and implementation docstring. Navigation and search list the callable once.
Individual overload identities remain in snapshots, and references to them link
to the corresponding signature on the callable's page.

Python pages prefer public reexport names over private implementation paths.
For example, a function defined as `datamonger._api.fetch_data` and exported as
`datamonger.fetch_data` uses the latter name in its heading, navigation, and
search result. Both names remain searchable and resolve to the same page.
Snapshots retain the defining name, source location, and export evidence.

Computed exports, conditional definitions, opaque signatures, unsupported
decorators, and other dynamic behavior cannot be inferred safely. Diplodocus
reports the limit instead of guessing an API. For a
public function decorated with `contextlib.contextmanager`, it preserves the
parameters and renders a generator's `Iterator[T]` or `Generator[T, ...]` return
annotation as `AbstractContextManager[T]`. An unsupported decorator on a public
declaration produces a diagnostic, and its callable signature is omitted because
the wrapper may change it. Unsupported decorators on internal declarations do
not prevent extraction of the public surface unless those declarations are
reexported.

Python doctest transcripts execute during `extract`, `build`, and `serve` through
the caller's `python` interpreter (Python 3.11 or newer). Prepare the checkout and
its example dependencies first. Diplodocus loads the selected checkout, verifies
the module's source path, and starts a fresh process per API document. Examples
within one document share the defining module's globals. The standard library
`doctest` runner checks expected output and exceptions, supports comparison
directives, and records explicit `+SKIP` outcomes. Output mismatches and unexpected
errors prevent publication. Ordinary code fences remain illustrative.

