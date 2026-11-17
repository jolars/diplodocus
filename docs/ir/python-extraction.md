# Native Python extraction

Python command discovery uses the prepared checkout as the authority for API
semantics. Assembly supervises an embedded exporter, verifies checkout identity,
and normalizes observations into `PythonExtraction` for the common adapter
boundary. Source and stub parsers supply supplemental syntax, maintained
signatures, and source evidence.

## Native boundary

The exporter uses public Sphinx application, extension-event, Builder, and
doctree APIs. It enables autodoc and Napoleon in a temporary built-in
configuration. It does not subclass the legacy autodoc `Documenter` hierarchy,
use private import machinery, load project `conf.py`, or import rendered HTML.
Sphinx `>=9.0.4,<10` is supported, subject to each release's Python requirements.
Python 3.11 remains the minimum interpreter version.

The private, versioned exchange carries runtime bindings, signatures, class
shapes, aliases, processed documentation, original docstrings, diagnostics,
exact versions, dependencies, and consumed inputs. Unknown fields and protocol
versions are rejected. Absolute runtime paths and temporary Sphinx state stay
local to assembly.

Rust parses native signature syntax with Ruff and uses the common identity and
maintained-stub normalization helpers. Generated syntax is a parsing aid; its
offsets never become source evidence. Runtime origins and documentation inputs
must remain within the selected package. Installed dependencies contribute
local environment observations rather than repository declarations.

## Public surface and identities

Runtime `__all__`, including computed lists, supplies effective exports. Without
it, public definitions and explicit import aliases form the visible surface.
Private defining modules remain support records when their declarations are
reexported. Their unrelated private members and module prose stay private.
Disjoint targets can reexport the same canonical item. When a target selects its
definition, that target supplies the declaration and the other targets supply
the reexports. Generated children registered under a checkout module and exposed
by that module use its file as source evidence.

Canonical identities use defining modules and qualified names. Reexports and
assignment aliases resolve to canonical items. Callable families retain
separately addressable overload identities based on normalized signatures.
Public aliases remain the preferred presentation names without changing
identities or attribution. See the [identity contract](item-identity.md).

Native observations supply generated declarations, decorated callables,
properties, methods, dataclass fields, constructors, bases, and attached prose.
Maintained stubs can supply property signatures for runtime data descriptors.
The existing context-manager return presentation remains available through
normalization. Unrepresentable signatures must not acquire guessed defaults or
address-based identities. PEP 695 generic type parameters remain unsupported
and produce an explicit error rather than losing their bounds or defaults.

Maintained stubs select declarations and signatures; runtime documentation
supplies prose, with maintained stub documentation as a fallback. Stub-only
declarations can retain their own prose and need no runtime counterpart.
A failed runtime-backed import remains an error even when
stubs are present. Missing dependencies never trigger a static command fallback.

## Documentation and examples

Autodoc and Napoleon process reStructuredText, NumPy, and Google docstrings.
Supported doctree nodes become common document IR: prose, headings, entries,
lists, code, admonitions, math, and symbolic references. Unsupported nodes and
legacy Markdown-only syntax remain visible with diagnostics. Raw HTML gains no
rendering authority.

References retain their spelling and labels for workspace resolution through
canonical names, aliases, lexical contexts, and class bases. Unavailable
private or external references render as code with diagnostics; ambiguous
references remain errors. Sphinx does not choose final site routes.

Original decoded docstrings remain separate from processed prose. Native
doctests use this original text and the interpreter selected for discovery.
Each doctest imports the module that defined the docstring and uses its globals.
Examples in maintained stubs require an importable runtime module.
Inherited examples from outside the checkout remain display-only. `check` does
not execute examples. `extract`, `build`, and `serve` use fresh processes per API
document and retain output, expected exceptions, options, and explicit skips.

Source parsers retain exact maintained-source mappings when proven. Generated
or runtime-attached documentation can have only file-level evidence. The
adapter does not manufacture source declarations or copy temporary parser
offsets into snapshots.

## Evidence, reuse, and snapshots

Native extraction records `ExtractionMode::Native` and completed
`PythonRuntime` evidence: target, policy, Python/Sphinx/Docutils versions,
distribution versions, and contributing inputs. `PythonExports::Runtime`
distinguishes observed exports, and `PythonModuleSource::Runtime` represents
modules without maintained source or stubs. Documents use `python-docstring`.

Discovery and doctests have separate persistent entries. Reuse requires
matching package inputs, declared dependencies, interpreter selection, installed
dependency contents, native libraries, and environment inputs. Assembly observes
dependency bytes before discovery imports the package and verifies them afterward.
It tracks both symlinked import search paths and their resolved targets.
Publication also checks checkout files read during discovery, including
import-time data. Declare data outside the default inventory with `extra-inputs`
to enable cache reuse for those reads. Uncertain coverage disables reuse.
Observation owners remain live through publication checks and supply watch inputs.
Failures preserve the previous publication.

IR schema 16 includes this evidence; schema 15 snapshots require fresh
extraction. Snapshot validation verifies completion and input/tool associations
without Python. Detached `generate` needs no producer checkout or runtime.

`parse_target` and legacy source-inspection helpers remain process-free. Their
static diagnostics describe source inspection; commands no longer use that
interpretation as a fallback. Parser and normalization checks remain in the
default suite. Slow native acceptance runs in the `incremental-native` CI job;
see [Contributing](https://github.com/jolars/diplodocus/blob/main/CONTRIBUTING.md).
