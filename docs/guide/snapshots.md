# Portable snapshots

`extract` writes a self-contained SQLite snapshot containing the structured
documentation, presentation settings, provenance, diagnostics, and validated
local assets. `generate` reads a completed snapshot and writes the site
without the original source checkout, language runtimes, or Jupyter kernels.
`build` combines these two stages. See the [command reference](cli.md) for
paths and options.

The snapshot manifest has separate storage, documentation IR, and record
encoding versions. The current reader accepts storage version 2, IR version 1,
and encoding version 1. It rejects other combinations rather than migrating
them. If a newer Diplodocus cannot read an older snapshot, extract a new one
from the original sources before generating the site. The producer's package
version is recorded, but is not itself the compatibility rule.

Extraction replaces the complete snapshot. It keeps stable semantic IDs for
retained entities and drops removed entities and unused assets. A failed
extraction or publication leaves the previous completed snapshot in place.
Treat a copied snapshot as a build artifact; do not edit its records by hand.
The [storage schema](https://github.com/jolars/diplodocus/blob/main/docs/design/snapshot-schema.md)
in the source repository describes its tables and validation rules.
