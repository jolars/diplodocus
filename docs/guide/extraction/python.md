# Python API extraction

Python extraction imports the selected checkout and uses Sphinx autodoc and
Napoleon to collect API declarations and structured documentation. Prepare the
package, dependencies, and compiled extensions first. The caller's `python`
interpreter must provide Python 3.11 or newer and Sphinx `>=9.0.4,<10`; the
selected Sphinx release must support that interpreter. Diplodocus does not
install dependencies or run a build backend.

`check`, `extract`, `build`, and `serve` share native discovery. `check` imports
package code without running documentation examples. Import errors, checkout
mismatches, and native failures remain errors. `generate` reads a completed
snapshot and needs neither Python nor the producer checkout.

The built-in Sphinx configuration enables autodoc and Napoleon. It supports
reStructuredText, NumPy, and Google docstrings. Diplodocus does not discover or
load project `conf.py`, third-party extensions, MyST, or separately rendered
Sphinx sites. Markdown fences in docstrings produce an unsupported-syntax
diagnostic. Authored Markdown and QMD pages keep their own authoring rules.

The loaded package supplies computed exports, generated declarations, native
signatures, descriptors, and attached documentation. Runtime `__all__` controls
exports. Without it, public definitions and explicitly public import aliases
form the visible surface. Annotation helpers do not become API items merely
because they were imported. Unsupported observations produce diagnostics
instead of a static fallback.

Maintained `.pyi` files select their public declarations and signatures.
Implementation documentation supplies prose when available; maintained stubs
provide fallback documentation. Stub-only modules
remain documentable without a runtime counterpart, but a stub does not make a
failed runtime import succeed. A package importing an unavailable native
extension still requires that extension to be prepared.

Overloaded callables have one reference page containing their public signatures
and implementation documentation. Navigation and search list the callable once.
Individual overload identities remain in snapshots and link to signature
anchors. Public reexport names appear in headings, navigation, and search while
snapshots retain defining names, aliases, and source evidence.

Python doctest transcripts execute during `extract`, `build`, and `serve` in the
prepared environment. Each API document gets a fresh process, and its examples
share the defining module's globals. The standard-library `doctest` runner
checks output and expected exceptions, supports comparison directives, and
records explicit `+SKIP` outcomes. Failed examples prevent publication. Ordinary
code blocks remain illustrative.

Doctests in stub-only modules require an importable runtime module, even though
their API declarations can be documented without one.

Discovery and examples have separate caches. Source, stub, dependency,
interpreter, Sphinx, and native-library changes invalidate affected results.
Additional maintained reads and environment inputs use the existing
`incremental` configuration. Uncertain dependency coverage disables reuse.
Failed watched builds preserve the previous successful site.
