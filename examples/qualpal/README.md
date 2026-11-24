# Qualpal and qualpalr bundle acceptance

Build Diplodocus with `cargo build --locked`, then run the recipe from the
Diplodocus checkout in a prepared environment:

```sh
python examples/qualpal/bundles.py
```

The recipe takes the current commit from each sibling checkout and stages its
tracked files in a temporary directory. Use `--qualpal` and `--qualpalr` for
other checkouts. The producer environment needs Doxygen and R with `pkgload`,
`pkgbuild`, `cpp11`, `roxygen2`, `evaluate`, and `jsonlite`. Supply a matching
prepared extension with `--qualpalr-dll` to skip compilation.

The recipe exports each API separately and builds with the Qualpal bundle
alongside the local R package. It then copies both bundles to a consumer
directory, extracts with an empty `PATH`, and generates from the relocated
snapshot after removing the consumer directory. The final site has C++ and R
package reference sections under `packages/`. The script prints the artifact
directory and leaves it available for inspection.

## Export and assemble in CI

The [composition workflow](../../.github/workflows/qualpal-bundles.yml) runs
Qualpal and qualpalr exports in separate jobs against full source commit SHAs.
The Qualpal job extracts API documentation with Doxygen. The qualpalr job
compiles the R package's native extension and runs its help examples. It uses
the C++ sources already included in qualpalr. Each job uploads a SQLite bundle
with the source SHA in both the artifact name and the bundle's repository
record. For qualpalr the recipe omits vignettes and uses a documentation patch
for the pinned release.

The [reusable producer
workflow](../../.github/workflows/qualpal-bundle-producer.yml) can also run from
each producer repository's own CI. Add this caller to
`.github/workflows/api-bundle.yml` in Qualpal, replacing both
`DIPLODOCUS_COMMIT` placeholders with the same full Diplodocus commit SHA. In
qualpalr, set `package: qualpalr`.

```yaml
name: Export API bundle

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  api-bundle:
    uses: jolars/diplodocus/.github/workflows/qualpal-bundle-producer.yml@DIPLODOCUS_COMMIT
    with:
      package: qualpal
      source-revision: ${{ github.sha }}
      diplodocus-revision: DIPLODOCUS_COMMIT
```

The composition workflow downloads the two artifacts from its own run into a
third job. That job checks out Diplodocus, composes the bundles with an empty
`PATH`, and uploads the generated HTML and Markdown as `qualpal-documentation`.
The checks cover links between the two references, C++ overloads, and figures
from the R help examples.

To assemble bundles downloaded from independent producer runs, use the same
Diplodocus revision that exported them and pass the two files to the recipe:

```sh
python3 examples/qualpal/bundles.py \
  --assemble bundles/qualpal.sqlite bundles/qualpalr.sqlite \
  --output assembly
```

The verified site is in `assembly/detached/site`.

To let a consumer fetch the bundles directly, attach each exported SQLite
file to a public release in its producer repository with
`gh release upload TAG FILE`. Record the producer commit and the result of
`sha256sum FILE` in the consumer's
[release bundle declarations](../../docs/guide/snapshots.md).
For checkout development, enable the local override for the package you are
editing and keep its site mount.
