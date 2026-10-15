# Portable snapshots

`extract` writes a self-contained SQLite snapshot containing the structured
documentation, presentation settings, provenance, diagnostics, and validated
local assets. `generate` reads a completed snapshot and writes the site without
the original source checkout, language runtimes, or Jupyter kernels. `build`
combines these two stages. See the [command reference](cli.md) for paths and
options.

The snapshot manifest has separate storage, documentation IR, and record
encoding versions. The current reader accepts storage version 2, IR version 5,
and encoding version 1. It rejects other combinations rather than migrating
them. If a newer Diplodocus cannot read an older snapshot, extract a new one
from the original sources before generating the site. The producer's package
version is recorded, but is not itself the compatibility rule.

Extraction replaces the complete snapshot. It keeps stable semantic IDs for
retained entities and drops removed entities and unused assets. A failed
extraction or publication leaves the previous completed snapshot in place. Treat
a copied snapshot as a build artifact; do not edit its records by hand. The
[storage
schema](https://github.com/jolars/diplodocus/blob/main/docs/design/snapshot-schema.md)
in the source repository describes its tables and validation rules.

## Use a snapshot in an existing site

SQLite is a supported consumption boundary. Read the manifest's storage, IR,
and encoding versions before interpreting records. Package and item records
contain structured JSON; documents retain semantic link targets rather than
site URLs. The `assets` table carries retained image and download bytes. A
consumer can query these records directly and choose its own presentation.

An experimental Rust API, `diplodocus::rendering::render_fragments`, reuses the
built-in document renderer without its page shell. To try it from this checkout:

```console
cargo run --locked --example export-fragments -- \
  --input documentation.sqlite --output fragments
```

The output includes document bodies, `documents.json` with semantic identities
and heading metadata, search records, syntax colors, and retained assets. Links
remain relative to the exported document paths. The host must map those links
when it supplies different routes, and provide its own layout and navigation.
The fragment export format is experimental and does not change the SQLite
storage contract. The [Eunoia experiment](https://github.com/jolars/diplodocus/tree/main/examples/eunoia)
shows an existing SvelteKit site consuming both SQL records and fragments.
