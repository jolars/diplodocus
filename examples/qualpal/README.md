# Qualpal and qualpalr bundle acceptance

Build Diplodocus with `cargo build --locked`, then run the recipe from the
Diplodocus checkout in a prepared environment:

```console
python examples/qualpal/bundles.py
```

The recipe takes the current commit from each sibling checkout and stages its
tracked files in a temporary directory. Use `--qualpal` and `--qualpalr` for
other checkouts. The producer environment needs Doxygen and R with
`pkgload`, `pkgbuild`, `cpp11`, `roxygen2`, and `evaluate`. Supply a
matching prepared extension with `--qualpalr-dll` to skip compilation.

The recipe exports each API separately and builds with the Qualpal bundle
alongside the local R package. It then moves both bundles to a consumer
directory, extracts with an empty `PATH`, and generates from the relocated
snapshot after removing the consumer directory. The final site has C++ and R
package reference sections under `packages/`. The script prints the artifact
directory and leaves it available for inspection.
