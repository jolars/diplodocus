# C++ extraction contract

The `doxygen` adapter translates Doxygen XML into shared API items, structured
C++ signatures, and documents. The package declares `ecosystem: cpp`, a metadata
Doxyfile, and an explicit Doxyfile extraction target. Doxygen runs from the
package root, so relative configuration paths use that directory.

## Native boundary

The caller supplies `doxygen` on `PATH` and prepares generated headers and
Doxyfiles. Diplodocus does not invoke CMake, compile the library, or install
dependencies. The development environment supplies Doxygen 1.17.0.

Both `check` and extraction invoke Doxygen. Native configuration normalization
expands Doxyfile includes and environment variables. A temporary wrapper requests
XML output, disables other output formats and diagrams, and redirects generated
files into private staging. The remaining preprocessing and extraction settings,
including custom input filters, retain their native meaning. Such filters run
with the caller's privileges.

Native commands have a ten-minute preparation deadline. Cancellation and timeout
terminate the supervised process group. Native failures, invalid XML, changed
inputs, and cleanup failures prevent snapshot publication.

The adapter fingerprints maintained package files, explicitly selected input and
include trees, and recursively included Doxyfiles. It excludes conventional build,
dependency, and generated site directories from broad discovery. Explicit input
roots can select prepared generated files. Local inputs must remain within the
declared repository. Input contents and inventory membership are rechecked after
native extraction and before publication. Preview watching observes the same
roots. Arbitrary undeclared files read by external filters are outside this
observation contract.

## API semantics

Supported declarations include namespaces, classes, structs, unions, enums,
enumerators, functions, methods, constructors, destructors, operators, fields,
variables, aliases, and macros. Templates, parameter defaults, base types, and
declaration qualifiers remain structured signature data. This is Doxygen's
source model, without a promise of compiler type resolution.

Effective visibility settings apply during XML translation. Doxygen's XML can
contain private declarations even when `EXTRACT_PRIVATE=NO`; the adapter filters
them rather than trusting XML membership alone. Protected members remain
documented. Explicitly enabled private extraction retains private members.

Callable names identify families. Each overload also has its own semantic item
and documentation. Overload keys use normalized parameter types, template
parameter kinds, and member qualifiers. Parameter names, defaults, source
positions, declaration order, and Doxygen reference IDs do not define identity.
Native reference IDs are used only while translating links.

Namespaces, named types, and free callable families have reference pages.
Members, enumerators, and overloads remain independently addressable while
rendering on their owning pages. Snapshot validation checks ownership cycles,
family membership, references, and native extraction evidence. IR version 14
carries the C++ additions; storage version 2 and record encoding version 1 remain
unchanged.

## Documentation

The `doxygen-xml` document format retains prose, formatting, lists, tables,
parameter descriptions, template parameters, returns, exceptions, notes,
references, and illustrative code. XML descriptions do not supply original
comment byte ranges, so the adapter does not invent them. Unsupported constructs
produce warnings and retain readable text.

Doxygen code blocks have display semantics. This adapter does not compile or
execute them. Standalone Doxygen page and group compounds and generated diagrams
are outside this version; authored guides use Diplodocus content collections.

Snapshots retain completed API documents, native tool versions, and input
evidence. `generate` reads those snapshots without Doxygen or the source checkout.
