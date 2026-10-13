# Contributing

Read the document that owns the behavior:

- [`spec/`](spec/README.md) is normative for clients, registry tooling, and
  conformance behavior.
- [`TRUST.md`](TRUST.md) constrains integrity, authentication, licensing, and
  preservation claims.

## Adding a dataset

1. Choose a canonical `(source, name, version)` identity. If artifact identity
   or the representation recipe differs, use a new dataset version.
2. Draft a manifest from the schema. Supply descriptive, provenance, license,
   artifact, location, decoder, and task metadata.
3. Let the tool retrieve and verify every location:

   ```console
   tool draft.yaml \
     --output registry/datasets/example.yaml
   ```

   The tool writes the manifest to stdout and a review report to stderr.
4. Review every decoded name and shape before committing the manifest.

<div>Unsupported content in the original contributor guide.</div>
