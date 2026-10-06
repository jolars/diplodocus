# Python and R API extraction

Diplodocus reads configured API targets statically. It does not import a Python
package, load an R package, run a build backend, or install dependencies.
Targets must point to maintained files inside their declared package and
repository boundaries. The [workspace guide](configuration.md) shows how to
declare a target.

## Python

Python extraction reads `pyproject.toml` metadata and maintained `.py` and
`.pyi` files. It recognizes statically visible functions, classes, methods,
properties, constants, fields, type aliases, overloads, and supported
decorators. A statically resolvable `__all__` controls exports; otherwise,
public definitions and explicit aliases form the visible surface. Maintained
stubs supply public signatures, while implementation docstrings supply prose.
PEP 257 and NumPy-style docstrings provide structured documentation, and
simple `:func:` roles can become cross-references.

Computed exports, conditional definitions, opaque signatures, unsupported
decorators, and other dynamic behavior cannot be inferred safely. Diplodocus
reports the limit instead of executing package code or guessing an API.
For a public function decorated with `contextlib.contextmanager`, it preserves
the parameters and renders a generator's `Iterator[T]` or `Generator[T, ...]`
return annotation as `AbstractContextManager[T]`. An unsupported decorator on
a public declaration produces a diagnostic, and its callable signature is
omitted because the wrapper may change it. Unsupported decorators on internal
declarations do not prevent extraction of the public surface unless those
declarations are reexported.

## R

R extraction reads `DESCRIPTION`, `NAMESPACE`, maintained `R/` source, and
checked-in `man/` Rd files. Supported static namespace declarations include
`export`, `import`, `importFrom`, `S3method`, and literal `useDynLib` declarations.
It connects exports and S3 registrations to direct source definitions, then
attaches supported Rd topics,
aliases, usage, arguments, and prose. Rd examples remain display-only.

Computed names, conditional namespace directives, dynamic construction,
roxygen generation, and unsupported Rd markup are not evaluated. Unsupported
markup stays visible with a diagnostic. For exact parser and identity rules,
see the [Python extraction contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/python-extraction.md)
or [R extraction contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/r-extraction.md)
in the source repository.
